use std::ops::{Index, IndexMut};

use crate::mmu::RAM_SIZE;

/// Eager RAM implementation: single monolithic vector.
pub struct RAM {
    data: Vec<u8>,
}

impl RAM {
    pub fn new() -> Self {
        Self {
            data: vec![0; RAM_SIZE],
        }
    }
}

impl Index<usize> for RAM {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        self.data.index(index)
    }
}

impl IndexMut<usize> for RAM {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.data.index_mut(index)
    }
}
