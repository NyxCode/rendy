//!
//! 

#![forbid(overflowing_literals)]
#![warn(missing_copy_implementations)]
#![warn(missing_debug_implementations)]
#![warn(missing_docs)]
#![warn(intra_doc_link_resolution_failure)]
#![warn(path_statements)]
#![warn(trivial_bounds)]
#![warn(type_alias_bounds)]
#![warn(unconditional_recursion)]
#![warn(unions_with_drop_fields)]
#![warn(while_true)]
#![warn(unused)]
#![warn(bad_style)]
#![warn(future_incompatible)]
#![warn(rust_2018_compatibility)]
#![warn(rust_2018_idioms)]

use std::borrow::Cow;

/// Chech if slice o f ordered values is sorted.
pub fn is_slice_sorted<T: Ord>(slice: &[T]) -> bool {
    is_slice_sorted_by_key(slice, |i| i)
}

/// Check if slice is sorted using ordered key and key extractor
pub fn is_slice_sorted_by_key<'a, T, K: Ord>(slice: &'a [T], f: impl Fn(&'a T) -> K) -> bool {
    if let Some((first, slice)) = slice.split_first() {
        let mut cmp = f(first);
        for item in slice {
            let item = f(item);
            if cmp > item {
                return false;
            }
            cmp = item;
        }
    }
    true
}

/// Cast vec of some arbitrary type into vec of bytes.
pub fn cast_vec<T: Copy>(mut vec: Vec<T>) -> Vec<u8> {
    let len = std::mem::size_of::<T>() * vec.len();
    let cap = std::mem::size_of::<T>() * vec.capacity();
    let ptr = vec.as_mut_ptr();
    std::mem::forget(vec);
    unsafe { Vec::from_raw_parts(ptr as _, len, cap) }
}

/// Cast slice of some arbitrary type into slice of bytes.
pub fn cast_slice<T>(slice: &[T]) -> &[u8] {
    let len = std::mem::size_of::<T>() * slice.len();
    let ptr = slice.as_ptr();
    unsafe { std::slice::from_raw_parts(ptr as _, len) }
}

/// Cast `cow` of some arbitrary type into `cow` of bytes.
pub fn cast_cow<T: Copy>(cow: Cow<'_, [T]>) -> Cow<'_, [u8]> {
    match cow {
        Cow::Borrowed(slice) => Cow::Borrowed(cast_slice(slice)),
        Cow::Owned(vec) => Cow::Owned(cast_vec(vec)),
    }
}
