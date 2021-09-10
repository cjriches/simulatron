mod cpu;
mod display;
mod disk;
mod keyboard;
mod mmu;
mod ram;
mod ui;

use std::sync::mpsc;

use crate::disk::RealDiskController;
use crate::mmu::ROM_SIZE;

pub fn run() {
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

    // Set up test ROM.
    let mut test_rom = [0; ROM_SIZE];
    // Set the whole screen.
    test_rom[0] = 0x0A;  // Copy literal
    test_rom[1] = 0x10;  // into r0b
    test_rom[2] = 0x24;  // character '$'.

    test_rom[3] = 0x08;  // Store into
    test_rom[4] = 0x00;
    test_rom[5] = 0x00;
    test_rom[6] = 0x02;
    test_rom[7] = 0x40;  // first display character
    test_rom[8] = 0x10;  // r0b.

    test_rom[9] = 0x0A;  // Copy literal
    test_rom[10] = 0x22; // into kspr
    test_rom[11] = 0x00;
    test_rom[12] = 0x00;
    test_rom[13] = 0x50;
    test_rom[14] = 0x00; // address 0x00005000.

    test_rom[15] = 0x0A; // Copy literal
    test_rom[16] = 0x00; // into r0
    test_rom[17] = 0x00;
    test_rom[18] = 0x00;
    test_rom[19] = 0x40;
    test_rom[20] = 0x00; // address 0x00004000.

    test_rom[21] = 0x08; // Store into
    test_rom[22] = 0x00;
    test_rom[23] = 0x00;
    test_rom[24] = 0x00;
    test_rom[25] = 0x0C; // keyboard interrupt handler
    test_rom[26] = 0x00; // r0.

    test_rom[27] = 0x0A; // Copy literal
    test_rom[28] = 0x11; // into r1b
    test_rom[29] = 0x05; // instruction IRETURN.

    test_rom[30] = 0x09; // Store into
    test_rom[31] = 0x00; // address in r0
    test_rom[32] = 0x11; // r1b.

    test_rom[33] = 0x0A; // Copy literal
    test_rom[34] = 0x24; // into imr
    test_rom[35] = 0x00;
    test_rom[36] = 0x08; // keyboard interrupt only.

    test_rom[37] = 0x01; // Pause.

    test_rom[38] = 0x06; // Load
    test_rom[39] = 0x10; // into r0b
    test_rom[40] = 0x00;
    test_rom[41] = 0x00;
    test_rom[42] = 0x19;
    test_rom[43] = 0xB0; // key buffer.

    test_rom[44] = 0x08; // Store into
    test_rom[45] = 0x00;
    test_rom[46] = 0x00;
    test_rom[47] = 0x02;
    test_rom[48] = 0x41; // second display character
    test_rom[49] = 0x10; // r0b.

    test_rom[50] = 0x01; // Pause.

    // Create components.
    let disk_a = RealDiskController::new(
        String::from("DiskA"),
        interrupt_tx_disk_a,
        cpu::INTERRUPT_DISK_A);
    let disk_b = RealDiskController::new(
        String::from("DiskB"),
        interrupt_tx_disk_b,
        cpu::INTERRUPT_DISK_B);
    let display = display::DisplayController::new(ui_tx_display);
    let keyboard = keyboard::KeyboardController::new(
        keyboard_tx,
        keyboard_rx,
        interrupt_tx_keyboard);
    let mmu = mmu::MMU::new(interrupt_tx_mmu, disk_a, disk_b,
                            display, keyboard, test_rom);
    let mut cpu = cpu::CPU::new(ui_tx_cpu, mmu, interrupt_tx, interrupt_rx);
    let mut ui = ui::UI::new(ui_tx, ui_rx, keyboard_tx_ui);

    // Run the Simulatron.
    cpu.start();
    ui.run().unwrap();
    cpu.stop();
}
