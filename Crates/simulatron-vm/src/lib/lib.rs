mod cpu;
mod display;
mod disk;
mod keyboard;
mod mmu;
mod ram;
mod ui;

use std::sync::mpsc;

use crate::disk::RealDiskController;
pub use crate::mmu::ROM_SIZE;

pub fn run(rom: [u8; ROM_SIZE], disk_a_path: &str, disk_b_path: &str) {
    // Create communication channels.
    let (interrupt_tx, interrupt_rx) = mpsc::channel();
    let interrupt_tx_keyboard = interrupt_tx.clone();
    let interrupt_tx_mmu = interrupt_tx.clone();
    let interrupt_tx_disk_a = interrupt_tx.clone();
    let interrupt_tx_disk_b = interrupt_tx.clone();
    let (ui_tx, ui_rx) = mpsc::channel();
    let ui_tx_display = ui_tx.clone();
    let ui_tx_cpu = ui_tx.clone();
    let (keyboard_tx, keyboard_rx) = mpsc::channel();
    let keyboard_tx_ui = keyboard_tx.clone();

    // Create components.
    let disk_a = RealDiskController::new(
        disk_a_path,
        interrupt_tx_disk_a,
        cpu::INTERRUPT_DISK_A);
    let disk_b = RealDiskController::new(
        disk_b_path,
        interrupt_tx_disk_b,
        cpu::INTERRUPT_DISK_B);
    let display = display::DisplayController::new(ui_tx_display);
    let keyboard = keyboard::KeyboardController::new(
        keyboard_tx,
        keyboard_rx,
        interrupt_tx_keyboard);
    let mmu = mmu::MMU::new(interrupt_tx_mmu, disk_a, disk_b,
                            display, keyboard, rom);
    let mut cpu = cpu::CPU::new(ui_tx_cpu, mmu, interrupt_tx, interrupt_rx);
    let mut ui = ui::UI::new(ui_tx, ui_rx, keyboard_tx_ui);

    // Run the Simulatron.
    cpu.start();
    ui.run().unwrap();
    cpu.stop();
}
