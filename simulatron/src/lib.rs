mod cpu;
mod display;
mod disk;
mod keyboard;
mod mmu;
mod ram;
mod rom;
mod ui;

use std::sync::{Arc, Mutex, mpsc};

pub struct Simulatron {
    interrupt_tx: mpsc::Sender<u32>,
    cpu: Option<cpu::CPU>,
    disk_a: Arc<Mutex<disk::DiskController>>,
    disk_b: Arc<Mutex<disk::DiskController>>,
    keyboard: Arc<Mutex<keyboard::KeyboardController>>,
    ui: ui::UI,
}

impl Simulatron {
    pub fn new() -> Self {
        // Create communication channels.
        let (interrupt_tx, interrupt_rx) = mpsc::channel();
        let interrupt_tx_keyboard = interrupt_tx.clone();
        let interrupt_tx_mmu = interrupt_tx.clone();
        let interrupt_tx_disk_a = interrupt_tx.clone();
        let interrupt_tx_disk_b = interrupt_tx.clone();
        let interrupt_tx_cpu = interrupt_tx.clone();
        let (ui_tx, ui_rx) = mpsc::channel();
        let ui_tx_display = ui_tx.clone();
        let ui_tx_cpu = ui_tx.clone();
        let (keyboard_tx, keyboard_rx) = mpsc::channel();
        let keyboard_tx_ui = keyboard_tx.clone();

        // Set up test ROM.
        let mut test_rom = [0; 512];
        test_rom[0] = 0x86;  // Copy literal
        test_rom[4] = 0x24;  // character '$'
        test_rom[8] = 0x10;  // into r0b.

        test_rom[9] = 0x82;  // Store
        test_rom[13] = 0x10; // r0b into
        test_rom[16] = 0x02; // address of first display character.
        test_rom[17] = 0x40;

        test_rom[18] = 0x86; // Copy literal
        test_rom[21] = 0x50; // address 0x00005000
        test_rom[26] = 0x22; // into kspr.

        test_rom[27] = 0x86; // Copy literal
        test_rom[30] = 0x40; // address 0x00004000
                             // into r0.

        test_rom[36] = 0x82; // Store
                             // r0 into
        test_rom[44] = 0x04; // keyboard interrupt handler.

        test_rom[45] = 0x86; // Copy literal
        test_rom[49] = 0x05; // instruction IRETURN
        test_rom[53] = 0x10; // into r0b.

        test_rom[54] = 0x82; // Store
        test_rom[58] = 0x10; // r0b into
        test_rom[61] = 0x40; // literal address 0x00004000.

        test_rom[63] = 0x86; // Copy literal
        test_rom[67] = 0x02; // keyboard interrupt only
        test_rom[71] = 0x24; // into imr.

        test_rom[72] = 0x01; // Pause.

        test_rom[73] = 0x80; // Load from literal
        test_rom[76] = 0x19; // address of key buffer
        test_rom[77] = 0xB0;
        test_rom[81] = 0x10; // into r0b.

        test_rom[82] = 0x82; // Store
        test_rom[86] = 0x10; // r0b into
        test_rom[89] = 0x02; // address of second display character.
        test_rom[90] = 0x41;

        test_rom[91] = 0x01; // Pause.

        test_rom[92] = 0x00; // Halt.

        // Create components.
        let disk_a = Arc::new(Mutex::new(disk::DiskController::new(
            String::from("DiskA"), interrupt_tx_disk_a, cpu::INTERRUPT_DISK_A)));
        let disk_b = Arc::new(Mutex::new(disk::DiskController::new(
            String::from("DiskB"), interrupt_tx_disk_b, cpu::INTERRUPT_DISK_B)));
        let display = display::DisplayController::new(ui_tx_display);
        let keyboard = Arc::new(Mutex::new(keyboard::KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard)));
        let ram = ram::RAM::new();
        let rom = rom::ROM::new(test_rom);
        let mmu = mmu::MMU::new(interrupt_tx_mmu, Arc::clone(&disk_a), Arc::clone(&disk_b),
                                display, Arc::clone(&keyboard), ram, rom);
        let cpu = Some(cpu::CPU::new(ui_tx_cpu, mmu, interrupt_tx_cpu, interrupt_rx));
        let ui = ui::UI::new(ui_tx, ui_rx, keyboard_tx_ui);

        Simulatron {
            interrupt_tx,
            cpu,
            disk_a,
            disk_b,
            keyboard,
            ui,
        }
    }

    pub fn run(&mut self) {
        self.keyboard.lock().unwrap().start();
        self.disk_a.lock().unwrap().start();
        self.disk_b.lock().unwrap().start();
        let cpu_thread = self.cpu.take().unwrap().start();

        self.ui.run();

        self.interrupt_tx.send(cpu::JOIN_THREAD).unwrap();
        self.cpu = Some(cpu_thread.join().unwrap());
        self.disk_b.lock().unwrap().stop();
        self.disk_a.lock().unwrap().stop();
        self.keyboard.lock().unwrap().stop();
    }
}
