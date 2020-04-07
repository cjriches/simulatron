mod char_mapping;
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
    display: Rc<display::DisplayController>,
    keyboard: Rc<RefCell<keyboard::KeyboardController>>,
    ram: Rc<RefCell<ram::RAM>>,
    rom: Rc<rom::ROM>,
    mmu: mmu::MMU,
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
        let cpu = cpu::CPU::new(interrupt_rx);
        let disk_a = Rc::new(RefCell::new(disk::DiskController::new(
            String::from("DiskA"), interrupt_tx_disk_a)));
        let disk_b = Rc::new(RefCell::new(disk::DiskController::new(
            String::from("DiskB"), interrupt_tx_disk_b)));
        let display = Rc::new(display::DisplayController::new(display_tx));
        let keyboard = Rc::new(RefCell::new(keyboard::KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard)));
        let ram = Rc::new(RefCell::new(ram::RAM::new()));
        let rom = Rc::new(rom::ROM::new());
        let mmu = mmu::MMU::new(interrupt_tx_mmu, Rc::clone(&disk_a), Rc::clone(&disk_b),
                                Rc::clone(&display), Rc::clone(&keyboard),
                                Rc::clone(&ram), Rc::clone(&rom));
        let ui = ui::UI::new(display_tx_ui, display_rx, keyboard_tx_ui);

        Simulatron {
            cpu,
            disk_a,
            disk_b,
            display,
            keyboard,
            ram,
            rom,
            mmu,
            ui,
        }
    }

    pub fn run(&mut self) {
        self.keyboard.borrow_mut().start();

        self.ui.run();

        self.keyboard.borrow_mut().stop();
    }
}
