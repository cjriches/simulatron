mod disk_interface;
mod disk_real;

pub use disk_interface::*;
pub use disk_real::RealDiskController;

// Mock implementation for testing.
#[cfg(test)]
mod disk_mock;
#[cfg(test)]
pub use disk_mock::MockDiskController;
