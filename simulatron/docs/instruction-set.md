# Instruction Set
### Version 1.0

## Assembly Instructions

##### Key
| Description | Meaning                                                   |
| ----------- | --------------------------------------------------------- |
| Address     | Literal address, label, or register containing an address |
| Value       | Literal value or register containing a value              |
| Register    | A register reference                                      |

##### Privileged instructions
| Description              | Instruction  | Operand 1              |
| ------------------------ | ------------ | ---------------------- |
| Halt                     | HALT         |                        |
| Pause                    | PAUSE        |                        |
| Set timer                | TIMER        | Num milliseconds value |
| Enter user mode          | USERMODE     |                        |

##### Data Movement
| Description       | Instruction | Operand 1            | Operand 2            | Operand 3    |
| ----------------- | ----------- | -------------------- | -------------------- | ------------ |
| Load              | LOAD        | Source address       | Destination register |              |
| Store             | STORE       | Source value         | Destination address  |              |
| Move              | MOVE        | Source value         | Destination register |              |
| Atomic Swap       | SWAP        | Source register      | Source address       |              |
| Push              | PUSH        | Source value         |                      |              |
| Pop               | POP         | Destination register |                      |              |
| Block Memory Copy | BLOCKCOPY   | Source address       | Destination address  | Length value |

##### Arithmetic
| Description          | Instruction | Operand 1                        | Operand 2                         |
| -------------------- | ----------- | -------------------------------- | --------------------------------- |
| Add                  | ADD         | Operand value                    | Operand and destination register  |
| Add with carry       | ADDCARRY    | Operand value                    | Operand and destination register  |
| Subtract             | SUB         | Subtrahend value                 | Minuend and destination register  |
| Subtract with borrow | SUBBORROW   | Subtrahend value                 | Minuend and destination register  |
| Multiply             | MULT        | Operand value                    | Operand and destination register  |
| Signed division      | SDIV        | Divisor value                    | Dividend and destination register |
| Unsigned division    | UDIV        | Divisor value                    | Dividend and destination register |
| Remainder            | REM         | Divisor value                    | Dividend and destination register |
| Negate               | NEGATE      | Operand and destination register |                                   |
| Increment            | INC         | Operand and destination register |                                   |
| Decrement            | DEC         | Operand and destination register |                                   |

##### Bitwise operations
| Description             | Instruction | Operand 1                        | Operand 2                        |
| ----------------------- | ----------- | -------------------------------- | -------------------------------- |
| Not                     | NOT         | Operand and destination register |                                  |
| And                     | AND         | Operand value                    | Operand and destination register |
| Or                      | OR          | Operand value                    | Operand and destination register |
| Xor                     | XOR         | Operand value                    | Operand and destination register |
| Left Shift              | LSHIFT      | Num places value                 | Operand and destination register |
| Logical Right Shift     | RSHIFTL     | Num places value                 | Operand and destination register |
| Arithmetic Right Shift  | RSHIFTA     | Num places value                 | Operand and destination register |
| Left Rotate             | LROT        | Num places value                 | Operand and destination register |
| Right Rotate            | RROT        | Num places value                 | Operand and destination register |
| Left Rotate with carry  | LROTCARRY   | Num places value                 | Operand and destination register |
| Right Rotate with carry | RROTCARRY   | Num places value                 | Operand and destination register |

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
| Call (3)                 | CALL         | Target address   |                |
| Return (3)               | RETURN       |                  |                |
| Syscall                  | SYSCALL      |                  |                |

```
(1): JZERO is a synonym for JEQUAL; the same opcode is used.
(2): JNOTZERO is a synonym for JNOTEQUAL; the same opcode is used.
(3): CALL and RETURN have no opcode; they are implemented in terms of PUSH/POP and JUMP.

Note that above/below are the unsigned versions of greater/less.
```

## Opcodes
If an unmapped opcode is encountered, no operation will take place and an illegal operation interrupt will be raised.

| Opcode | Instruction  | Variant         |     | Opcode | Instruction | Variant                                           |
| ------:| ------------ | --------------- | --- | ------:| ----------- | ------------------------------------------------- |
|   0x00 | HALT         |                 |     |   0x80 | LOAD        | literal address                                   |
|   0x01 | PAUSE        |                 |     |   0x81 | LOAD        | register ref                                      |
|   0x02 | USERMODE     |                 |     |   0x82 | STORE       | literal address / literal address                 |
|   0x03 | SYSCALL      |                 |     |   0x83 | STORE       | literal address / register ref                    |
|   0x04 |              |                 |     |   0x84 | STORE       | register ref / literal address                    |
|   0x05 |              |                 |     |   0x85 | STORE       | register ref / register ref                       |
|   0x06 |              |                 |     |   0x86 | MOVE        | literal value                                     |
|   0x07 |              |                 |     |   0x87 | MOVE        | register ref                                      |
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
|   0x1A |              |                 |     |   0x9A | AND         | literal value                                     |
|   0x1B |              |                 |     |   0x9B | AND         | register ref                                      |
|   0x1C |              |                 |     |   0x9C | OR          | literal value                                     |
|   0x1D |              |                 |     |   0x9D | OR          | register ref                                      |
|   0x1E |              |                 |     |   0x9E | XOR         | literal value                                     |
|   0x1F |              |                 |     |   0x9F | XOR         | register ref                                      |
|   0x20 | TIMER        | literal value   |     |   0xA0 | LSHIFT      | literal value                                     |
|   0x21 | TIMER        | register ref    |     |   0xA1 | LSHIFT      | register ref                                      |
|   0x22 | PUSH         | literal value   |     |   0xA2 | RSHIFTL     | literal value                                     |
|   0x23 | PUSH         | register ref    |     |   0xA3 | RSHIFTL     | register ref                                      |
|   0x24 | POP          |                 |     |   0xA4 | RSHIFTA     | literal value                                     |
|   0x25 | NEGATE       |                 |     |   0xA5 | RSHIFTA     | register ref                                      |
|   0x26 | INC          |                 |     |   0xA6 | LROT        | literal value                                     |
|   0x27 | DEC          |                 |     |   0xA7 | LROT        | register ref                                      |
|   0x28 | NOT          |                 |     |   0xA8 | RROT        | literal value                                     |
|   0x29 | JUMP         | literal address |     |   0xA9 | RROT        | register ref                                      |
|   0x2A | JUMP         | register ref    |     |   0xAA | LROTCARRY   | literal value                                     |
|   0x2B | JEQUAL       | literal address |     |   0xAB | LROTCARRY   | register ref                                      |
|   0x2C | JEQUAL       | register ref    |     |   0xAC | RROTCARRY   | literal value                                     |
|   0x2D | JNOTEQUAL    | literal address |     |   0xAD | RROTCARRY   | register ref                                      |
|   0x2E | JNOTEQUAL    | register ref    |     |   0xAE | COMPARE     | literal value / literal value                     |
|   0x2F | JGREATER     | literal address |     |   0xAF | COMPARE     | literal value / register ref                      |
|   0x30 | JGREATER     | register ref    |     |   0xB0 | COMPARE     | register ref / literal value                      |
|   0x31 | JGREATEREQ   | literal address |     |   0xB1 | COMPARE     | register ref / register ref                       |
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
|   0x62 |              |                 |     |   0xE2 | BLOCKCOPY   | literal address / register ref / literal value    |
|   0x63 |              |                 |     |   0xE3 | BLOCKCOPY   | literal address / register ref / register ref     |
|   0x64 |              |                 |     |   0xE4 | BLOCKCOPY   | register ref / literal address / literal value    |
|   0x65 |              |                 |     |   0xE5 | BLOCKCOPY   | register ref / literal address / register ref     |
|   0x66 |              |                 |     |   0xE6 | BLOCKCOPY   | register ref / register ref / literal value       |
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
