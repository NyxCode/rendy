//!
//! The mighty triangle example.
//! This examples shows colord triangle on white background.
//! Nothing fancy. Just prove that `rendy` works.
//!

#![cfg_attr(
    not(any(
        feature = "dx12",
        feature = "gl",
        feature = "metal",
        feature = "vulkan"
    )),
    allow(unused)
)]
use {
    rendy_examples::*,
    genmesh::generators::{IndexedPolygon, SharedVertex},
    rand::{distributions::{Distribution, Uniform}, SeedableRng as _},
    rendy::{
        command::{Families, QueueId, RenderPassEncoder, DrawIndexedCommand},
        factory::Factory,
        graph::{render::*, GraphBuilder, GraphContext, NodeBuffer, NodeImage},
        hal::{self, Device as _, PhysicalDevice as _, pso::ShaderStageFlags},
        memory::Dynamic,
        mesh::{Mesh, Model, PosColorNorm},
        resource::{Buffer, BufferInfo, DescriptorSet, DescriptorSetLayout, Escape, Handle},
        shader::{ShaderSetBuilder, ShaderSet, SpirvShader},
        util::*,
    },
    std::{cmp::min, mem::size_of},
};

#[cfg(feature = "spirv-reflection")]
use rendy::shader::SpirvReflection;

#[cfg(not(feature = "spirv-reflection"))]
use rendy::mesh::AsVertex;

lazy_static::lazy_static! {
    static ref VERTEX: SpirvShader = SpirvShader::new(
        unsafe {
            let bytes = include_bytes!("vert.spv");
            std::slice::from_raw_parts(bytes.as_ptr() as *const u32, bytes.len() / 4).to_vec()
        },
        ShaderStageFlags::VERTEX,
        "main"
    );

    static ref FRAGMENT: SpirvShader = SpirvShader::new(
        unsafe {
            let bytes = include_bytes!("frag.spv");
            std::slice::from_raw_parts(bytes.as_ptr() as *const u32, bytes.len() / 4).to_vec()
        },
        ShaderStageFlags::FRAGMENT,
        "main",
    );

    static ref SHADERS: ShaderSetBuilder = ShaderSetBuilder::default()
        .with_vertex(&*VERTEX).unwrap()
        .with_fragment(&*FRAGMENT).unwrap();
}


#[cfg(feature = "spirv-reflection")]
lazy_static::lazy_static! {
    static ref SHADER_REFLECTION: SpirvReflection = SHADERS.reflect().unwrap();
}

#[derive(Clone, Copy, Debug)]
#[repr(C, align(16))]
struct Light {
    pos: nalgebra::Vector3<f32>,
    pad: f32,
    intencity: f32,
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct UniformArgs {
    proj: nalgebra::Matrix4<f32>,
    view: nalgebra::Matrix4<f32>,
    lights_count: i32,
    pad: [i32; 3],
    lights: [Light; MAX_LIGHTS],
}

#[derive(Debug)]
struct Camera {
    view: nalgebra::Projective3<f32>,
    proj: nalgebra::Perspective3<f32>,
}

#[derive(Debug)]
struct Scene<B: hal::Backend> {
    camera: Camera,
    object_mesh: Option<Mesh<B>>,
    objects: Vec<nalgebra::Transform3<f32>>,
    lights: Vec<Light>,
}

const MAX_LIGHTS: usize = 32;
const MAX_OBJECTS: usize = 1024;
const UNIFORM_SIZE: u64 = size_of::<UniformArgs>() as u64;
const MODELS_SIZE: u64 = size_of::<Model>() as u64 * MAX_OBJECTS as u64;

rendy_without_gl_backend! {
    const INDIRECT_SIZE: u64 = size_of::<DrawIndexedCommand>() as u64;
}

rendy_with_gl_backend! {
    const INDIRECT_SIZE: u64 = 0;
}

fn buffer_frame_size(align: u64) -> u64 {
    ((UNIFORM_SIZE + MODELS_SIZE + INDIRECT_SIZE - 1) | (align - 1)) + 1
}

fn uniform_offset(index: usize, align: u64) -> u64 {
    buffer_frame_size(align) * index as u64
}

fn models_offset(index: usize, align: u64) -> u64 {
    uniform_offset(index, align) + UNIFORM_SIZE
}

rendy_without_gl_backend! {
    fn indirect_offset(index: usize, align: u64) -> u64 {
        models_offset(index, align) + MODELS_SIZE
    }
}

#[derive(Debug, Default)]
struct MeshRenderPipelineDesc;

#[derive(Debug)]
struct MeshRenderPipeline<B: hal::Backend> {
    align: u64,
    buffer: Escape<Buffer<B>>,
    sets: Vec<Escape<DescriptorSet<B>>>,
}

impl<B> SimpleGraphicsPipelineDesc<B, Scene<B>> for MeshRenderPipelineDesc
where
    B: hal::Backend,
{
    type Pipeline = MeshRenderPipeline<B>;

    fn load_shader_set(
        &self,
        factory: &mut Factory<B>,
        _scene: &Scene<B>,
    ) -> ShaderSet<B> {
        SHADERS.build(factory, Default::default()).unwrap()
    }

    fn vertices(
        &self,
    ) -> Vec<(
        Vec<hal::pso::Element<hal::format::Format>>,
        hal::pso::ElemStride,
        hal::pso::VertexInputRate,
    )> {
        #[cfg(feature = "spirv-reflection")]
        return vec![
            SHADER_REFLECTION
                .attributes(&["position", "color", "normal"])
                .unwrap()
                .gfx_vertex_input_desc(hal::pso::VertexInputRate::Vertex),
            SHADER_REFLECTION
                .attributes_range(3..7)
                .unwrap()
                .gfx_vertex_input_desc(hal::pso::VertexInputRate::Instance(1)),
        ];

        #[cfg(not(feature = "spirv-reflection"))]
        return vec![
            PosColorNorm::vertex().gfx_vertex_input_desc(hal::pso::VertexInputRate::Vertex),
            Model::vertex().gfx_vertex_input_desc(hal::pso::VertexInputRate::Instance(1)),
        ];
    }

    fn layout(&self) -> Layout {
        #[cfg(feature = "spirv-reflection")]
        return SHADER_REFLECTION.layout().unwrap();

        #[cfg(not(feature = "spirv-reflection"))]
        return Layout {
            sets: vec![SetLayout {
                bindings: vec![
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 0,
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::VERTEX,
                        immutable_samplers: false,
                    },
                    hal::pso::DescriptorSetLayoutBinding {
                        binding: 1,
                        ty: hal::pso::DescriptorType::UniformBuffer,
                        count: 1,
                        stage_flags: hal::pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                ],
            }],
            push_constants: Vec::new(),
        };
    }

    fn build<'a>(
        self,
        ctx: &GraphContext<B>,
        factory: &mut Factory<B>,
        _queue: QueueId,
        _scene: &Scene<B>,
        buffers: Vec<NodeBuffer>,
        images: Vec<NodeImage>,
        set_layouts: &[Handle<DescriptorSetLayout<B>>],
    ) -> Result<MeshRenderPipeline<B>, failure::Error> {
        assert!(buffers.is_empty());
        assert!(images.is_empty());
        assert_eq!(set_layouts.len(), 1);

        let frames = ctx.frames_in_flight as _;
        let align = factory
            .physical()
            .limits()
            .min_uniform_buffer_offset_alignment;

        let buffer = factory
            .create_buffer(
                BufferInfo {
                    size: buffer_frame_size(align) * frames as u64,
                    usage: hal::buffer::Usage::UNIFORM
                        | hal::buffer::Usage::INDIRECT
                        | hal::buffer::Usage::VERTEX,
                },
                Dynamic,
            )
            .unwrap();

        let mut sets = Vec::new();
        for index in 0..frames {
            unsafe {
                let set = factory
                    .create_descriptor_set(set_layouts[0].clone())
                    .unwrap();
                factory.write_descriptor_sets(vec![
                    hal::pso::DescriptorSetWrite {
                        set: set.raw(),
                        binding: 0,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Buffer(
                            buffer.raw(),
                            Some(uniform_offset(index, align))
                                ..Some(uniform_offset(index, align) + UNIFORM_SIZE),
                        )),
                    },
                    hal::pso::DescriptorSetWrite {
                        set: set.raw(),
                        binding: 1,
                        array_offset: 0,
                        descriptors: Some(hal::pso::Descriptor::Buffer(
                            buffer.raw(),
                            Some(uniform_offset(index, align))
                                ..Some(uniform_offset(index, align) + UNIFORM_SIZE),
                        )),
                    },
                ]);
                sets.push(set);
            }
        }

        Ok(MeshRenderPipeline {
            align,
            buffer,
            sets,
        })
    }
}

impl<B> SimpleGraphicsPipeline<B, Scene<B>> for MeshRenderPipeline<B>
where
    B: hal::Backend,
{
    type Desc = MeshRenderPipelineDesc;

    fn prepare(
        &mut self,
        factory: &Factory<B>,
        _queue: QueueId,
        _set_layouts: &[Handle<DescriptorSetLayout<B>>],
        index: usize,
        scene: &Scene<B>,
    ) -> PrepareResult {
        unsafe {
            factory
                .upload_visible_buffer(
                    &mut self.buffer,
                    uniform_offset(index, self.align),
                    &[UniformArgs {
                        pad: [0, 0, 0],
                        proj: scene.camera.proj.to_homogeneous(),
                        view: scene.camera.view.inverse().to_homogeneous(),
                        lights_count: scene.lights.len() as i32,
                        lights: {
                            let mut array = [Light {
                                pad: 0.0,
                                pos: nalgebra::Vector3::new(0.0, 0.0, 0.0),
                                intencity: 0.0,
                            }; MAX_LIGHTS];
                            let count = min(scene.lights.len(), 32);
                            array[..count].copy_from_slice(&scene.lights[..count]);
                            array
                        },
                    }],
                )
                .unwrap()
        };

        rendy_without_gl_backend! {
            unsafe {
                factory
                    .upload_visible_buffer(
                        &mut self.buffer,
                        indirect_offset(index, self.align),
                        &[DrawIndexedCommand {
                            index_count: scene.object_mesh.as_ref().unwrap().len(),
                            instance_count: scene.objects.len() as u32,
                            first_index: 0,
                            vertex_offset: 0,
                            first_instance: 0,
                        }],
                    )
                    .unwrap()
            }
        }

        if !scene.objects.is_empty() {
            unsafe {
                factory
                    .upload_visible_buffer(
                        &mut self.buffer,
                        models_offset(index, self.align),
                        &scene.objects[..],
                    )
                    .unwrap()
            };
        }

        PrepareResult::DrawRecord
    }

    fn draw(
        &mut self,
        layout: &B::PipelineLayout,
        mut encoder: RenderPassEncoder<'_, B>,
        index: usize,
        scene: &Scene<B>,
    ) {
        unsafe {
            encoder.bind_graphics_descriptor_sets(
                layout,
                0,
                Some(self.sets[index].raw()),
                std::iter::empty(),
            );

            #[cfg(feature = "spirv-reflection")]
            let vertex = [SHADER_REFLECTION
                .attributes(&["position", "color", "normal"])
                .unwrap()];

            #[cfg(not(feature = "spirv-reflection"))]
            let vertex = [PosColorNorm::vertex()];

            scene
                .object_mesh
                .as_ref()
                .unwrap()
                .bind(0, &vertex, &mut encoder)
                .unwrap();

            encoder.bind_vertex_buffers(
                1,
                std::iter::once((self.buffer.raw(), models_offset(index, self.align))),
            );

            rendy_without_gl_backend! {
                encoder.draw_indexed_indirect(
                    self.buffer.raw(),
                    indirect_offset(index, self.align),
                    1,
                    INDIRECT_SIZE as u32,
                );
            }

            rendy_with_gl_backend! {
                encoder.draw_indexed(
                    0 .. scene.object_mesh.as_ref().unwrap().len(),
                    0,
                    0 .. scene.objects.len() as u32
                );
            }
        }
    }

    fn dispose(self, _factory: &mut Factory<B>, _scene: &Scene<B>) {}
}


rendy_wasm32! {
    #[wasm_bindgen(start)]
    pub fn wasm_main() {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        main();
    }
}

fn main() {
    run(|factory, families, surface, extent| {

        let mut graph_builder = GraphBuilder::<Backend, Scene<Backend>>::new();

        let kind = hal::image::Kind::D2(extent.width as u32, extent.height as u32, 1, 1);
        let aspect = extent.width / extent.height;

        let depth = graph_builder.create_image(
            kind,
            1,
            hal::format::Format::D32Sfloat,
            Some(hal::command::ClearValue::DepthStencil(
                hal::command::ClearDepthStencil(1.0, 0),
            )),
        );

        let pass = graph_builder.add_node(
            // SubpassBuilder::new()
            MeshRenderPipeline::builder()
                .into_subpass()
                .with_color_surface()
                .with_depth_stencil(depth)
                .into_pass()
                .with_surface(
                    surface,
                    extent,
                    Some(hal::command::ClearValue::Color([1.0, 1.0, 1.0, 1.0].into())),
                ),
        );

        let mut scene = Scene {
            camera: Camera {
                proj: nalgebra::Perspective3::new(aspect as f32, 3.1415 / 4.0, 1.0, 200.0),
                view: nalgebra::Projective3::identity() * nalgebra::Translation3::new(0.0, 0.0, 10.0),
            },
            object_mesh: None,
            objects: vec![],
            lights: vec![
                Light {
                    pad: 0.0,
                    pos: nalgebra::Vector3::new(0.0, 0.0, 0.0),
                    intencity: 10.0,
                },
                Light {
                    pad: 0.0,
                    pos: nalgebra::Vector3::new(0.0, 20.0, -20.0),
                    intencity: 140.0,
                },
                Light {
                    pad: 0.0,
                    pos: nalgebra::Vector3::new(-20.0, 0.0, -60.0),
                    intencity: 100.0,
                },
                Light {
                    pad: 0.0,
                    pos: nalgebra::Vector3::new(20.0, -30.0, -100.0),
                    intencity: 160.0,
                },
            ],
        };

        log::info!("{:#?}", scene);

        let graph = graph_builder
            .with_frames_in_flight(3)
            .build(factory, families, &scene)
            .unwrap();

        let icosphere = genmesh::generators::IcoSphere::subdivide(4);
        let indices: Vec<_> = genmesh::Vertices::vertices(icosphere.indexed_polygon_iter())
            .map(|i| i as u32)
            .collect();
        let vertices: Vec<_> = icosphere
            .shared_vertex_iter()
            .map(|v| PosColorNorm {
                position: v.pos.into(),
                color: [
                    (v.pos.x + 1.0) / 2.0,
                    (v.pos.y + 1.0) / 2.0,
                    (v.pos.z + 1.0) / 2.0,
                    1.0,
                ]
                .into(),
                normal: v.normal.into(),
            })
            .collect();

        scene.object_mesh = Some(
            Mesh::<Backend>::builder()
                .with_indices(&indices[..])
                .with_vertices(&vertices[..])
                .build(graph.node_queue(pass), &factory)
                .unwrap(),
        );

        let mut rng = rand::rngs::SmallRng::seed_from_u64(12478493548762156);
        let rxy = Uniform::new(-1.0, 1.0);
        let rz = Uniform::new(0.0, 185.0);

        (graph, scene, move |_: &mut Factory<Backend>, _: &mut Families<Backend>, scene: &mut Scene<Backend>| {
            if scene.objects.len() >= MAX_OBJECTS {
                return false;
            }
            scene.objects.push({
                let z = rz.sample(&mut rng);
                nalgebra::Transform3::identity()
                    * nalgebra::Translation3::new(
                        rxy.sample(&mut rng) * (z / 2.0 + 4.0),
                        rxy.sample(&mut rng) * (z / 2.0 + 4.0),
                        -z,
                    )
            });
            true
        })
    })
}
