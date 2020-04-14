mod cpu;
mod display;
mod disk;
mod keyboard;
mod mmu;
mod ram;
mod rom;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

pub struct Simulatron {
    cpu: cpu::CPU,
    disk_a: Rc<RefCell<disk::DiskController>>,
    disk_b: Rc<RefCell<disk::DiskController>>,
    keyboard: Rc<RefCell<keyboard::KeyboardController>>,
    ui: ui::UI,
}

impl Simulatron {
    pub fn new() -> Self {
        // Create communication channels.
        let (interrupt_tx_keyboard, interrupt_rx) = mpsc::channel();
        let interrupt_tx_mmu = interrupt_tx_keyboard.clone();
        let interrupt_tx_disk_a = interrupt_tx_keyboard.clone();
        let interrupt_tx_disk_b = interrupt_tx_keyboard.clone();
        let (display_tx, display_rx) = mpsc::channel();
        let display_tx_ui = display_tx.clone();
        let (keyboard_tx, keyboard_rx) = mpsc::channel();
        let keyboard_tx_ui = keyboard_tx.clone();

        // Create components.
        let disk_a = Rc::new(RefCell::new(disk::DiskController::new(
            String::from("DiskA"), interrupt_tx_disk_a, cpu::INTERRUPT_DISK_A)));
        let disk_b = Rc::new(RefCell::new(disk::DiskController::new(
            String::from("DiskB"), interrupt_tx_disk_b, cpu::INTERRUPT_DISK_B)));
        let display = display::DisplayController::new(display_tx);
        let keyboard = Rc::new(RefCell::new(keyboard::KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard)));
        let ram = ram::RAM::new();
        let rom = rom::ROM::new();
        let mmu = mmu::MMU::new(interrupt_tx_mmu, Rc::clone(&disk_a), Rc::clone(&disk_b),
                                display, Rc::clone(&keyboard), ram, rom);
        let ui = ui::UI::new(display_tx_ui, display_rx, keyboard_tx_ui);
        let cpu = cpu::CPU::new(mmu, interrupt_rx);

        Simulatron {
            cpu,
            disk_a,
            disk_b,
            keyboard,
            ui,
        }
    }

    pub fn run(&mut self) {
        self.keyboard.borrow_mut().start();
        self.disk_a.borrow_mut().start();
        self.disk_b.borrow_mut().start();
        self.cpu.start();

        self.ui.run();

        self.cpu.stop();
        self.disk_b.borrow_mut().stop();
        self.disk_a.borrow_mut().stop();
        self.keyboard.borrow_mut().stop();
    }
}
