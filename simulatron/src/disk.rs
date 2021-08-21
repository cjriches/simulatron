mod disk_interface;
mod disk_real;

pub use disk_interface::*;
pub use disk_real::RealDiskController;

// Testing implementations.
#[cfg(test)]
mod disk_memory;
#[cfg(test)]
pub use disk_memory::MemDiskController;
#[cfg(test)]
mod disk_mock;
#[cfg(test)]
pub use disk_mock::MockDiskController;
