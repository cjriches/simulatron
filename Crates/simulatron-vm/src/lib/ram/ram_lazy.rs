use ahash::AHashMap;
use std::ops::{Index, IndexMut};

use crate::mmu::RAM_SIZE;

const PAGE_SHIFT: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: usize = PAGE_SIZE - 1;
const NUM_PAGES: usize = (RAM_SIZE >> PAGE_SHIFT) + if RAM_SIZE % PAGE_SIZE > 0 { 1 } else { 0 };

/// Lazy RAM implementation: hashmap from page number to page.
pub struct RAM {
    /// `ahash` is faster than the standard hasher, and cryptographic security
    /// doesn't matter here.
    data: AHashMap<usize, Vec<u8>>,
}

impl RAM {
    pub fn new() -> Self {
        Self {
            data: AHashMap::with_capacity(NUM_PAGES),
        }
    }

    /// Return an immutable reference to the given RAM index.
    fn get(&self, index: usize) -> &u8 {
        match self.data.get(&(index >> PAGE_SHIFT)) {
            Some(page) => &page[index & PAGE_MASK],
            None => &0,
        }
    }

    //noinspection RsSelfConvention
    /// Return a mutable reference to the given RAM index.
    fn get_mut(&mut self, index: usize) -> &mut u8 {
        let page = self
            .data
            .entry(index >> PAGE_SHIFT)
            .or_insert_with(|| vec![0; PAGE_SIZE]);
        &mut page[index & PAGE_MASK]
    }
}

impl Index<usize> for RAM {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index)
    }
}

impl IndexMut<usize> for RAM {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index)
    }
}
