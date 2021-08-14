# Instruction Set
### Version 2.0.0-alpha

## Registers Available
| Register Reference (hex) | Short name | Full Name                       | Description                                      |
| ------------------------:| ---------- | ------------------------------- | ------------------------------------------------ |
|                    00-07 | r0-r7      | Integer Registers 0-7 (full)    | 32-bit integer general purpose registers.        |
|                    08-0F | r0h-r7h    | Integer Registers 0-7 (half)    | Lower 16 bits of r0-r7.                          |
|                    10-17 | r0b-r7b    | Integer Registers 0-7 (byte)    | Lowest 8 bits of r0-r7.                          |
|                    18-1F | f0-f7      | Float Registers 0-7             | 32-bit floating-point general purpose registers. |
|                       20 | FLAGS      | Flags Register                  | Holds the flags as described below. 16 bits.     |
|                       21 | USPR       | User Stack Pointer Register     | Points to the current top of the user stack.     |
|                       22 | KSPR       | Kernel Stack Pointer Register   | Points to the current top of the kernel stack.   |
|                       23 | PDPR       | Page Directory Pointer Register | Points to the current page directory.            |
|                       24 | IMR        | Interrupt Mask Register         | Enables/disables specific interrupts. 16 bits.   |

KSPR, PDPR, and IMR are privileged registers; they can only be accessed in kernel mode.

To move values between integer and floating-point registers, the COPY instruction should be used. This automatically performs the required conversions. Storing a float to memory and then loading it as an integer (or vice versa) will NOT perform any conversion.

Integers are stored in big-endian 2's complement representation; floats are stored in the IEEE 754 binary32 representation.

Operations on the lower bits of r0-r7 will consider the register to be of that size, e.g. `COPY r0 255` then `ADD r0b 1` will overflow to zero.
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

### Interrupt definitions
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
                              0xFAFFCAFE
```



## Instructions
A full list of all instructions is given below. Some instructions are mapped to more than one opcode due to differing operand combinations.

Operands are ordered such that data flows from right-to-left. This makes it possible for destination register references to be inspected before fetching the source operand, which in turn makes it possible for the source operand length to vary on the destination register. Of course, any assembly language implementations do not need to follow this pattern, and are free to switch operand orders in the source code if a left-to-right flow is deemed to be more intuitive.

### Privileged instructions
These are only executable in kernel mode. If the CPU is in user mode, an illegal operation interrupt will be raised.

`HALT`: Immediately halt the processor. No further instructions will be executed under any circumstances, and the machine is safe to power off.

`PAUSE`: Temporarily halt the processor. Any enabled interrupt will wake the processor, which will resume from where it left off and immediately execute the interrupt handler.

`TIMER num_milliseconds`: Set the interrupt timer. It will send a timer interrupt after at least the given number of milliseconds, repeating indefinitely with the same period. A value of zero will disable the timer.

`USERMODE`: Pop the target address off the stack, clear the flags, enter user mode, and jump to the target address. Note that the address will be interpreted as virtual by the current page table.

`IRETURN`: See the interrupt section.

### Data movement instructions
`LOAD register address`: Load from the given memory address into the given register.

`STORE address register`: Store the given register into the given memory address.

`COPY destination source`: Either load a register with a constant value, or copy one register into another. This can copy between integer and floating-point registers, and automatically converts the values into the destination representation. Note that converting a float to an integer will truncate towards zero.

`SWAP register address`: Atomically exchange the values of a memory location and a register.

`PUSH register`: Decrement the stack pointer by the appropriate amount and then copy the given register to the stack.

`POP register`: Copy the top of the stack into the given register and then increment the stack pointer by the appropriate amount.

`BLOCKCOPY length destination source`: Copy `length` bytes from the source memory address to the destination memory address. 

Note that PUSH and POP will use KSPR if in kernel mode, and USPR if in user mode.

### Arithmetic instructions
Operand types must match, i.e. `ADD r0 f0` would be illegal.

`NEGATE register`: Arithmetically negate the given register.

`ADD register value`: Add the given value to the given register.

`ADDCARRY register value`: Add the given value plus the `C` flag to the given register. Not applicable to floats.

`SUB register value`: Subtract the given value from the given register.

`SUBBORROW register value`: Subtract the given value plus the `C` flag from the given register. Not applicable to floats.

`MULT register value`: Multiply the given register by the given value.

`SDIV register value`: Signed division; divide the given register by the given value. Integer division will truncate towards zero.

`UDIV register value`: Unsigned division; divide the given register by the given value. Truncates towards negative infinity. Not applicable to floats.

`SREM register value`: Signed division; divide the given register by the given value and store the remainder in the register.

`UREM register value`: Unsigned division; divide the given register by the given value and store the remainder in the register. Not applicable to floats.

### Bitwise instructions
None of these instructions are applicable to floats.

`NOT register`: Logically negate the given register.

`AND register value`: Logically AND the given register with the given value.

`OR register value`: Logically OR the given register with the given value.

`XOR register value`: Logically XOR the given register with the given value.

`LSHIFT register num_bits`: Shift the given register left by the given number of bits.

`RSHIFTL register num_bits`: Logical/Unsigned shift the given register right by the given number of bits; left-most bits will be filled with zeroes.

`RSHIFTA register num_bits`: Arithmetic/Signed shift the given register right by the given number of bits; left-most bits will be filled with the sign bit.

`LROT register num_bits`: Rotate the given register left by the given number of bits.

`RROT register num_bits`: Rotate the given register right by the given number of bits.

`LROTCARRY register num_bits`: Rotate the given register left by the given number of bits, including the `C` flag in the rotation as if it were to the left of the register.

`RROTCARRY register num_bits`: Rotate the given register right by the given number of bits, including the `C` flag in the rotation as if it were to the right of the register.

### Flow control instructions
`JUMP address`: Unconditionally jump to the given address.

`COMPARE value1 value2`: Subtract `value1` from `value2` and discard the result. Flags will still be set.

`JEQUAL address`: Jump to the given address if the last comparison had `value1 = value2`, i.e. `Z`=1.

`JNOTEQUAL address`: Jump to the given address if the last comparison had `value1 != value2`, i.e. `Z`=0.

`JGREATER address`: Jump to the given address if the last comparison had `value1 < value2` (signed comparison), i.e. `N`=`O` and `Z`=0.

`JGREATEREQ address`: Jump to the given address if the last comparison had `value1 <= value2` (signed comparison), i.e. `N`=`O` or `Z`=1.

`JABOVE address`: Like JGREATER but unsigned, i.e. `C`=0 and `Z`=0.

`JABOVEEQ address`: Like JGREATEREQ but unsigned, i.e. `C`=0 or `Z`=1.

`JLESSER address`: Jump to the given address if the last comparison had `value1 > value2` (signed comparison), i.e. `N`!=`O`.

`JLESSEREQ address`: Jump to the given address if the last comparison had `value1 >= value2` (signed comparison), i.e. `N`!=`O` or `Z`=1.

`JBELOW address`: Like JLESSER but unsigned, i.e. `C`=1.

`JBELOWEQ address`: Like JLESSEREQ but unsigned, i.e. `C`=1 or `Z`=1.

`JOVERFLOW address`: Jump to the given address if the last operation overflowed, i.e. `O`=1.

`JNOTOVERFLOW address`: Jump to the given address if the last operation didn't overflow, i.e. `O`=0.

`CALL address`: Push the address of the next instruction followed by the current flags to the stack, and unconditionally jump to the given address.

`RETURN`: Pop flags from the stack, then pop the return address from the stack and unconditionally jump to it.

`SYSCALL`: Raise a syscall interrupt.

## Opcodes
Opcodes have a fixed length of one byte. The number of operands depends on the opcode; the length of operands depends on the opcode and potentially on other operands that appear earlier. 

If an unmapped opcode is encountered, no operation will take place and an illegal operation interrupt will be raised.

Note that if an opcode takes multiple register references to registers with unspecified lengths, the lengths must match, or an illegal operation interrupt will be raised.

### Operand types
`Literal address`: A 4-byte literal address.

`Register ref address`: A 1-byte register reference to any 32-bit integer register, the contents of which will be interpreted to contain a 4-byte address.

`Register ref integer`: A 1-byte register reference to any integer register, the contents of which will be interpreted as an integer of the appropriate length.

`Literal byte`: A 1-byte literal integer.

`Register ref byte`: A 1-byte register reference to one of r0b-r7b, the contents of which will be interpreted to contain a 1-byte integer.

`Literal word`: A 4-byte literal integer.

`Register ref word`: A 1-byte register reference to any 32-bit integer register, the contents of which will be interpreted to contain a 4-byte integer. This is equivalent in all but name to `Register ref address`.

`Register ref`: A 1-byte reference to any register.

`Variable literal`: A variable-length literal which may represent a 1, 2, or 4 byte integer, or a 4-byte float. These only appear after a `Register ref` operand; the length of the register referred to defines the literal length.

`Variable integer literal`: A variable-length literal which may represent a 1, 2, or 4 byte integer. These only appear after a `Register ref integer` operand; the length of the register referred to defines the literal length.

### Opcode table
|Opcode|Instruction |Operand 1 type      |Operand 2 type          |Operand 3 type      |
|-----:|------------|--------------------|------------------------|--------------------|
|  0x00|HALT        |                    |                        |                    |
|  0x01|PAUSE       |                    |                        |                    |
|  0x02|TIMER       |Literal word        |                        |                    |
|  0x03|TIMER       |Register ref word   |                        |                    |
|  0x04|USERMODE    |                    |                        |                    |
|  0x05|IRETURN     |                    |                        |                    |
|  0x06|LOAD        |Register ref        |Literal address         |                    |
|  0x07|LOAD        |Register ref        |Register ref address    |                    |
|  0x08|STORE       |Literal address     |Register ref            |                    |
|  0x09|STORE       |Register ref address|Register ref            |                    |
|  0x0A|COPY        |Register ref        |Variable literal        |                    |
|  0x0B|COPY        |Register ref        |Register ref            |                    |
|  0x0C|SWAP        |Register ref        |Literal address         |                    |
|  0x0D|SWAP        |Register ref        |Register ref address    |                    |
|  0x0E|PUSH        |Register ref        |                        |                    |
|  0x0F|POP         |Register ref        |                        |                    |
|  0x10|BLOCKCOPY   |Literal word        |Literal address         |Literal address     |
|  0x11|BLOCKCOPY   |Literal word        |Literal address         |Register ref address|
|  0x12|BLOCKCOPY   |Literal word        |Register ref address    |Literal address     |
|  0x13|BLOCKCOPY   |Literal word        |Register ref address    |Register ref address|
|  0x14|BLOCKCOPY   |Register ref word   |Literal address         |Literal address     |
|  0x15|BLOCKCOPY   |Register ref word   |Literal address         |Register ref address|
|  0x16|BLOCKCOPY   |Register ref word   |Register ref address    |Literal address     |
|  0x17|BLOCKCOPY   |Register ref word   |Register ref address    |Register ref address|
|  0x18|NEGATE      |Register ref        |                        |                    |
|  0x19|ADD         |Register ref        |Variable literal        |                    |
|  0x1A|ADD         |Register ref        |Register ref            |                    |
|  0x1B|ADDCARRY    |Register ref integer|Variable literal        |                    |
|  0x1C|ADDCARRY    |Register ref integer|Register ref integer    |                    |
|  0x1D|SUB         |Register ref        |Variable literal        |                    |
|  0x1E|SUB         |Register ref        |Register ref            |                    |
|  0x1F|SUBBORROW   |Register ref integer|Variable literal        |                    |
|  0x20|SUBBORROW   |Register ref integer|Register ref integer    |                    |
|  0x21|MULT        |Register ref        |Variable literal        |                    |
|  0x22|MULT        |Register ref        |Register ref            |                    |
|  0x23|SDIV        |Register ref        |Variable literal        |                    |
|  0x24|SDIV        |Register ref        |Register ref            |                    |
|  0x25|UDIV        |Register ref integer|Variable literal        |                    |
|  0x26|UDIV        |Register ref integer|Register ref integer    |                    |
|  0x27|SREM        |Register ref        |Variable literal        |                    |
|  0x28|SREM        |Register ref        |Register ref            |                    |
|  0x29|UREM        |Register ref integer|Variable literal        |                    |
|  0x2A|UREM        |Register ref integer|Register ref integer    |                    |
|  0x2B|NOT         |Register ref integer|                        |                    |
|  0x2C|AND         |Register ref integer|Variable integer literal|                    |
|  0x2D|AND         |Register ref integer|Register ref integer    |                    |
|  0x2E|OR          |Register ref integer|Variable integer literal|                    |
|  0x2F|OR          |Register ref integer|Register ref integer    |                    |
|  0x30|XOR         |Register ref integer|Variable integer literal|                    |
|  0x31|XOR         |Register ref integer|Register ref integer    |                    |
|  0x32|LSHIFT      |Register ref integer|Literal byte            |                    |
|  0x33|LSHIFT      |Register ref integer|Register ref byte       |                    |
|  0x34|RSHIFTL     |Register ref integer|Literal byte            |                    |
|  0x35|RSHIFTL     |Register ref integer|Register ref byte       |                    |
|  0x36|RSHIFTA     |Register ref integer|Literal byte            |                    |
|  0x37|RSHIFTA     |Register ref integer|Register ref byte       |                    |
|  0x38|LROT        |Register ref integer|Literal byte            |                    |
|  0x39|LROT        |Register ref integer|Register ref byte       |                    |
|  0x3A|RROT        |Register ref integer|Literal byte            |                    |
|  0x3B|RROT        |Register ref integer|Register ref byte       |                    |
|  0x3C|LROTCARRY   |Register ref integer|Literal byte            |                    |
|  0x3D|LROTCARRY   |Register ref integer|Register ref byte       |                    |
|  0x3E|RROTCARRY   |Register ref integer|Literal byte            |                    |
|  0x3F|RROTCARRY   |Register ref integer|Register ref byte       |                    |
|  0x40|JUMP        |Literal address     |                        |                    |
|  0x41|JUMP        |Register ref address|                        |                    |
|  0x42|COMPARE     |Register ref        |Variable literal        |                    |
|  0x43|COMPARE     |Register ref        |Register ref            |                    |
|  0x44|JEQUAL      |Literal address     |                        |                    |
|  0x45|JEQUAL      |Register ref address|                        |                    |
|  0x46|JNOTEQUAL   |Literal address     |                        |                    |
|  0x47|JNOTEQUAL   |Register ref address|                        |                    |
|  0x48|JGREATER    |Literal address     |                        |                    |
|  0x49|JGREATER    |Register ref address|                        |                    |
|  0x4A|JGREATEREQ  |Literal address     |                        |                    |
|  0x4B|JGREATEREQ  |Register ref address|                        |                    |
|  0x4C|JABOVE      |Literal address     |                        |                    |
|  0x4D|JABOVE      |Register ref address|                        |                    |
|  0x4E|JABOVEEQ    |Literal address     |                        |                    |
|  0x4F|JABOVEEQ    |Register ref address|                        |                    |
|  0x50|JLESSER     |Literal address     |                        |                    |
|  0x51|JLESSER     |Register ref address|                        |                    |
|  0x52|JLESSEREQ   |Literal address     |                        |                    |
|  0x53|JLESSEREQ   |Register ref address|                        |                    |
|  0x54|JBELOW      |Literal address     |                        |                    |
|  0x55|JBELOW      |Register ref address|                        |                    |
|  0x56|JBELOWEQ    |Literal address     |                        |                    |
|  0x57|JBELOWEQ    |Register ref address|                        |                    |
|  0x58|JOVERFLOW   |Literal address     |                        |                    |
|  0x59|JOVERFLOW   |Register ref address|                        |                    |
|  0x5A|JNOTOVERFLOW|Literal address     |                        |                    |
|  0x5B|JNOTOVERFLOW|Register ref address|                        |                    |
|  0x5C|CALL        |Literal address     |                        |                    |
|  0x5D|CALL        |Register ref address|                        |                    |
|  0x5E|RETURN      |                    |                        |                    |
|  0x5F|SYSCALL     |                    |                        |                    |
|  0x60|            |                    |                        |                    |
|  0x61|            |                    |                        |                    |
|  0x62|            |                    |                        |                    |
|  0x63|            |                    |                        |                    |
|  0x64|            |                    |                        |                    |
|  0x65|            |                    |                        |                    |
|  0x66|            |                    |                        |                    |
|  0x67|            |                    |                        |                    |
|  0x68|            |                    |                        |                    |
|  0x69|            |                    |                        |                    |
|  0x6A|            |                    |                        |                    |
|  0x6B|            |                    |                        |                    |
|  0x6C|            |                    |                        |                    |
|  0x6D|            |                    |                        |                    |
|  0x6E|            |                    |                        |                    |
|  0x6F|            |                    |                        |                    |
|  0x70|            |                    |                        |                    |
|  0x71|            |                    |                        |                    |
|  0x72|            |                    |                        |                    |
|  0x73|            |                    |                        |                    |
|  0x74|            |                    |                        |                    |
|  0x75|            |                    |                        |                    |
|  0x76|            |                    |                        |                    |
|  0x77|            |                    |                        |                    |
|  0x78|            |                    |                        |                    |
|  0x79|            |                    |                        |                    |
|  0x7A|            |                    |                        |                    |
|  0x7B|            |                    |                        |                    |
|  0x7C|            |                    |                        |                    |
|  0x7D|            |                    |                        |                    |
|  0x7E|            |                    |                        |                    |
|  0x7F|            |                    |                        |                    |
|  0x80|            |                    |                        |                    |
|  0x81|            |                    |                        |                    |
|  0x82|            |                    |                        |                    |
|  0x83|            |                    |                        |                    |
|  0x84|            |                    |                        |                    |
|  0x85|            |                    |                        |                    |
|  0x86|            |                    |                        |                    |
|  0x87|            |                    |                        |                    |
|  0x88|            |                    |                        |                    |
|  0x89|            |                    |                        |                    |
|  0x8A|            |                    |                        |                    |
|  0x8B|            |                    |                        |                    |
|  0x8C|            |                    |                        |                    |
|  0x8D|            |                    |                        |                    |
|  0x8E|            |                    |                        |                    |
|  0x8F|            |                    |                        |                    |
|  0x90|            |                    |                        |                    |
|  0x91|            |                    |                        |                    |
|  0x92|            |                    |                        |                    |
|  0x93|            |                    |                        |                    |
|  0x94|            |                    |                        |                    |
|  0x95|            |                    |                        |                    |
|  0x96|            |                    |                        |                    |
|  0x97|            |                    |                        |                    |
|  0x98|            |                    |                        |                    |
|  0x99|            |                    |                        |                    |
|  0x9A|            |                    |                        |                    |
|  0x9B|            |                    |                        |                    |
|  0x9C|            |                    |                        |                    |
|  0x9D|            |                    |                        |                    |
|  0x9E|            |                    |                        |                    |
|  0x9F|            |                    |                        |                    |
|  0xA0|            |                    |                        |                    |
|  0xA1|            |                    |                        |                    |
|  0xA2|            |                    |                        |                    |
|  0xA3|            |                    |                        |                    |
|  0xA4|            |                    |                        |                    |
|  0xA5|            |                    |                        |                    |
|  0xA6|            |                    |                        |                    |
|  0xA7|            |                    |                        |                    |
|  0xA8|            |                    |                        |                    |
|  0xA9|            |                    |                        |                    |
|  0xAA|            |                    |                        |                    |
|  0xAB|            |                    |                        |                    |
|  0xAC|            |                    |                        |                    |
|  0xAD|            |                    |                        |                    |
|  0xAE|            |                    |                        |                    |
|  0xAF|            |                    |                        |                    |
|  0xB0|            |                    |                        |                    |
|  0xB1|            |                    |                        |                    |
|  0xB2|            |                    |                        |                    |
|  0xB3|            |                    |                        |                    |
|  0xB4|            |                    |                        |                    |
|  0xB5|            |                    |                        |                    |
|  0xB6|            |                    |                        |                    |
|  0xB7|            |                    |                        |                    |
|  0xB8|            |                    |                        |                    |
|  0xB9|            |                    |                        |                    |
|  0xBA|            |                    |                        |                    |
|  0xBB|            |                    |                        |                    |
|  0xBC|            |                    |                        |                    |
|  0xBD|            |                    |                        |                    |
|  0xBE|            |                    |                        |                    |
|  0xBF|            |                    |                        |                    |
|  0xC0|            |                    |                        |                    |
|  0xC1|            |                    |                        |                    |
|  0xC2|            |                    |                        |                    |
|  0xC3|            |                    |                        |                    |
|  0xC4|            |                    |                        |                    |
|  0xC5|            |                    |                        |                    |
|  0xC6|            |                    |                        |                    |
|  0xC7|            |                    |                        |                    |
|  0xC8|            |                    |                        |                    |
|  0xC9|            |                    |                        |                    |
|  0xCA|            |                    |                        |                    |
|  0xCB|            |                    |                        |                    |
|  0xCC|            |                    |                        |                    |
|  0xCD|            |                    |                        |                    |
|  0xCE|            |                    |                        |                    |
|  0xCF|            |                    |                        |                    |
|  0xD0|            |                    |                        |                    |
|  0xD1|            |                    |                        |                    |
|  0xD2|            |                    |                        |                    |
|  0xD3|            |                    |                        |                    |
|  0xD4|            |                    |                        |                    |
|  0xD5|            |                    |                        |                    |
|  0xD6|            |                    |                        |                    |
|  0xD7|            |                    |                        |                    |
|  0xD8|            |                    |                        |                    |
|  0xD9|            |                    |                        |                    |
|  0xDA|            |                    |                        |                    |
|  0xDB|            |                    |                        |                    |
|  0xDC|            |                    |                        |                    |
|  0xDD|            |                    |                        |                    |
|  0xDE|            |                    |                        |                    |
|  0xDF|            |                    |                        |                    |
|  0xE0|            |                    |                        |                    |
|  0xE1|            |                    |                        |                    |
|  0xE2|            |                    |                        |                    |
|  0xE3|            |                    |                        |                    |
|  0xE4|            |                    |                        |                    |
|  0xE5|            |                    |                        |                    |
|  0xE6|            |                    |                        |                    |
|  0xE7|            |                    |                        |                    |
|  0xE8|            |                    |                        |                    |
|  0xE9|            |                    |                        |                    |
|  0xEA|            |                    |                        |                    |
|  0xEB|            |                    |                        |                    |
|  0xEC|            |                    |                        |                    |
|  0xED|            |                    |                        |                    |
|  0xEE|            |                    |                        |                    |
|  0xEF|            |                    |                        |                    |
|  0xF0|            |                    |                        |                    |
|  0xF1|            |                    |                        |                    |
|  0xF2|            |                    |                        |                    |
|  0xF3|            |                    |                        |                    |
|  0xF4|            |                    |                        |                    |
|  0xF5|            |                    |                        |                    |
|  0xF6|            |                    |                        |                    |
|  0xF7|            |                    |                        |                    |
|  0xF8|            |                    |                        |                    |
|  0xF9|            |                    |                        |                    |
|  0xFA|            |                    |                        |                    |
|  0xFB|            |                    |                        |                    |
|  0xFC|            |                    |                        |                    |
|  0xFD|            |                    |                        |                    |
|  0xFE|            |                    |                        |                    |
|  0xFF|            |                    |                        |                    |
