# Instruction Set
### Version 2.0.0-alpha

## Registers Available
| Register Reference | Short name | Full Name                       | Description                                      |
| ------------------:| ---------- | ------------------------------- | ------------------------------------------------ |
|                0-7 | r0-r7      | Integer Registers 0-7 (full)    | 32-bit integer general purpose registers.        |
|               8-15 | r0h-r7h    | Integer Registers 0-7 (half)    | Lower 16 bits of r0-r7.                          |
|              16-23 | r0b-r7b    | Integer Registers 0-7 (byte)    | Lowest 8 bits of r0-r7.                          |
|              24-31 | f0-f7      | Float Registers 0-7             | 32-bit floating-point general purpose registers. |
|                 32 | FLAGS      | Flags Register                  | Holds the flags as described below. 16 bits.     |
|                 33 | USPR       | User Stack Pointer Register     | Points to the current top of the user stack.     |
|                 34 | KSPR       | Kernel Stack Pointer Register   | Points to the current top of the kernel stack.   |
|                 35 | PDPR       | Page Directory Pointer Register | Points to the current page directory.            |
|                 36 | IMR        | Interrupt Mask Register         | Enables/disables specific interrupts. 16 bits.   |

KSPR, PDPR, and IMR are privileged registers; they can only be accessed in kernel mode.

To move values between integer and floating-point registers, the COPY instruction should be used. This automatically performs the required conversions.

The r* registers can all be accessed in multiple ways. Appending an 'h' accesses only the lower half (16 bits); appending a 'b' accesses only the lowest byte. Operations on this subset will consider the register to be of that size, e.g. `COPY 255 r0` then `ADD 1 r0b` will overflow to zero.
```
********************************
*       *       *       *      *
********************************
|               |       |      |
|               |       |______|
|               |          r0b |
|               |______________|
|                      r0h     |
|______________________________|
               r0
```

## Flags
There are several flags set by arithmetic or bitwise operations; these may be inspected by subsequent operations.

| Flag | Meaning                                                                              |
|:----:| ------------------------------------------------------------------------------------ |
|   Z  | The last operation resulted in zero.                                                 |
|   N  | The last operation resulted in a negative number.                                    |
|   C  | The last operation either carried or borrowed a bit beyond the size of the register. |
|   O  | The last operation resulted in a value too large for the register.                   |

The register is 16 bits wide, but there are only 15 spaces for flags. Bit 15 is used during interrupt handling; it will always be zero when read, and writing it has no effect. The reserved bits should never be manually set to anything other than zero.

```
______________________________________________
|14|13|12|11|10|9 |8 |7 |6 |5 |4 |3 |2 |1 |0 |
|            RESERVED            |O |C |N |Z |
______________________________________________
```

## Interrupts
There are eight different interrupts, represented by the integers 0-7. When an interrupt is raised, it will be latched by the CPU. Between instruction cycles, the CPU will check for latched interrupts and service them. If there are multiple interrupts waiting, they will be prioritised in descending order. If an interrupt is disabled, it will not be serviced but will remain latched until it is enabled.

An interrupt is enabled if and only if the IMR bit corresponding to its number is set to 1.

Servicing an interrupt causes the following to happen as a single atomic operation:
1. If in user mode, the processor switches into kernel mode.
2. The FLAGS are pushed onto the stack. Bit 15 will be 0 if the processor was in user mode, 1 if the processor was in kernel mode.
3. The address of the next instruction is pushed onto the stack. Note that if the processor was in user mode, this will still be a virtual address.
4. The current IMR is pushed onto the stack.
5. The IMR is set to 0, disabling all interrupts.
6. The processor jumps to the address held in physical memory address (interrupt number * 4).

Note that no other state is saved, so if the interrupt handler wishes to preserve register values it should push and pop them itself. The handler can return by executing IRETURN, although this is not mandatory. It is also possible to modify the values on the stack before executing IRETURN to change what will happen.

Executing IRETURN causes the following to happen as a single atomic operation:
1. The IMR is popped off the stack.
2. The next instruction to execute is popped off the stack.
3. The FLAGS are popped off the stack.
4. If bit 15 of the flags was 0, the processor will enter user mode.

##### Interrupt definitions
| Number | Name              | Cause                                                       |
| ------:| ----------------- | ----------------------------------------------------------- |
|      0 | Syscall           | The `SYSCALL` instruction.                                  |
|      1 | Keyboard          | A key being pressed.                                        |
|      2 | Disk A            | Disk A completes an operation.                              |
|      3 | Disk B            | Disk B completes an operation.                              |
|      4 | Page Fault        | Raised by the MMU - see memory management docs for details. |
|      5 | Divide by 0       | Integer division by zero was attempted.                     |
|      6 | Illegal Operation | An illegal operation was attempted, e.g. calling a privileged instruction in user mode, or trying to write to a read-only address. |
|      7 | Timer             | Raised as described in the `TIMER` instruction.             |

Example interrupt servicing (assume physical address 0 holds 0x00008420):
```
Mode: user            Next instruction: 0x00001234                                     Mode: kernel          Next instruction: 0x00008420
Flags: 0x0001         IMR: 0x007F                     ---> Interrupt 0 arrives --->    Flags: 0x0001         IMR: 0x0000
UStack: 0xDEADBEEF    KStack: 0xFAFFCAFE                                               UStack: 0xDEADBEEF    KStack: 0x007F
                                                                                                                     0x00001234
                                                                                                                     0x0001
                                                                                                                     0xFAFFCAFE
```

Example return from interrupt at end of handler:
```
Mode: kernel          Next instruction: 0x00008500                                 Mode: user            Next instruction: 0x00001234
Flags: 0x0000         IMR: 0x0000                     ---> Execute IRETURN --->    Flags: 0x0001         IMR: 0x007F
UStack: 0xDEADBEEF    KStack: 0x007F                                               UStack: 0xDEADBEEF    KStack: 0xFAFFCAFE
                              0x00001234
                              0x0001
```

## Instructions

##### Operand Type Key
| Description | Meaning                                                       |
| ----------- | ------------------------------------------------------------- |
| Address     | A literal address or a register reference.                    |
| Value       | A literal value or a register reference (integer or float).   |
| Integer     | A literal integer or a register reference (not float).        |
| Register    | A register reference (integer or float).                      |

##### Privileged instructions
| Description              | Instruction  | Operand 1                |
| ------------------------ | ------------ | ------------------------ |
| Halt                     | HALT         |                          |
| Pause                    | PAUSE        |                          |
| Set timer                | TIMER        | Num milliseconds integer |
| Enter user mode          | USERMODE     |                          |
| Return from interrupt    | IRETURN      |                          |

`HALT`: Immediately halt the processor. No further instructions will be executed under any circumstances, and the machine is safe to power off.

`PAUSE`: Temporarily halt the processor. Any received interrupt will resume execution at the following instruction after the interrupt is handled. If all interrupts are disabled, this instruction is semantically equivalent to HALT.

`TIMER`: Set the interrupt timer. It will send a timer interrupt after at least the given number of milliseconds, repeating indefinitely with the same period. A value of zero will disable the timer.
 
`USERMODE`: Pop the target virtual address off the stack, enter user mode, and jump to it.
 
`IRETURN`: See the interrupt section.

##### Data Movement
| Description       | Instruction | Operand 1            | Operand 2            | Operand 3      |
| ----------------- | ----------- | -------------------- | -------------------- | -------------- |
| Load              | LOAD        | Source address       | Destination register |                |
| Store             | STORE       | Source register      | Destination address  |                |
| Copy              | COPY        | Source value         | Destination register |                |
| Atomic Swap       | SWAP        | Source register      | Source address       |                |
| Push              | PUSH        | Source value         |                      |                |
| Pop               | POP         | Destination register |                      |                |
| Block Memory Copy | BLOCKCOPY   | Source address       | Destination address  | Length integer |

`STORE`: Note that storing a literal is not allowed. This is because there would be no way to determine whether the literal is 8, 16, or 32 bits in length.

`COPY`: Either load a register with a constant value, or copy one register into another. This can copy between integer and floating-point registers, and automatically converts the values into the destination representation. Note that only a full 32-bit integer register can be copied into a floating-point register, and converting a float to an integer will truncate towards zero.

`PUSH`: Decrement the stack pointer by the appropriate amount and then write the given value to the stack.

`POP`: Read the top value from the stack, then increment the stack pointer by the appropriate amount.

PUSH and POP will use KSPR if in kernel mode, and USPR if in user mode.

`BLOCKCOPY`: Copy a contiguous block of memory of the given length from the source to the destination address.

##### Arithmetic
| Description          | Instruction | Operand 1                        | Operand 2                         |
| -------------------- | ----------- | -------------------------------- | --------------------------------- |
| Negate               | NEGATE      | Operand and destination register |                                   |
| Add                  | ADD         | Operand value                    | Operand and destination register  |
| Add with carry       | ADDCARRY    | Operand value                    | Operand and destination register  |
| Subtract             | SUB         | Subtrahend value                 | Minuend and destination register  |
| Subtract with borrow | SUBBORROW   | Subtrahend value                 | Minuend and destination register  |
| Multiply             | MULT        | Operand value                    | Operand and destination register  |
| Signed division      | SDIV        | Divisor value                    | Dividend and destination register |
| Unsigned division    | UDIV        | Divisor value                    | Dividend and destination register |
| Remainder            | REM         | Divisor value                    | Dividend and destination register |

`ADDCARRY`: Adds the first operand plus the `C` flag to the destination register. Not applicable to floats.

`SUBBORROW`: Subtracts the first operand plus the `C` flag from the destination register. Not applicable to floats.

`SDIV`: Will give incorrect results for unsigned integers. For integer division, truncates towards zero.

`UDIV`: Will give incorrect results for signed integers. Truncates towards negative infinity. Not applicable to floats.

The operand types must match, i.e. `ADD r0 f0` is illegal, but `COPY r0 f1` followed by `ADD f1 f0` is fine.

##### Bitwise operations
| Description             | Instruction | Operand 1                        | Operand 2                        |
| ----------------------- | ----------- | -------------------------------- | -------------------------------- |
| Not                     | NOT         | Operand and destination register |                                  |
| And                     | AND         | Operand integer                  | Operand and destination register |
| Or                      | OR          | Operand integer                  | Operand and destination register |
| Xor                     | XOR         | Operand integer                  | Operand and destination register |
| Left Shift              | LSHIFT      | Num places integer               | Operand and destination register |
| Logical Right Shift     | RSHIFTL     | Num places integer               | Operand and destination register |
| Arithmetic Right Shift  | RSHIFTA     | Num places integer               | Operand and destination register |
| Left Rotate             | LROT        | Num places integer               | Operand and destination register |
| Right Rotate            | RROT        | Num places integer               | Operand and destination register |
| Left Rotate with carry  | LROTCARRY   | Num places integer               | Operand and destination register |
| Right Rotate with carry | RROTCARRY   | Num places integer               | Operand and destination register |

None of these operations are applicable to floats.

`RSHIFTL`: Logical or Unsigned shift; left-most bits will be filled with zeroes.

`RSHIFTA`: Arithmetic or Signed shift; left-most bits will be filled with the sign bit.

`LROTCARRY`: Includes the `C` flag in the rotation, as if it was to the left of the left-most bit.

`RROTCARRY`: Includes the `C` flag in the rotation, as if it was to the right of the right-most bit.

##### Flow Control
| Description              | Instruction  | Operand 1        | Operand 2      |
| ------------------------ | ------------ | ---------------- | -------------- |
| Jump                     | JUMP         | Target address   |                |
| Compare                  | COMPARE      | Subtrahend value | Minuend value  |
| Jump if equal            | JEQUAL       | Target address   |                |
| Jump if not equal        | JNOTEQUAL    | Target address   |                |
| Jump if zero (1)         | JZERO        | Target address   |                |
| Jump if not zero (2)     | JNOTZERO     | Target address   |                |
| Jump if greater          | JGREATER     | Target address   |                |
| Jump if greater or equal | JGREATEREQ   | Target address   |                |
| Jump if above            | JABOVE       | Target address   |                |
| Jump if above or equal   | JABOVEEQ     | Target address   |                |
| Jump if lesser           | JLESSER      | Target address   |                |
| Jump if lesser or equal  | JLESSEREQ    | Target address   |                |
| Jump if lower            | JLOWER       | Target address   |                |
| Jump if lower or equal   | JLOWEREQ     | Target address   |                |
| Jump if overflow         | JOVERFLOW    | Target address   |                |
| Jump if not overflow     | JNOTOVERFLOW | Target address   |                |
| Call                     | CALL         | Target address   |                |
| Return                   | RETURN       |                  |                |
| Syscall                  | SYSCALL      |                  |                |

```
(1): JZERO is an alias for JEQUAL.
(2): JNOTZERO is an alias for JNOTEQUAL.
```

`COMPARE`: Subtracts the first operand from the second and discards the result, setting flags as appropriate. Note that comparing two literals is not allowed, as there would be no way of determining whether the operation is 8, 16, or 32 bits in length.

`GREATER`/`LESSER`: Signed comparison.

`ABOVE`/`BELOW`: Unsigned comparison.

`CALL`: Pushes the address of the next instruction followed by the current flags onto the stack and jumps to the given address.

`RETURN`: Restores the flags from the stack, then pops the return address and jumps to it.

`SYSCALL`: Raises a syscall interrupt. 

## Opcodes
Opcodes have a fixed length of one byte. All operands have length 4 bytes. Each instruction will automatically fetch the appropriate number of operands. 

If an unmapped opcode is encountered, no operation will take place and an illegal operation interrupt will be raised.

| Opcode | Instruction  | Variant         |     | Opcode | Instruction | Variant                                           |
| ------:| ------------ | --------------- | --- | ------:| ----------- | ------------------------------------------------- |
|   0x00 | HALT         |                 |     |   0x80 | LOAD        | literal address                                   |
|   0x01 | PAUSE        |                 |     |   0x81 | LOAD        | register ref                                      |
|   0x02 | USERMODE     |                 |     |   0x82 | STORE       | literal address                                   |
|   0x03 | SYSCALL      |                 |     |   0x83 | STORE       | register ref                                      |
|   0x04 | RETURN       |                 |     |   0x84 |             |                                                   |
|   0x05 | IRETURN      |                 |     |   0x85 |             |                                                   |
|   0x06 |              |                 |     |   0x86 | COPY        | literal value                                     |
|   0x07 |              |                 |     |   0x87 | COPY        | register ref                                      |
|   0x08 |              |                 |     |   0x88 | SWAP        | literal address                                   |
|   0x09 |              |                 |     |   0x89 | SWAP        | register ref                                      |
|   0x0A |              |                 |     |   0x8A | ADD         | literal value                                     |
|   0x0B |              |                 |     |   0x8B | ADD         | register ref                                      |
|   0x0C |              |                 |     |   0x8C | ADDCARRY    | literal value                                     |
|   0x0D |              |                 |     |   0x8D | ADDCARRY    | register ref                                      |
|   0x0E |              |                 |     |   0x8E | SUB         | literal value                                     |
|   0x0F |              |                 |     |   0x8F | SUB         | register ref                                      |
|   0x10 |              |                 |     |   0x90 | SUBBORROW   | literal value                                     |
|   0x11 |              |                 |     |   0x91 | SUBBORROW   | register ref                                      |
|   0x12 |              |                 |     |   0x92 | MULT        | literal value                                     |
|   0x13 |              |                 |     |   0x93 | MULT        | register ref                                      |
|   0x14 |              |                 |     |   0x94 | SDIV        | literal value                                     |
|   0x15 |              |                 |     |   0x95 | SDIV        | register ref                                      |
|   0x16 |              |                 |     |   0x96 | UDIV        | literal value                                     |
|   0x17 |              |                 |     |   0x97 | UDIV        | register ref                                      |
|   0x18 |              |                 |     |   0x98 | REM         | literal value                                     |
|   0x19 |              |                 |     |   0x99 | REM         | register ref                                      |
|   0x1A |              |                 |     |   0x9A | AND         | literal integer                                   |
|   0x1B |              |                 |     |   0x9B | AND         | register ref                                      |
|   0x1C |              |                 |     |   0x9C | OR          | literal integer                                   |
|   0x1D |              |                 |     |   0x9D | OR          | register ref                                      |
|   0x1E |              |                 |     |   0x9E | XOR         | literal integer                                   |
|   0x1F |              |                 |     |   0x9F | XOR         | register ref                                      |
|   0x20 | TIMER        | literal integer |     |   0xA0 | LSHIFT      | literal integer                                   |
|   0x21 | TIMER        | register ref    |     |   0xA1 | LSHIFT      | register ref                                      |
|   0x22 | PUSH         | literal value   |     |   0xA2 | RSHIFTL     | literal integer                                   |
|   0x23 | PUSH         | register ref    |     |   0xA3 | RSHIFTL     | register ref                                      |
|   0x24 | POP          |                 |     |   0xA4 | RSHIFTA     | literal integer                                   |
|   0x25 | NEGATE       |                 |     |   0xA5 | RSHIFTA     | register ref                                      |
|   0x26 | CALL         |                 |     |   0xA6 | LROT        | literal integer                                   |
|   0x27 |              |                 |     |   0xA7 | LROT        | register ref                                      |
|   0x28 | NOT          |                 |     |   0xA8 | RROT        | literal integer                                   |
|   0x29 | JUMP         | literal address |     |   0xA9 | RROT        | register ref                                      |
|   0x2A | JUMP         | register ref    |     |   0xAA | LROTCARRY   | literal integer                                   |
|   0x2B | JEQUAL       | literal address |     |   0xAB | LROTCARRY   | register ref                                      |
|   0x2C | JEQUAL       | register ref    |     |   0xAC | RROTCARRY   | literal integer                                   |
|   0x2D | JNOTEQUAL    | literal address |     |   0xAD | RROTCARRY   | register ref                                      |
|   0x2E | JNOTEQUAL    | register ref    |     |   0xAE | COMPARE     | literal value / register ref                      |
|   0x2F | JGREATER     | literal address |     |   0xAF | COMPARE     | register ref / literal value                      |
|   0x30 | JGREATER     | register ref    |     |   0xB0 | COMPARE     | register ref / register ref                       |
|   0x31 | JGREATEREQ   | literal address |     |   0xB1 |             |                                                   |
|   0x32 | JGREATEREQ   | register ref    |     |   0xB2 |             |                                                   |
|   0x33 | JABOVE       | literal address |     |   0xB3 |             |                                                   |
|   0x34 | JABOVE       | register ref    |     |   0xB4 |             |                                                   |
|   0x35 | JABOVEEQ     | literal address |     |   0xB5 |             |                                                   |
|   0x36 | JABOVEEQ     | register ref    |     |   0xB6 |             |                                                   |
|   0x37 | JLESSER      | literal address |     |   0xB7 |             |                                                   |
|   0x38 | JLESSER      | register ref    |     |   0xB8 |             |                                                   |
|   0x39 | JLESSEREQ    | literal address |     |   0xB9 |             |                                                   |
|   0x3A | JLESSEREQ    | register ref    |     |   0xBA |             |                                                   |
|   0x3B | JLOWER       | literal address |     |   0xBB |             |                                                   |
|   0x3C | JLOWER       | register ref    |     |   0xBC |             |                                                   |
|   0x3D | JLOWEREQ     | literal address |     |   0xBD |             |                                                   |
|   0x3E | JLOWEREQ     | register ref    |     |   0xBE |             |                                                   |
|   0x3F | JOVERFLOW    | literal address |     |   0xBF |             |                                                   |
|   0x40 | JOVERFLOW    | register ref    |     |   0xC0 |             |                                                   |
|   0x41 | JNOTOVERFLOW | literal address |     |   0xC1 |             |                                                   |
|   0x42 | JNOTOVERFLOW | register ref    |     |   0xC2 |             |                                                   |
|   0x43 |              |                 |     |   0xC3 |             |                                                   |
|   0x44 |              |                 |     |   0xC4 |             |                                                   |
|   0x45 |              |                 |     |   0xC5 |             |                                                   |
|   0x46 |              |                 |     |   0xC6 |             |                                                   |
|   0x47 |              |                 |     |   0xC7 |             |                                                   |
|   0x48 |              |                 |     |   0xC8 |             |                                                   |
|   0x49 |              |                 |     |   0xC9 |             |                                                   |
|   0x4A |              |                 |     |   0xCA |             |                                                   |
|   0x4B |              |                 |     |   0xCB |             |                                                   |
|   0x4C |              |                 |     |   0xCC |             |                                                   |
|   0x4D |              |                 |     |   0xCD |             |                                                   |
|   0x4E |              |                 |     |   0xCE |             |                                                   |
|   0x4F |              |                 |     |   0xCF |             |                                                   |
|   0x50 |              |                 |     |   0xD0 |             |                                                   |
|   0x51 |              |                 |     |   0xD1 |             |                                                   |
|   0x52 |              |                 |     |   0xD2 |             |                                                   |
|   0x53 |              |                 |     |   0xD3 |             |                                                   |
|   0x54 |              |                 |     |   0xD4 |             |                                                   |
|   0x55 |              |                 |     |   0xD5 |             |                                                   |
|   0x56 |              |                 |     |   0xD6 |             |                                                   |
|   0x57 |              |                 |     |   0xD7 |             |                                                   |
|   0x58 |              |                 |     |   0xD8 |             |                                                   |
|   0x59 |              |                 |     |   0xD9 |             |                                                   |
|   0x5A |              |                 |     |   0xDA |             |                                                   |
|   0x5B |              |                 |     |   0xDB |             |                                                   |
|   0x5C |              |                 |     |   0xDC |             |                                                   |
|   0x5D |              |                 |     |   0xDD |             |                                                   |
|   0x5E |              |                 |     |   0xDE |             |                                                   |
|   0x5F |              |                 |     |   0xDF |             |                                                   |
|   0x60 |              |                 |     |   0xE0 | BLOCKCOPY   | literal address / literal address / literal value |
|   0x61 |              |                 |     |   0xE1 | BLOCKCOPY   | literal address / literal address / register ref  |
|   0x62 |              |                 |     |   0xE2 | BLOCKCOPY   | literal address / register ref / literal integer  |
|   0x63 |              |                 |     |   0xE3 | BLOCKCOPY   | literal address / register ref / register ref     |
|   0x64 |              |                 |     |   0xE4 | BLOCKCOPY   | register ref / literal address / literal integer  |
|   0x65 |              |                 |     |   0xE5 | BLOCKCOPY   | register ref / literal address / register ref     |
|   0x66 |              |                 |     |   0xE6 | BLOCKCOPY   | register ref / register ref / literal integer     |
|   0x67 |              |                 |     |   0xE7 | BLOCKCOPY   | register ref / register ref / register ref        |
|   0x68 |              |                 |     |   0xE8 |             |                                                   |
|   0x69 |              |                 |     |   0xE9 |             |                                                   |
|   0x6A |              |                 |     |   0xEA |             |                                                   |
|   0x6B |              |                 |     |   0xEB |             |                                                   |
|   0x6C |              |                 |     |   0xEC |             |                                                   |
|   0x6D |              |                 |     |   0xED |             |                                                   |
|   0x6E |              |                 |     |   0xEE |             |                                                   |
|   0x6F |              |                 |     |   0xEF |             |                                                   |
|   0x70 |              |                 |     |   0xF0 |             |                                                   |
|   0x71 |              |                 |     |   0xF1 |             |                                                   |
|   0x72 |              |                 |     |   0xF2 |             |                                                   |
|   0x73 |              |                 |     |   0xF3 |             |                                                   |
|   0x74 |              |                 |     |   0xF4 |             |                                                   |
|   0x75 |              |                 |     |   0xF5 |             |                                                   |
|   0x76 |              |                 |     |   0xF6 |             |                                                   |
|   0x77 |              |                 |     |   0xF7 |             |                                                   |
|   0x78 |              |                 |     |   0xF8 |             |                                                   |
|   0x79 |              |                 |     |   0xF9 |             |                                                   |
|   0x7A |              |                 |     |   0xFA |             |                                                   |
|   0x7B |              |                 |     |   0xFB |             |                                                   |
|   0x7C |              |                 |     |   0xFC |             |                                                   |
|   0x7D |              |                 |     |   0xFD |             |                                                   |
|   0x7E |              |                 |     |   0xFE |             |                                                   |
|   0x7F |              |                 |     |   0xFF |             |                                                   |
