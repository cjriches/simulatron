mod char_mapping;
mod cpu;
mod display;
mod keyboard;
mod mmu;
mod ram;
mod rom;
mod ui;

use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc;

pub struct Simulatron {
    display: Rc<display::DisplayController>,
    keyboard: Rc<RefCell<keyboard::KeyboardController>>,
    ram: Rc<ram::RAM>,
    rom: Rc<rom::ROM>,
    mmu: mmu::MMU,
    ui: ui::UI,
}

impl Simulatron {
    pub fn new() -> Self {
        // Create communication channels.
        let (interrupt_tx_keyboard, interrupt_rx) = mpsc::channel();
        let interrupt_tx_mmu = interrupt_tx_keyboard.clone();
        let (display_tx, display_rx) = mpsc::channel();
        let display_tx_ui = display_tx.clone();
        let (keyboard_tx, keyboard_rx) = mpsc::channel();
        let keyboard_tx_ui = keyboard_tx.clone();

        // Create components.
        let display = Rc::new(
            display::DisplayController::new(display_tx));
        let keyboard = Rc::new(RefCell::new(keyboard::KeyboardController::new(
            keyboard_tx, keyboard_rx, interrupt_tx_keyboard)));
        let ram = Rc::new(ram::RAM::new());
        let rom = Rc::new(rom::ROM::new());
        let mmu = mmu::MMU::new(interrupt_tx_mmu, Rc::clone(&display), Rc::clone(&keyboard),
                                Rc::clone(&ram), Rc::clone(&rom));
        let ui = ui::UI::new(display_tx_ui, display_rx, keyboard_tx_ui);

        Simulatron {
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
