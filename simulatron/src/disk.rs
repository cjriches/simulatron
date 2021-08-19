mod disk_interface;
mod disk_mock;
mod disk_real;

pub use disk_interface::*;
pub use disk_mock::MockDiskController;
pub use disk_real::RealDiskController;
