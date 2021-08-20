use super::*;

use ntest::{assert_about_eq, timeout};

use crate::{disk::MockDiskController, display::DisplayController,
            keyboard::{KeyboardController, KeyMessage, key_str_to_u8},
            mmu::MMU, ram::RAM, rom::ROM, ui::UICommand};

fn run(rom_data: [u8; 512], keypress: Option<KeyMessage>) -> (CPU<MockDiskController>, Vec<UICommand>) {
    // Create communication channels.
    let (interrupt_tx, interrupt_rx) = mpsc::channel();
    let interrupt_tx_keyboard = interrupt_tx.clone();
    let interrupt_tx_mmu = interrupt_tx.clone();
    let (ui_tx, ui_rx) = mpsc::channel();
    let ui_tx_display = ui_tx.clone();
    let (keyboard_tx, keyboard_rx) = mpsc::channel();
    let keyboard_tx_manual = keyboard_tx.clone();

    // Create components.
    let disk_a = MockDiskController;
    let disk_b = MockDiskController;
    let display = DisplayController::new(ui_tx_display);
    let keyboard = KeyboardController::new(
        keyboard_tx, keyboard_rx, interrupt_tx_keyboard);
    let ram = RAM::new();
    let rom = ROM::new(rom_data);
    let mmu = MMU::new(interrupt_tx_mmu, disk_a, disk_b,
                       display, keyboard, ram, rom);
    let mut cpu = CPU::new(ui_tx, mmu, interrupt_tx, interrupt_rx);

    // Run the CPU till halt.
    cpu.start();
    if let Some(message) = keypress {
        keyboard_tx_manual.send(message).unwrap();
    }
    cpu.wait_for_halt();
    // Collect any resulting UI commands.
    let ui_commands = ui_rx.try_iter().collect();
    return (cpu, ui_commands);
}

macro_rules! internal {
    ($cpu:ident) => { $cpu.internal.as_ref().unwrap() }
}

#[test]
#[timeout(100)]
fn test_halt() {
    // Simplest possible test; check the CPU halts immediately on opcode 0.
    let (_, ui_commands) = run([0; 512], None);
    assert_eq!(ui_commands.len(), 2);  // Enable and Disable messages.
}

#[test]
#[timeout(100)]
fn test_copy_literal() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x03;  // into r3
    rom[2] = 0x42;
    rom[3] = 0x06;
    rom[4] = 0x96;
    rom[5] = 0x96;  // some random number.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[3], 0x42069696);
}

#[test]
#[timeout(100)]
fn test_copy_reg() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x03;  // into r3
    rom[2] = 0x13;
    rom[3] = 0x57;
    rom[4] = 0x9B;
    rom[5] = 0xDF;  // some random number.

    rom[6] = 0x0B;  // Copy register
    rom[7] = 0x08;  // into r0h
    rom[8] = 0x0B;  // from r3h.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[3], 0x13579BDF);
    assert_eq!(internal!(cpu).r[0], 0x00009BDF);
}

#[test]
#[timeout(100)]
fn test_store_literal_address() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0x12;
    rom[3] = 0x34;
    rom[4] = 0x56;
    rom[5] = 0x78;  // some random number.

    rom[6] = 0x08;  // Store into
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x4A;
    rom[10] = 0xBC; // address 0x00004ABC
    rom[11] = 0x00; // r0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).mmu.load_physical_32(0x00004ABC), Ok(0x12345678));
}

#[test]
#[timeout(100)]
fn test_store_reg_address() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0xAB;
    rom[3] = 0xCD;
    rom[4] = 0xEF;
    rom[5] = 0x00;  // some random number.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x01;  // into r1
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x4A;
    rom[11] = 0xBC; // address 0x00004ABC.

    rom[12] = 0x09; // Store into
    rom[13] = 0x01; // address in r1
    rom[14] = 0x00; // r0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).mmu.load_physical_32(0x00004ABC), Ok(0xABCDEF00));
}

#[test]
#[timeout(100)]
fn test_load_literal_address() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x07;  // into r7
    rom[2] = 0xFF;
    rom[3] = 0xFF;
    rom[4] = 0xFF;
    rom[5] = 0xFF;  // some random number.

    rom[6] = 0x06;  // Load
    rom[7] = 0x17;  // into r7b
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x80; // ROM byte 0x40 (64).

    rom[64] = 0x55;

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0xFFFFFF55);
}

#[test]
#[timeout(100)]
fn test_load_reg_address() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x0E;  // into r6h
    rom[2] = 0xFF;
    rom[3] = 0xFF;  // some random number.

    rom[4] = 0x0A;  // Copy literal
    rom[5] = 0x00;  // into r0
    rom[6] = 0x00;
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x80;  // ROM byte 0x40 (64).

    rom[10] = 0x07; // Load
    rom[11] = 0x16; // into r6b
    rom[12] = 0x00; // address in r0.

    rom[64] = 0x34;

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[6], 0x0000FF34);
}

#[test]
#[timeout(100)]
fn test_swap_literal() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x66;  // some random number.

    rom[3] = 0x0C;  // Swap with literal address
    rom[4] = 0x10;  // r0b
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x40;
    rom[8] = 0x00;  // address 0x00004000.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x00000000);
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x00004000), Ok(0x66));
}

#[test]
#[timeout(100)]
fn test_swap_reg() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x77;  // some random number.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x01;  // into r1
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x50;
    rom[8] = 0x00;  // address 0x00005000.

    rom[9] = 0x0D;  // Swap with reg ref address
    rom[10] = 0x10; // r0b
    rom[11] = 0x01; // address in r1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x00000000);
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x00005000), Ok(0x77));
}

#[test]
#[timeout(100)]
fn test_kernel_stack() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into kspr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x80;
    rom[5] = 0x00;  // address 0x00008000.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x08;  // into r0h
    rom[8] = 0xFF;
    rom[9] = 0xFF;  // some random number.

    rom[10] = 0x0E; // Push to the stack
    rom[11] = 0x08; // r0h.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x10; // into r0b
    rom[14] = 0xAA; // some random number.

    rom[15] = 0x0E; // Push to the stack
    rom[16] = 0x10; // r0b.

    rom[17] = 0x0F; // Pop from the stack
    rom[18] = 0x09; // into r1h.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[1], 0x0000AAFF);
    assert_eq!(internal!(cpu).kspr, 0x00007FFF);
    assert_eq!(internal!(cpu).mmu.load_physical_32(0x00007FFC), Ok(0x00AAFFFF));
}

#[test]
#[timeout(100)]
fn test_user_mode() {
    let mut rom = [0; 512];
    // Store a number for the user process to find.
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x99;  // some random number.

    rom[3] = 0x08;  // Store into
    rom[4] = 0x00;
    rom[5] = 0x00;
    rom[6] = 0x40;
    rom[7] = 0x40;  // address 0x00004040
    rom[8] = 0x10;  // r0b.

    // Set the user stack pointer.
    rom[9] = 0x0A;  // Copy literal
    rom[10] = 0x21; // into uspr
    rom[11] = 0x00;
    rom[12] = 0x00;
    rom[13] = 0x00;
    rom[14] = 0x64; // virtual address 0x64.

    // Set the page directory pointer.
    rom[15] = 0x0A; // Copy literal
    rom[16] = 0x23; // into pdpr
    rom[17] = 0x00;
    rom[18] = 0x00;
    rom[19] = 0x00;
    rom[20] = 0xC0; // ROM byte 0x80 (128).

    // Write the user mode instructions to RAM.

    // (Load the number we left behind).
    rom[21] = 0x0A; // Copy literal
    rom[22] = 0x08; // into r0h
    rom[23] = 0x06; // LOAD instruction
    rom[24] = 0x17; // into r7b.

    rom[25] = 0x08; // Store into
    rom[26] = 0x00;
    rom[27] = 0x00;
    rom[28] = 0x40;
    rom[29] = 0x00; // address 0x00004000
    rom[30] = 0x08; // r0h.

    rom[31] = 0x0A; // Copy literal
    rom[32] = 0x00; // into r0
    rom[33] = 0x00;
    rom[34] = 0x00;
    rom[35] = 0x00;
    rom[36] = 0x40; // virtual address 0x40.

    rom[37] = 0x08; // Store into
    rom[38] = 0x00;
    rom[39] = 0x00;
    rom[40] = 0x40;
    rom[41] = 0x02; // address 0x00004002
    rom[42] = 0x00; // r0.

    // (Push that number to the user stack).
    rom[43] = 0x0A; // Copy literal
    rom[44] = 0x08; // into r0h
    rom[45] = 0x0E; // PUSH instruction
    rom[46] = 0x17; // register ref r7b.

    rom[47] = 0x08; // Store into
    rom[48] = 0x00;
    rom[49] = 0x00;
    rom[50] = 0x40;
    rom[51] = 0x06; // address 0x000040006
    rom[52] = 0x08; // r0h.

    // (Try and set imr. This will cause an illegal operation interrupt).
    rom[53] = 0x0A;
    rom[54] = 0x00; // into r0
    rom[55] = 0x0A; // COPY instruction
    rom[56] = 0x24; // register ref imr
    rom[57] = 0xFF;
    rom[58] = 0xFF; // some random number.

    rom[59] = 0x08; // Store into
    rom[60] = 0x00;
    rom[61] = 0x00;
    rom[62] = 0x40;
    rom[63] = 0x08; // address 0x00004008
    rom[64] = 0x00; // r0.

    // Enable the illegal operation interrupt. We won't set a handler, so it will jump to
    // memory location zero, which will contain HALT, so the machine will actually halt.
    // We do need to set kspr though.

    // Set the kernel stack pointer.
    rom[65] = 0x0A; // Copy literal
    rom[66] = 0x22; // into kspr
    rom[67] = 0x00;
    rom[68] = 0x00;
    rom[69] = 0xA0;
    rom[70] = 0x00; // address 0x0000A000.

    // Enable the interrupt.
    rom[71] = 0x0A; // Copy literal
    rom[72] = 0x24; // into imr
    rom[73] = 0x00;
    rom[74] = 0x40; // illegal operation interrupt only.

    // Push the user mode address to the stack.
    rom[75] = 0x0A; // Copy literal
    rom[76] = 0x00; // into r0
    rom[77] = 0x00;
    rom[78] = 0x00;
    rom[79] = 0x00;
    rom[80] = 0x00; // virtual address 0x0.

    rom[81] = 0x0E; // Push to the stack
    rom[82] = 0x00; // r0.

    // Almost forgot; we need to write the page table entry to main memory.
    // Sadly we can't put that in ROM, although the page directory entry will be.
    rom[83] = 0x0A; // Copy literal
    rom[84] = 0x00; // into r0
    rom[85] = 0x00;
    rom[86] = 0x00;
    rom[87] = 0x40;
    rom[88] = 0x1F; // Valid, Present, RWX entry at 0x00004000.

    rom[89] = 0x08; // Store into
    rom[90] = 0x00;
    rom[91] = 0x00;
    rom[92] = 0xB0;
    rom[93] = 0x00; // address 0x0000B000
    rom[94] = 0x00; // r0.

    // Enter user mode!
    rom[95] = 0x04;

    // Page directory entry.
    rom[128] = 0x00;
    rom[129] = 0x00;
    rom[130] = 0xB0;
    rom[131] = 0x01; // Valid entry at 0x0000B000.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    // Assert the user mode process stored in its stack correctly.
    assert_eq!(internal!(cpu).uspr, 0x00000063);
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x00004063), Ok(0x99));
}

#[test]
#[timeout(1000)]
fn test_keyboard() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into kspr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00;  // address 0x00005000.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x00;  // into r0
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x40;
    rom[11] = 0x00; // address 0x00004000.

    rom[12] = 0x08; // Store into
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0x00;
    rom[16] = 0x04; // keyboard interrupt handler
    rom[17] = 0x00; // r0.

    // Address 0x4000 is HALT, so we should halt on interrupt.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x24; // into imr
    rom[20] = 0x00;
    rom[21] = 0x02; // keyboard interrupt only.

    rom[22] = 0x01; // Pause (will only be reached if this happens before interrupt sent).
    rom[23] = 0x01; // Pause (should never happen, acts as a fail condition).

    const KEY: &str = "F";
    let (cpu, ui_commands) = run(rom,
                                 Some(KeyMessage::Key(KEY, false, false).unwrap()));
    assert_eq!(ui_commands.len(), 2);

    // Assert that the key was correctly detected.
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x19B0), Ok(key_str_to_u8(KEY).unwrap()));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x19B1), Ok(0));
}

#[test]
#[timeout(100)]
fn test_display() {
    let mut rom = [0; 512];
    // Set character 5,32 to '#'.
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x21;  // character '!'.

    rom[3] = 0x08;  // Store into
    rom[4] = 0x00;
    rom[5] = 0x00;
    rom[6] = 0x03;
    rom[7] = 0xF0;  // display cell (r5,c32)
    rom[8] = 0x10;  // r0b.

    // Set foreground colour 20, 50 to dark red.
    rom[9] = 0x0A;  // Copy literal
    rom[10] = 0x10; // into r0b
    rom[11] = 0x10; // RGB(85, 0, 0).

    rom[12] = 0x08;  // Store into
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0x10;
    rom[16] = 0x82;  // foreground colour cell (r20,c50)
    rom[17] = 0x10;  // r0b.

    // Set background colour 0, 0 to yellow.
    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x10; // into r0b
    rom[20] = 0x3C; // RGB(255, 255, 0).

    rom[21] = 0x08;  // Store into
    rom[22] = 0x00;
    rom[23] = 0x00;
    rom[24] = 0x11;
    rom[25] = 0xE0;  // background colour cell (r0,c0)
    rom[26] = 0x10;  // r0b.

    let (_cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 5);
    assert_eq!(ui_commands[0], UICommand::SetEnabled(true));
    assert_eq!(ui_commands[1], UICommand::SetChar(5, 32, '!'));
    assert_eq!(ui_commands[2], UICommand::SetFg(20, 50, 85, 0, 0));
    assert_eq!(ui_commands[3], UICommand::SetBg(0, 0, 255, 255, 0));
    assert_eq!(ui_commands[4], UICommand::SetEnabled(false));
}

#[test]
#[timeout(100)]
fn test_syscall() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into kspr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00;  // address 0x00005000.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x00;  // into r0
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0xC0; // ROM byte 0x80 (128).

    rom[12] = 0x08; // Store into
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0x00;
    rom[16] = 0x00; // syscall interrupt handler
    rom[17] = 0x00; // r0.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x24; // into imr
    rom[20] = 0x00;
    rom[21] = 0x01; // syscall only.

    rom[22] = 0x6B; // SYSCALL.

    rom[23] = 0x01; // Pause (fail condition).

    // Interrupt handler.
    rom[128] = 0x0A; // Copy literal
    rom[129] = 0x17; // into r7b
    rom[130] = 0x42; // some number.

    rom[131] = 0x00; // HALT.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x42);
}

#[test]
#[timeout(100)]
fn test_bad_reg_ref() {
    let mut rom = [0; 512];
    // Try a copy with unmatched sizes. Should raise an interrupt.
    rom[0] = 0x0A; // Copy literal
    rom[1] = 0x22; // into kspr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00; // address 0x00005000.

    rom[6] = 0x0A; // Copy literal
    rom[7] = 0x24; // into imr
    rom[8] = 0x00;
    rom[9] = 0x40; // illegal operation interrupt only.

    rom[10] = 0x0B; // Copy register
    rom[11] = 0x01; // into r1
    rom[12] = 0x0B; // from r3h.

    rom[13] = 0x01; // Pause. We should never hit this.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[1], 0x00000000);
}

#[test]
#[timeout(100)]
fn test_pfsr() {
    let mut rom = [0; 512];
    // Set the page directory pointer.
    rom[0] = 0x0A; // Copy literal
    rom[1] = 0x23; // into pdpr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x00;
    rom[5] = 0xC0; // ROM byte 0x80 (128).

    // Write the page table entry.
    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x00;  // into r0
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x50;
    rom[11] = 0x13; // Valid, Present, Execute entry at 0x00005000.

    rom[12] = 0x08; // Store into
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0xB0;
    rom[16] = 0x00; // address 0x0000B000
    rom[17] = 0x00; // r0.

    // Write the user mode instructions to RAM.

    // (Load an execute-only address, which is not allowed).
    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x08; // into r0h
    rom[20] = 0x06; // LOAD instruction
    rom[21] = 0x17; // into r7b.

    rom[22] = 0x08; // Store into
    rom[23] = 0x00;
    rom[24] = 0x00;
    rom[25] = 0x50;
    rom[26] = 0x00; // address 0x00005000
    rom[27] = 0x08; // r0h.

    rom[28] = 0x0A; // Copy literal
    rom[29] = 0x00; // into r0
    rom[30] = 0x00;
    rom[31] = 0x00;
    rom[32] = 0x00;
    rom[33] = 0x04; // virtual address 0x00000004.

    rom[34] = 0x08; // Store into
    rom[35] = 0x00;
    rom[36] = 0x00;
    rom[37] = 0x50;
    rom[38] = 0x02; // address 0x00005002
    rom[39] = 0x00; // r0.

    // (Pause, which should never be hit).
    rom[40] = 0x0A; // Copy literal
    rom[41] = 0x10; // into r0b
    rom[42] = 0x01; // PAUSE instruction.

    rom[43] = 0x08; // Store into
    rom[44] = 0x00;
    rom[45] = 0x00;
    rom[46] = 0x50;
    rom[47] = 0x06; // address 0x00005006.
    rom[48] = 0x10; // r0b.

    // Set the kernel stack pointer.
    rom[49] = 0x0A; // Copy literal
    rom[50] = 0x22; // into kspr
    rom[51] = 0x00;
    rom[52] = 0x00;
    rom[53] = 0xA0;
    rom[54] = 0x00; // address 0x0000A000.

    // Enable page fault interrupts.
    rom[55] = 0x0A; // Copy literal
    rom[56] = 0x24; // into imr
    rom[57] = 0x00;
    rom[58] = 0x10; // page fault interrupt only.

    // Set page fault interrupt handler
    rom[59] = 0x0A; // Copy literal
    rom[60] = 0x00; // into r0
    rom[61] = 0x00;
    rom[62] = 0x00;
    rom[63] = 0x01;
    rom[64] = 0x40; // ROM byte 0x100 (256).

    rom[65] = 0x08; // Store into
    rom[66] = 0x00;
    rom[67] = 0x00;
    rom[68] = 0x00;
    rom[69] = 0x10; // address 0x00000010 (page fault handler).
    rom[70] = 0x00; // r0.

    // Push the user mode address to the stack.
    rom[71] = 0x0A; // Copy literal
    rom[72] = 0x00; // into r0
    rom[73] = 0x00;
    rom[74] = 0x00;
    rom[75] = 0x00;
    rom[76] = 0x00; // virtual address 0x0.

    rom[77] = 0x0E; // Push to the stack
    rom[78] = 0x00; // r0.

    // Enter user mode!
    rom[79] = 0x04;

    // Page directory entry.
    rom[128] = 0x00;
    rom[129] = 0x00;
    rom[130] = 0xB0;
    rom[131] = 0x01; // Valid entry at 0x0000B000.

    // Page fault handler.
    rom[256] = 0x0B; // Copy between registers
    rom[257] = 0x05; // into r5
    rom[258] = 0x25; // from pfsr.

    rom[259] = 0x00; // Halt.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    // Assert the user mode process stored in its stack correctly.
    assert_eq!(internal!(cpu).r[5], internal!(cpu).mmu.page_fault_status_register());
    assert_eq!(internal!(cpu).r[5], crate::mmu::PAGE_FAULT_ILLEGAL_ACCESS);
}

#[test]
#[timeout(200)]
fn test_timer_literal_interval() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into kspr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00;  // address 0x00005000.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x00;  // into r0
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x40;
    rom[11] = 0x00; // address 0x00004000.

    rom[12] = 0x08; // Store into
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0x00;
    rom[16] = 0x07; // timer interrupt handler
    rom[17] = 0x00; // r0.

    // Address 0x4000 is HALT, so we should halt on interrupt.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x24; // into imr
    rom[20] = 0x00;
    rom[21] = 0x80; // timer interrupt only.

    rom[22] = 0x02; // Set timer to
    rom[23] = 0x00;
    rom[24] = 0x00;
    rom[25] = 0x00;
    rom[26] = 0x64; // 100 milliseconds.

    rom[27] = 0x01; // Pause.

    let (_cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    // Simply by halting we confirm that the test was successful.
}

#[test]
#[timeout(200)]
fn test_timer_reg_interval() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into kspr
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00;  // address 0x00005000.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x00;  // into r0
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x40;
    rom[11] = 0x00; // address 0x00004000.

    rom[12] = 0x08; // Store into
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0x00;
    rom[16] = 0x07; // timer interrupt handler
    rom[17] = 0x00; // r0.

    // Address 0x4000 is HALT, so we should halt on interrupt.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x24; // into imr
    rom[20] = 0x00;
    rom[21] = 0x80; // timer interrupt only.

    rom[22] = 0x0A; // Copy literal
    rom[23] = 0x00; // into r0
    rom[24] = 0x00;
    rom[25] = 0x00;
    rom[26] = 0x00;
    rom[27] = 0x64; // 100 milliseconds.

    rom[28] = 0x03; // Set timer to
    rom[29] = 0x00; // interval in r0.

    rom[30] = 0x01; // Pause.

    let (_cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    // Simply by halting we confirm that the test was successful.
}

#[test]
#[timeout(100)]
fn test_blockcopy() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x40;
    rom[5] = 0x00;  // address 0x00004000.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x01;  // into r1
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x80; // ROM byte 0x40 (64).

    rom[12] = 0x13; // Blockcopy literal length, ref source and dest
    rom[13] = 0x00;
    rom[14] = 0x00;
    rom[15] = 0x00;
    rom[16] = 0x40; // 64 bytes
    rom[17] = 0x00; // into address in r0
    rom[18] = 0x01; // from address in r1.

    // DATA
    rom[64] = 0x11;
    rom[65] = 0x22;
    rom[66] = 0x33;
    rom[67] = 0x44;
    rom[68] = 0x55;
    rom[69] = 0x66;
    rom[70] = 0x77;
    rom[71] = 0x88;
    rom[72] = 0x99;
    rom[73] = 0xAA;
    rom[74] = 0xBB;
    rom[75] = 0xCC;
    rom[76] = 0xDD;
    rom[77] = 0xEE;
    rom[78] = 0xFF;
    // Gap of zeroes
    rom[113] = 0x11;
    rom[114] = 0x22;
    rom[115] = 0x33;
    rom[116] = 0x44;
    rom[117] = 0x55;
    rom[118] = 0x66;
    rom[119] = 0x77;
    rom[120] = 0x88;
    rom[121] = 0x99;
    rom[122] = 0xAA;
    rom[123] = 0xBB;
    rom[124] = 0xCC;
    rom[125] = 0xDD;
    rom[126] = 0xEE;
    rom[127] = 0xFF;
    // This last byte should NOT be copied.
    rom[128] = 0xFF;

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4000), Ok(0x11));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4001), Ok(0x22));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4002), Ok(0x33));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4003), Ok(0x44));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4004), Ok(0x55));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4005), Ok(0x66));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4006), Ok(0x77));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4007), Ok(0x88));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4008), Ok(0x99));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4009), Ok(0xAA));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x400A), Ok(0xBB));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x400B), Ok(0xCC));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x400C), Ok(0xDD));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x400D), Ok(0xEE));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x400E), Ok(0xFF));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x400F), Ok(0x00));

    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4030), Ok(0x00));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4031), Ok(0x11));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4032), Ok(0x22));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4033), Ok(0x33));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4034), Ok(0x44));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4035), Ok(0x55));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4036), Ok(0x66));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4037), Ok(0x77));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4038), Ok(0x88));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4039), Ok(0x99));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x403A), Ok(0xAA));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x403B), Ok(0xBB));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x403C), Ok(0xCC));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x403D), Ok(0xDD));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x403E), Ok(0xEE));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x403F), Ok(0xFF));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x4040), Ok(0x00));
}

#[test]
#[timeout(100)]
fn test_blockset() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x00;
    rom[5] = 0x20;  // 32 bytes.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0x42;  // some value.

    rom[9] = 0x1D;  // Blockset ref length, literal dest, ref value
    rom[10] = 0x00; // length in r0
    rom[11] = 0x00;
    rom[12] = 0x00;
    rom[13] = 0x80;
    rom[14] = 0x00; // destination address 0x00008000
    rom[15] = 0x11; // value in r1b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8000), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8001), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8002), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8003), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8004), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8005), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8006), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8007), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8008), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8009), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x800A), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x800B), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x800C), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x800D), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x800E), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x800F), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8010), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8011), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8012), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8013), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8014), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8015), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8016), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8017), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8018), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8019), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x801A), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x801B), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x801C), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x801D), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x801E), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x801F), Ok(0x42));
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x8020), Ok(0x00));
}

#[test]
#[timeout(100)]
fn test_negate() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0x12;
    rom[3] = 0x34;
    rom[4] = 0x56;
    rom[5] = 0x78;  // random number.

    rom[6] = 0x20;  // Negate
    rom[7] = 0x00;  // r0.

    rom[8] = 0x0A;  // Copy literal
    rom[9] = 0x14;  // into r4b
    rom[10] = 0x10; // random number.

    rom[11] = 0x20; // Negate
    rom[12] = 0x14; // r4b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], -0x12345678_i32 as u32);
    assert_eq!(internal!(cpu).r[4], (-0x10_i32 as u8) as u32);
}

#[test]
#[timeout(100)]
fn test_add() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x05;  // some number.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x06;  // some number.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x12;  // into r2b
    rom[8] = 0xFF;  // max number.

    rom[9] = 0x22;  // Add register
    rom[10] = 0x10; // into r0b
    rom[11] = 0x11; // r1b.

    rom[12] = 0x22; // Add register
    rom[13] = 0x11; // into r1b
    rom[14] = 0x11; // r1b.

    rom[15] = 0x21; // Add literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x01; // 1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x0B);
    assert_eq!(internal!(cpu).r[1], 0x0C);
    assert_eq!(internal!(cpu).r[2], 0x00);
}

#[test]
#[timeout(100)]
fn test_flags() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // max number.

    rom[3] = 0x21;  // Add literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x01;  // 1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x00);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO | FLAG_CARRY);

    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x7F;  // max signed number.

    rom[3] = 0x21;  // Add literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x01;  // 1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x80);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_OVERFLOW);

    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // -1.

    rom[3] = 0x22;  // Add register
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x10;  // r0b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0xFE);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_CARRY);

    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x01;  // 1.

    rom[3] = 0x22;  // Add register
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x10;  // r0b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x02);
    assert_eq!(internal!(cpu).flags, 0);
}

#[test]
#[timeout(100)]
fn test_float_add() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x42;  // some number.

    rom[3] = 0x6C;  // Convert register
    rom[4] = 0x18;  // into f0
    rom[5] = 0x00;  // from r0.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x56;  // some number.

    rom[9] = 0x6C;  // Convert register
    rom[10] = 0x19; // into f1
    rom[11] = 0x00; // from r0.

    rom[12] = 0x22; // Add register
    rom[13] = 0x18; // into f0
    rom[14] = 0x19; // from f1.

    rom[15] = 0x6C; // Convert register
    rom[16] = 0x01; // into r1
    rom[17] = 0x18; // from f0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_about_eq!(internal!(cpu).f[0], (0x42 + 0x56) as f32);
    assert_eq!(internal!(cpu).r[1], 0x42 + 0x56);
    assert_eq!(internal!(cpu).flags, 0);
}

#[test]
#[timeout(100)]
fn test_float_convert() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0xFF;
    rom[3] = 0xFF;
    rom[4] = 0xFF;
    rom[5] = 0xFF;  // -1.

    rom[6] = 0x6C;  // Signed convert register
    rom[7] = 0x18;  // into f0
    rom[8] = 0x00;  // from r0.

    rom[9] = 0x6D;  // Unsigned convert register
    rom[10] = 0x19; // into f1
    rom[11] = 0x00; // from r0.

    rom[12] = 0x6D; // Unsigned convert register
    rom[13] = 0x00; // into r0
    rom[14] = 0x18; // from f0.

    rom[15] = 0x6C; // Signed convert register
    rom[16] = 0x01; // into r1
    rom[17] = 0x18; // from f0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_about_eq!(internal!(cpu).f[0], -1.0);
    assert_about_eq!(internal!(cpu).f[1], u32::MAX as f32);
    assert_eq!(internal!(cpu).r[0], 0);
    assert_eq!(internal!(cpu).r[1], u32::MAX);
}

#[test]
#[timeout(100)]
fn test_addcarry() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // max number.

    rom[3] = 0x21;  // Add literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x23;  // Add literal with carry
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x01;  // 1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x02);
}

#[test]
#[timeout(100)]
fn test_sub() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x05;  // some number.

    rom[3] = 0x6C;  // Convert
    rom[4] = 0x18;  // into f0
    rom[5] = 0x00;  // r0.

    rom[6] = 0x25;  // Sub literal
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x07;  // 7.

    rom[9] = 0x26;  // Sub register
    rom[10] = 0x18; // into f0
    rom[11] = 0x18; // f0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0xFE);
    assert_about_eq!(internal!(cpu).f[0], 0.0);
}

#[test]
#[timeout(100)]
fn test_subborrow() {
    let mut rom = [0; 512];
    rom[0] = 0x21;  // Add literal
    rom[1] = 0x11;  // into r1b
    rom[2] = 0x01;  // 1.

    rom[3] = 0x25;  // Subtract literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x05;  // 5.

    rom[6] = 0x28;  // Subtract register with borrow
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x11;  // r1b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0xF9);
}

#[test]
#[timeout(100)]
fn test_mult() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x05;  // 5.

    rom[6] = 0x2A;  // Multiply register
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x11;  // r1b.

    rom[9] = 0x29;  // Multiply literal
    rom[10] = 0x09; // into r1h
    rom[11] = 0x04;
    rom[12] = 0x00; // 1024.

    rom[13] = 0x6C; // Convert
    rom[14] = 0x18; // into f0
    rom[15] = 0x01; // from r1.

    rom[16] = 0x2A; // Multiply register
    rom[17] = 0x18; // into f0
    rom[18] = 0x18; // f0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x0F);
    assert_eq!(internal!(cpu).r[1], 0x1400);
    assert_about_eq!(internal!(cpu).f[0], 26214400.0);
}

#[test]
#[timeout(100)]
fn test_sdiv() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0F;  // 15.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x08;  // 8.

    rom[6] = 0x2C;  // Divide register
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x11;  // by r1b.

    rom[9] = 0x2B;  // Divide literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x04; // 4.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b.
    rom[14] = 0xE2; // -30.

    rom[15] = 0x2B; // Divide literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x05; // 5.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0xE2; // -30.

    rom[21] = 0x2B; // Divide literal
    rom[22] = 0x13; // into r3b
    rom[23] = 0xFB; // -5.

    rom[24] = 0x0A; // Copy literal
    rom[25] = 0x14; // into r4b
    rom[26] = 0xFF; // 255.

    rom[27] = 0x6C; // Convert
    rom[28] = 0x18; // into f0
    rom[29] = 0x04; // r4.

    rom[30] = 0x0A; // Copy literal
    rom[31] = 0x14; // into r4b
    rom[32] = 0x64; // 100.

    rom[33] = 0x6C; // Convert
    rom[34] = 0x19; // into f1
    rom[35] = 0x04; // r4.

    rom[36] = 0x2C; // Divide register
    rom[37] = 0x18; // into f0
    rom[38] = 0x19; // by f1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x01);
    assert_eq!(internal!(cpu).r[1], 0x02);
    assert_eq!(internal!(cpu).r[2], 0xFA);
    assert_eq!(internal!(cpu).r[3], 0x06);
    assert_about_eq!(internal!(cpu).f[0], 2.55);
    assert_about_eq!(internal!(cpu).f[1], 100.0);
}

#[test]
#[timeout(100)]
fn test_div_by_zero() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xC0;  // ROM address 128.

    rom[3] = 0x08;  // Store literal address
    rom[4] = 0x00;
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x14;  // div by zero interrupt handler
    rom[8] = 0x00;  // r0.

    rom[9] = 0x0A;  // Copy literal
    rom[10] = 0x22; // into kspr
    rom[11] = 0x00;
    rom[12] = 0x00;
    rom[13] = 0x50;
    rom[14] = 0x00; // address 0x00005000.

    rom[15] = 0x0A; // Copy literal
    rom[16] = 0x24; // into imr
    rom[17] = 0x00;
    rom[18] = 0x20; // div by zero interrupt only.

    rom[19] = 0x2B; // Divide literal
    rom[20] = 0x10; // into r0b
    rom[21] = 0x00; // 0.

    rom[22] = 0x01; // Pause (fail condition).

    // Interrupt handler.
    rom[128] = 0x21;  // Add literal
    rom[129] = 0x17;  // into r7b
    rom[130] = 0x01;  // 1.

    rom[131] = 0x00;  // HALT.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0xC0);
    assert_eq!(internal!(cpu).r[7], 0x01);
}

#[test]
#[timeout(100)]
fn test_udiv() {
    let mut rom = [0; 512];
    // These use the same values as test_sdiv, but interpret them as unsigned instead.
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0F;  // 15.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x08;  // 8.

    rom[6] = 0x2E;  // Divide register
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x11;  // r1b.

    rom[9] = 0x2D;  // Divide literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x04; // 4.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b.
    rom[14] = 0xE2; // 226.

    rom[15] = 0x2D; // Divide literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x05; // 5.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0xE2; // 226.

    rom[21] = 0x2D; // Divide literal
    rom[22] = 0x13; // into r3b
    rom[23] = 0xFB; // 251.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x01);
    assert_eq!(internal!(cpu).r[1], 0x02);
    assert_eq!(internal!(cpu).r[2], 0x2D);
    assert_eq!(internal!(cpu).r[3], 0x00);
}

#[test]
#[timeout(100)]
fn test_srem() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0F;  // 15.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x08;  // 8.

    rom[6] = 0x30;  // Remainder register
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x11;  // by r1b.

    rom[9] = 0x2F;  // Remainder literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x04; // 4.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b.
    rom[14] = 0xE2; // -30.

    rom[15] = 0x2F; // Remainder literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x04; // 4.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0xE2; // -30.

    rom[21] = 0x2F; // Remainder literal
    rom[22] = 0x13; // into r3b
    rom[23] = 0xFC; // -4.

    rom[24] = 0x0A; // Copy literal
    rom[25] = 0x14; // into r4b
    rom[26] = 0xFF; // 255.

    rom[27] = 0x6C; // Convert
    rom[28] = 0x18; // into f0
    rom[29] = 0x04; // r4.

    rom[30] = 0x0A; // Copy literal
    rom[31] = 0x14; // into r4b
    rom[32] = 0x64; // 100.

    rom[33] = 0x6C; // Convert
    rom[34] = 0x19; // into f1
    rom[35] = 0x04; // r4.

    rom[36] = 0x30; // Remainder register
    rom[37] = 0x18; // into f0
    rom[38] = 0x19; // by f1.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x07);
    assert_eq!(internal!(cpu).r[1], 0x00);
    assert_eq!(internal!(cpu).r[2], 0xFE);
    assert_eq!(internal!(cpu).r[3], 0xFE);
    assert_about_eq!(internal!(cpu).f[0], 55.0);
    assert_about_eq!(internal!(cpu).f[1], 100.0);
}

#[test]
#[timeout(100)]
fn test_urem() {
    let mut rom = [0; 512];
    // These use the same values as test_srem, but interpret them as unsigned instead.
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0F;  // 15.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x08;  // 8.

    rom[6] = 0x32;  // Remainder register
    rom[7] = 0x10;  // into r0b
    rom[8] = 0x11;  // r1b.

    rom[9] = 0x31;  // Remainder literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x04; // 4.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b.
    rom[14] = 0xE2; // 226.

    rom[15] = 0x31; // Remainder literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x04; // 4.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0xE2; // 226.

    rom[21] = 0x31; // Remainder literal
    rom[22] = 0x13; // into r3b
    rom[23] = 0xFC; // 252.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x07);
    assert_eq!(internal!(cpu).r[1], 0x00);
    assert_eq!(internal!(cpu).r[2], 0x02);
    assert_eq!(internal!(cpu).r[3], 0xE2);
}

#[test]
#[timeout(100)]
fn test_not() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x33;  // Logical NOT
    rom[4] = 0x08;  // r0h.

    rom[5] = 0x33;  // Logical NOT
    rom[6] = 0x01;  // r1.

    rom[7] = 0x0B;  // Copy register
    rom[8] = 0x02;  // into r2
    rom[9] = 0x01;  // r1.

    rom[10] = 0x33; // Logical NOT
    rom[11] = 0x12; // r2b.

    rom[12] = 0x33; // Logical NOT
    rom[13] = 0x02; // r2.

    rom[14] = 0x33; // Logical NOT
    rom[15] = 0x03; // r3.

    rom[16] = 0x33; // Logical NOT
    rom[17] = 0x03; // r3.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0xFFFC);
    assert_eq!(internal!(cpu).r[1], 0xFFFFFFFF);
    assert_eq!(internal!(cpu).r[2], 0xFF);
    assert_eq!(internal!(cpu).r[3], 0x0);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO);
}

#[test]
#[timeout(100)]
fn test_and() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x34;  // Logical AND literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x02;  // 2.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0x0F;  // 15.

    rom[9] = 0x34;  // Logical AND literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x10; // 16.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b
    rom[14] = 0xFE; // 254.

    rom[15] = 0x0A; // Copy literal
    rom[16] = 0x13; // into r3b
    rom[17] = 0x81; // 129.

    rom[18] = 0x35; // Logical AND register
    rom[19] = 0x12; // into r2b
    rom[20] = 0x13; // by r3b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x02);
    assert_eq!(internal!(cpu).r[1], 0x0);
    assert_eq!(internal!(cpu).r[2], 0x80);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE);
}

#[test]
#[timeout(100)]
fn test_or() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x36;  // Logical OR literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x02;  // 2.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0x0F;  // 15.

    rom[9] = 0x36;  // Logical OR literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x10; // 16.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b
    rom[14] = 0xFE; // 254.

    rom[15] = 0x0A; // Copy literal
    rom[16] = 0x13; // into r3b
    rom[17] = 0x81; // 129.

    rom[18] = 0x37; // Logical OR register
    rom[19] = 0x12; // into r2b
    rom[20] = 0x13; // by r3b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x03);
    assert_eq!(internal!(cpu).r[1], 0x1F);
    assert_eq!(internal!(cpu).r[2], 0xFF);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE);
}

#[test]
#[timeout(100)]
fn test_xor() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x38;  // Logical XOR literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x02;  // 2.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0x0F;  // 15.

    rom[9] = 0x38;  // Logical XOR literal
    rom[10] = 0x11; // into r1b
    rom[11] = 0x10; // 16.

    rom[12] = 0x0A; // Copy literal
    rom[13] = 0x12; // into r2b
    rom[14] = 0xFE; // 254.

    rom[15] = 0x0A; // Copy literal
    rom[16] = 0x13; // into r3b
    rom[17] = 0x81; // 129.

    rom[18] = 0x39; // Logical XOR register
    rom[19] = 0x12; // into r2b
    rom[20] = 0x13; // by r3b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x01);
    assert_eq!(internal!(cpu).r[1], 0x1F);
    assert_eq!(internal!(cpu).r[2], 0x7F);
    assert_eq!(internal!(cpu).flags, 0);
}

#[test]
#[timeout(100)]
fn test_lshift() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x3A;  // Left shift literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x02;  // 2.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0xC0;  // 192.

    rom[9] = 0x0A;  // Copy literal
    rom[10] = 0x12; // into r2b
    rom[11] = 0x01; // 1.

    rom[12] = 0x3B; // Left shift register
    rom[13] = 0x11; // into r1b
    rom[14] = 0x12; // r2b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x0C);
    assert_eq!(internal!(cpu).r[1], 0x80);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_CARRY);
}

#[test]
#[timeout(100)]
fn test_srshift() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x3C;  // Signed right shift literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0xC4;  // -60.

    rom[9] = 0x0A;  // Copy literal
    rom[10] = 0x12; // into r2b
    rom[11] = 0x03; // 3.

    rom[12] = 0x3D; // Signed right shift register
    rom[13] = 0x11; // into r1b
    rom[14] = 0x12; // r2b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x01);
    assert_eq!(internal!(cpu).r[1], 0xF8);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_CARRY);
}

#[test]
#[timeout(100)]
fn test_urshift() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x03;  // 3.

    rom[3] = 0x3E;  // Unsigned right shift literal
    rom[4] = 0x10;  // into r0b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x11;  // into r1b
    rom[8] = 0xC4;  // 196.

    rom[9] = 0x0A;  // Copy literal
    rom[10] = 0x12; // into r2b
    rom[11] = 0x03; // 3.

    rom[12] = 0x3F; // Unsigned right shift register
    rom[13] = 0x11; // into r1b
    rom[14] = 0x12; // r2b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x01);
    assert_eq!(internal!(cpu).r[1], 0x18);
    assert_eq!(internal!(cpu).flags, FLAG_CARRY);
}

#[test]
#[timeout(100)]
fn test_big_shift() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x00;  // into r0
    rom[2] = 0xFF;
    rom[3] = 0xFF;
    rom[4] = 0xFF;
    rom[5] = 0xFF;  // max value.

    rom[6] = 0x0B;  // Copy register
    rom[7] = 0x01;  // into r1
    rom[8] = 0x00;  // r0.

    rom[9] = 0x0B;  // Copy register
    rom[10] = 0x02; // into r2
    rom[11] = 0x00; // r0.

    rom[12] = 0x3A; // Left shift literal
    rom[13] = 0x00; // into r0
    rom[14] = 0x21; // by 33.

    rom[15] = 0x3C; // Signed right shift literal
    rom[16] = 0x01; // into r1
    rom[17] = 0x40; // by 64.

    rom[18] = 0x3E; // Unsigned right shift literal
    rom[19] = 0x02; // into r2
    rom[20] = 0xFF; // by 255.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0);
    assert_eq!(internal!(cpu).r[1], u32::MAX);
    assert_eq!(internal!(cpu).r[2], 0);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO | FLAG_CARRY);
}

#[test]
#[timeout(100)]
fn test_lrot() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x80;  // 128.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x12;  // into r2b
    rom[8] = 0xFF;  // 255.

    rom[9] = 0x41;  // Left rotate register
    rom[10] = 0x10; // into r0b
    rom[11] = 0x11; // by r1b.

    rom[12] = 0x40; // Left rotate literal
    rom[13] = 0x11; // into r1b
    rom[14] = 0x05; // by 5.

    rom[15] = 0x40; // Left rotate literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x01; // by 1.

    rom[18] = 0x40; // Left rotate literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0x01; // by 1.

    rom[21] = 0x40; // Left rotate literal
    rom[22] = 0x00; // into r0
    rom[23] = 0x20; // by 32.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x01);
    assert_eq!(internal!(cpu).r[1], 0x20);
    assert_eq!(internal!(cpu).r[2], 0xFF);
    assert_eq!(internal!(cpu).r[3], 0x00);
    assert_eq!(internal!(cpu).flags, 0);
}

#[test]
#[timeout(100)]
fn test_rrot() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x80;  // 128.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x12;  // into r2b
    rom[8] = 0xFF;  // 255.

    rom[9] = 0x43;  // Right rotate register
    rom[10] = 0x10; // into r0b
    rom[11] = 0x11; // by r1b.

    rom[12] = 0x42; // Right rotate literal
    rom[13] = 0x11; // into r1b
    rom[14] = 0x05; // by 5.

    rom[15] = 0x42; // Right rotate literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x01; // by 1.

    rom[18] = 0x42; // Right rotate literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0x01; // by 1.

    rom[21] = 0x42; // Right rotate literal
    rom[22] = 0x08; // into r0h
    rom[23] = 0x10; // by 16.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x40);
    assert_eq!(internal!(cpu).r[1], 0x08);
    assert_eq!(internal!(cpu).r[2], 0xFF);
    assert_eq!(internal!(cpu).r[3], 0x00);
    assert_eq!(internal!(cpu).flags, 0);
}

#[test]
#[timeout(100)]
fn test_rotcarry() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x80;  // 128.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x12;  // into r2b
    rom[8] = 0xFF;  // 255.

    rom[9] = 0x45;  // Left rotate carry register
    rom[10] = 0x10; // into r0b
    rom[11] = 0x11; // by r1b.

    rom[12] = 0x44; // Left rotate carry literal
    rom[13] = 0x11; // into r1b
    rom[14] = 0x05; // by 5.

    rom[15] = 0x46; // Right rotate carry literal
    rom[16] = 0x12; // into r2b
    rom[17] = 0x01; // by 1.

    rom[18] = 0x0A; // Copy literal
    rom[19] = 0x13; // into r3b
    rom[20] = 0x01; // 1.

    rom[21] = 0x47; // Right rotate carry register
    rom[22] = 0x13; // into r3b
    rom[23] = 0x13; // by r3b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x00);
    assert_eq!(internal!(cpu).r[1], 0x30);
    assert_eq!(internal!(cpu).r[2], 0x7F);
    assert_eq!(internal!(cpu).r[3], 0x80);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_CARRY);
}

#[test]
#[timeout(100)]
fn test_jump() {
    let mut rom = [0; 512];
    rom[0] = 0x48;   // Jump to literal address
    rom[1] = 0x00;
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0xC0;   // 0x000000C0 (ROM byte 128).

    rom[5] = 0x01;   // Pause (fail condition).

    rom[128] = 0x0A; // Copy literal
    rom[129] = 0x00; // into r0
    rom[130] = 0x00;
    rom[131] = 0x00;
    rom[132] = 0x40;
    rom[133] = 0x00; // address start of RAM.

    rom[134] = 0x0A; // Copy literal
    rom[135] = 0x01; // into r1
    rom[136] = 0x0A; // opcode: copy literal
    rom[137] = 0x17; // operand: into r7b
    rom[138] = 0x42; // operand: some number
    rom[139] = 0x00; // opcode: halt.

    rom[140] = 0x09; // Store at
    rom[141] = 0x00; // address in r0
    rom[142] = 0x01; // contents of r1.

    rom[143] = 0x49; // Jump to register address
    rom[144] = 0x00; // r0.

    rom[145] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[0], 0x4000);
    assert_eq!(internal!(cpu).r[1], 0x0A174200);
    assert_eq!(internal!(cpu).r[7], 0x42);
}

#[test]
#[timeout(100)]
fn test_compare() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x40;  // 64.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b with
    rom[5] = 0x40;  // 64.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO);


    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x40;  // 64.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b with
    rom[5] = 0x41;  // 65.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE | FLAG_CARRY);


    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x40;  // 64.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b with
    rom[5] = 0x3F;  // 63.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, 0);


    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // 255 (-1).

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0xFA;  // 250 (-6).

    rom[6] = 0x4B;  // Compare registers
    rom[7] = 0x10;  // r0b with
    rom[8] = 0x11;  // r1b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, 0);


    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x80;  // 128 (-128).

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x01;  // 1.

    rom[6] = 0x4B;  // Compare registers
    rom[7] = 0x10;  // r0b with
    rom[8] = 0x11;  // r1b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_OVERFLOW);


    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x00;  // 0.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0x81;  // 129 (-127).

    rom[6] = 0x4B;  // Compare registers
    rom[7] = 0x10;  // r0b with
    rom[8] = 0x11;  // r1b.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_CARRY);
}

#[test]
#[timeout(100)]
fn test_blockcmp() {
    let mut rom = [0; 512];
    rom[0] = 0x4C;  // Block compare literal literal literal
    rom[1] = 0x00;
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x04;  // 4 bytes
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x00;
    rom[8] = 0xC0;  // ROM byte 128
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x00;
    rom[12] = 0xC4; // with ROM byte 132.

    rom[128] = 0x12;
    rom[129] = 0x34;
    rom[130] = 0x56;
    rom[131] = 0x78;

    rom[132] = 0x12;
    rom[133] = 0x00;
    rom[134] = 0x56;
    rom[135] = 0x78;

    rom[136] = 0x12;
    rom[137] = 0x34;
    rom[138] = 0x56;
    rom[139] = 0x99;

    rom[140] = 0x12;
    rom[141] = 0x34;
    rom[142] = 0x56;
    rom[143] = 0x78;

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, 0);


    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xC8;  // ROM byte 136.

    rom[3] = 0x4D;  // Block compare literal literal ref
    rom[4] = 0x00;
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x04;  // 4 bytes
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0xC0; // ROM byte 128
    rom[12] = 0x00; // with r0.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE);


    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xCC;  // ROM byte 140.

    rom[3] = 0x4E;  // Block compare literal ref literal
    rom[4] = 0x00;
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x04;  // 4 bytes
    rom[8] = 0x00;  // r0
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x00;
    rom[12] = 0xC0; // with ROM byte 128.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO);


    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x04;  // 4.

    rom[3] = 0x50;  // Block compare ref literal literal
    rom[4] = 0x00;  // length r0
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x00;
    rom[8] = 0xC8;  // ROM byte 136.
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x00;
    rom[12] = 0xC4; // with ROM byte 132.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, 0);


    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x04;  // 4.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0xCC;  // ROM byte 140.

    rom[6] = 0x52;  // Block compare ref ref literal
    rom[7] = 0x00;  // length r0
    rom[8] = 0x01;  // r1
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x00;
    rom[12] = 0xC4; // ROM byte 132.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, 0);


    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x04;  // 4.

    rom[3] = 0x0A;  // Copy literal
    rom[4] = 0x11;  // into r1b
    rom[5] = 0xCC;  // ROM byte 140.

    rom[6] = 0x0A;  // Copy literal
    rom[7] = 0x12;  // into r2b
    rom[8] = 0xC8;  // ROM byte 136.

    rom[9] = 0x53;  // Block compare ref ref ref
    rom[10] = 0x00; // length r0
    rom[11] = 0x01; // r1
    rom[12] = 0x02; // with r2.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_NEGATIVE);


    rom[0] = 0x4C;  // Block compare literal literal literal
    rom[1] = 0x00;
    rom[2] = 0x00;
    rom[3] = 0x02;
    rom[4] = 0x00;  // 512 bytes
    rom[5] = 0x00;
    rom[6] = 0x00;
    rom[7] = 0x00;
    rom[8] = 0x40;  // start of ROM
    rom[9] = 0x00;
    rom[10] = 0x00;
    rom[11] = 0x00;
    rom[12] = 0x40; // with itself.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO);
}

#[test]
#[timeout(100)]
fn test_jequal() {
    let mut rom = [0; 512];
    rom[0] = 0x54;  // Jump if equal to literal address
    rom[1] = 0x00;
    rom[2] = 0x00;
    rom[3] = 0x01;
    rom[4] = 0x40;  // ROM byte 256.

    rom[5] = 0x0A;  // Copy literal
    rom[6] = 0x10;  // into r0b
    rom[7] = 0x80;  // ROM byte 64.

    rom[8] = 0x4A;  // Compare literal
    rom[9] = 0x11;  // r1b
    rom[10] = 0x00; // with 0.

    rom[11] = 0x55; // Jump if equal to register address
    rom[12] = 0x00; // r0.

    rom[13] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_jnotequal() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0A;  // 10.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x0A;  // with 10.

    rom[6] = 0x56;  // Jump if not equal to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x01;
    rom[10] = 0x40; // ROM byte 256.

    rom[11] = 0x4A; // Compare literal
    rom[12] = 0x10; // r0b.
    rom[13] = 0x0B; // with 11.

    rom[14] = 0x56; // Jump if not equal to literal address
    rom[15] = 0x00;
    rom[16] = 0x00;
    rom[17] = 0x00;
    rom[18] = 0x80; // ROM byte 64.

    rom[19] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_sjgreater() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0A;  // 10.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x0A;  // with 10.

    rom[6] = 0x58;  // Jump if greater to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x01;
    rom[10] = 0x40; // ROM byte 256.

    rom[11] = 0x4A; // Compare literal
    rom[12] = 0x10; // r0b.
    rom[13] = 0x09; // with 9.

    rom[14] = 0x58; // Jump if greater to literal address
    rom[15] = 0x00;
    rom[16] = 0x00;
    rom[17] = 0x00;
    rom[18] = 0x80; // ROM byte 64.

    rom[19] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_sjgreatereq() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // -1.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x0A;  // with 10.

    rom[6] = 0x5A;  // Jump if greater or equal to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x01;
    rom[10] = 0x40; // ROM byte 256.

    rom[11] = 0x4A; // Compare literal
    rom[12] = 0x10; // r0b.
    rom[13] = 0xFF; // with -1.

    rom[14] = 0x5A; // Jump if greater or equal to literal address
    rom[15] = 0x00;
    rom[16] = 0x00;
    rom[17] = 0x00;
    rom[18] = 0x80; // ROM byte 64.

    rom[19] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_ujgreater() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // 255.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x0A;  // with 10.

    rom[6] = 0x5C;  // Jump if unsigned greater to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x80; // ROM byte 64.

    rom[11] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_ujgreatereq() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // 255.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x81;  // with 129.

    rom[6] = 0x5C;  // Jump if unsigned greater or equal to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x80; // ROM byte 64.

    rom[11] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_sjlesser() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0xFF;  // -1.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0xFE;  // with -2.

    rom[6] = 0x60;  // Jump if lesser to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x01;
    rom[10] = 0x40; // ROM byte 256.

    rom[11] = 0x4A; // Compare literal
    rom[12] = 0x10; // r0b.
    rom[13] = 0x0A; // with 10.

    rom[14] = 0x60; // Jump if lesser to literal address
    rom[15] = 0x00;
    rom[16] = 0x00;
    rom[17] = 0x00;
    rom[18] = 0x80; // ROM byte 64.

    rom[19] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_sjlessereq() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x0A;  // 10.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x00;  // with 0.

    rom[6] = 0x62;  // Jump if lesser or equal to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x01;
    rom[10] = 0x40; // ROM byte 256.

    rom[11] = 0x4A; // Compare literal
    rom[12] = 0x10; // r0b.
    rom[13] = 0x0A; // with 10.

    rom[14] = 0x62; // Jump if lesser or equal to literal address
    rom[15] = 0x00;
    rom[16] = 0x00;
    rom[17] = 0x00;
    rom[18] = 0x80; // ROM byte 64.

    rom[19] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_ujlesser() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x42;  // 66.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x80;  // with 128.

    rom[6] = 0x64;  // Jump if unsigned lesser to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x80; // ROM byte 64.

    rom[11] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_ujlessereq() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x10;  // into r0b
    rom[2] = 0x01;  // 1.

    rom[3] = 0x4A;  // Compare literal
    rom[4] = 0x10;  // r0b
    rom[5] = 0x01;  // with 1.

    rom[6] = 0x66;  // Jump if unsigned lesser or equal to literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x80; // ROM byte 64.

    rom[11] = 0x01; // Pause (fail condition).

    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x17; // into r7b
    rom[66] = 0x99; // some number.

    rom[67] = 0x00; // HALT.

    rom[256] = 0x01; // Pause (fail condition).

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x99);
}

#[test]
#[timeout(100)]
fn test_call_return() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into KSPR
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00;  // address 0x00005000.

    rom[6] = 0x4A;  // Compare literal
    rom[7] = 0x10;  // r0b
    rom[8] = 0x00;  // with 0.

    rom[9] = 0x68;  // Call literal address
    rom[10] = 0x00;
    rom[11] = 0x00;
    rom[12] = 0x00;
    rom[13] = 0x80; // ROM byte 64.

    rom[14] = 0x0A; // Copy literal
    rom[15] = 0x17; // into r7b
    rom[16] = 0x56; // some number.

    rom[17] = 0x0A; // Copy literal
    rom[18] = 0x11; // into r1b
    rom[19] = 0xC0; // ROM byte 128.

    rom[20] = 0x69; // Call register address
    rom[21] = 0x01; // r1.

    rom[22] = 0x00; // HALT.

    // Subroutine 1
    rom[64] = 0x0A; // Copy literal
    rom[65] = 0x16; // into r6b
    rom[66] = 0xCA; // some number.

    rom[67] = 0x4A; // Compare literal
    rom[68] = 0x10; // r0b
    rom[69] = 0x01; // with 1.

    rom[70] = 0x6A; // RETURN.

    // Subroutine 2
    rom[128] = 0x0A; // Copy literal
    rom[129] = 0x15; // into r5b
    rom[130] = 0x29; // some number.

    rom[131] = 0x6A; // RETURN.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[5], 0x29);
    assert_eq!(internal!(cpu).r[6], 0xCA);
    assert_eq!(internal!(cpu).r[7], 0x56);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO);
    // The return address of the last subroutine should still be on the stack.
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x00004FFF), Ok(0x56));
}

#[test]
#[timeout(100)]
fn test_call_modify_return() {
    let mut rom = [0; 512];
    rom[0] = 0x0A;  // Copy literal
    rom[1] = 0x22;  // into KSPR
    rom[2] = 0x00;
    rom[3] = 0x00;
    rom[4] = 0x50;
    rom[5] = 0x00;  // address 0x00005000.

    rom[6] = 0x68;  // Call literal address
    rom[7] = 0x00;
    rom[8] = 0x00;
    rom[9] = 0x00;
    rom[10] = 0x80; // ROM byte 64.

    rom[11] = 0x01; // Pause (fail condition).

    // Subroutine 1
    // Erase the call metadata off the stack.
    rom[64] = 0x21; // Add literal
    rom[65] = 0x22; // into KSPR
    rom[66] = 0x00;
    rom[67] = 0x00;
    rom[68] = 0x00;
    rom[69] = 0x06; // 6 bytes.

    // Replace it with our own.
    rom[70] = 0x0A; // Copy literal
    rom[71] = 0x10; // into r0b
    rom[72] = 0xC0; // ROM byte 128.

    rom[73] = 0x0E; // Push
    rom[74] = 0x00; // r0.

    rom[75] = 0x0A; // Copy literal
    rom[76] = 0x10; // into r0b
    rom[77] = 0x03; // ZERO and NEGATIVE flags (normally impossible to co-occur).

    rom[78] = 0x0E; // Push
    rom[79] = 0x08; // r0h.

    rom[80] = 0x6A; // RETURN.

    // Return point
    rom[128] = 0x0A; // Copy literal
    rom[129] = 0x17; // into r7b
    rom[130] = 0x33; // some number.

    rom[131] = 0x00; // HALT.

    let (cpu, ui_commands) = run(rom, None);
    assert_eq!(ui_commands.len(), 2);
    assert_eq!(internal!(cpu).r[7], 0x33);
    assert_eq!(internal!(cpu).flags, FLAG_ZERO | FLAG_NEGATIVE);
    // The return address of the last subroutine should still be on the stack.
    assert_eq!(internal!(cpu).mmu.load_physical_8(0x00004FFF), Ok(0xC0));
}
