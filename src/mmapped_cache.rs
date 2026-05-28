use crate::mmapped_cache::InsertError::NoSpace;
use region::{Allocation, Protection};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::Hash;
use thiserror::Error;

pub struct MMappedCache<K: Eq + Hash> {
    mem: Allocation,
    len: usize,
    k_v_map: HashMap<K, (usize, usize)>,
}

#[derive(Error, Debug)]
pub enum InsertError {
    NoSpace(usize, usize),
}

impl Display for InsertError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            NoSpace(requested, have) => {
                write!(f, "No Space, requested {requested}, have {have}")
            }
        }
    }
}

impl<K: Eq + Hash> MMappedCache<K> {
    pub fn new(size: usize) -> Result<Self, region::Error> {
        let mem = region::alloc(size, Protection::READ_WRITE)?;
        Ok(Self {
            mem,
            len: 0,
            k_v_map: HashMap::new(),
        })
    }

    pub fn insert(&mut self, key: K, buf: &[u8]) -> Result<(), InsertError> {
        let count = std::cmp::min(self.mem.len() - self.len, buf.len());
        if count < buf.len() {
            return Err(NoSpace(buf.len(), count));
        }
        let offset = self.len;
        self.len += count;
        let ptr = self.mem.as_mut_ptr::<u8>().wrapping_add(offset);
        // SAFETY:
        // - ptr is non-null, and byte-aligned.
        // - the referenced range is within the allocation bounds.
        // - no aliasing of the referenced range because:
        //    - we are in an exclusive reference to self
        //    - we could not have handed out a reference to this range yet (because it doesn't exist)
        //    - the slice we are constructing is dropped on return (no reference handed out)
        let target = unsafe { std::slice::from_raw_parts_mut(ptr, count) };
        target.copy_from_slice(buf);
        self.k_v_map.insert(key, (offset, count));
        Ok(())
    }

    pub fn get(&self, key: &K) -> Option<&[u8]> {
        self.k_v_map.get(key).map(|(offset, len)| {
            let ptr = self.mem.as_ptr::<u8>().wrapping_add(*offset);
            // SAFETY
            // - ptr is non-null, aligned
            // - the referenced range is within the allocation bounds.
            // - no mutable aliases of the referenced range because:
            //    - we hold an immutable ref to self, and we only hand out immutable
            //      references, whose lifetimes are tied to self
            unsafe { std::slice::from_raw_parts(ptr, *len) }
        })
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.k_v_map.contains_key(key)
    }

    pub fn size(&self) -> usize {
        self.len
    }
}

// SAFETY:
// The memory pointed to by the *const T in Allocation is virtual memory
// allocated in this process, and valid across all threads.
unsafe impl<K: Eq + Hash + Send> Send for MMappedCache<K> {}

// SAFETY:
// It's safe for multiple threads to operate on MMappedCache simultaneously because it has no interior mutability.
unsafe impl<K: Eq + Hash + Sync> Sync for MMappedCache<K> {}
