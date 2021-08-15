use super::disk_interface::*;

pub struct MockDiskController;

impl DiskController for MockDiskController {
    fn start(&mut self) {
        // no-op
    }

    fn stop(&mut self) {
        // no-op
    }

    fn store_control(&mut self, _address: u32, _value: u8) {
        // no-op
    }

    fn load_status(&self, _address: u32) -> u8 {
        return 0;
    }

    fn store_data(&mut self, _address: u32, _value: u8) {
        // no-op
    }

    fn load_data(&self, _address: u32) -> u8 {
        return 0;
    }
}
