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
        let (display_tx, display_rx) = mpsc::channel();
        let display_tx_ui = display_tx.clone();
        let (keyboard_tx, keyboard_rx) = mpsc::channel();
        let keyboard_tx_ui = keyboard_tx.clone();

        // Create components.
        let disk_a = Arc::new(Mutex::new(disk::DiskController::new(
            String::from("DiskA"), interrupt_tx_disk_a, cpu::INTERRUPT_DISK_A)));
        let disk_b = Arc::new(Mutex::new(disk::DiskController::new(
            String::from("DiskB"), interrupt_tx_disk_b, cpu::INTERRUPT_DISK_B)));
        let display = display::DisplayController::new(display_tx);
        let keyboard = Arc::new(Mutex::new(keyboard::KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard)));
        let ram = ram::RAM::new();
        let rom = rom::ROM::new();
        let mmu = mmu::MMU::new(interrupt_tx_mmu, Arc::clone(&disk_a), Arc::clone(&disk_b),
                                display, Arc::clone(&keyboard), ram, rom);
        let ui = ui::UI::new(display_tx_ui, display_rx, keyboard_tx_ui);
        let cpu = Some(cpu::CPU::new(mmu, interrupt_rx));

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
