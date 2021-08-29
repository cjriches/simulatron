# Simulatron Assembly Language Definition
### Version 2.0.0-alpha

## Overview
A single assembly file (`*.simasm`) gets [translated](../simulatron-salt/README.md) to a single [object code](object-code.md) file (`*.simobj`). One or more object code files can then be [linked](../simulatron-silk/README.md) together to a final executable.

An assembly file contains a combination of four types of element:
1. Constant declarations
2. Data declarations
3. Labels
4. Instructions

It is conventional to place all constant declarations at the top of the file, followed by data declarations, and then instruction blocks. Each instruction block should be preceded by a label, and labels may also appear in the middle of instruction blocks. However, these are just conventions, and any ordering is legal.

## Literals and identifiers
Integer literals can be specified in decimal, binary, and hexadecimal:
```
42
0b101010
0x2A
```
If you do not specify the all the bits of the number in binary or hex, the number is assumed to be positive. Negative numbers can either specify the full 2's complement bit pattern, or use a minus symbol:
```
-42
-0b101010
0b11010110
-0x2A
0xD6
```
Scientific notation is also allowed:
```
1e3
1000
```

Floating point literals must have a decimal point:
```
1000.0
1.0e3

-0.01
-1.0e-2
```

Character literals are written with single quotes, and are converted to values according to their [character set representation](character-set.md).
```
'A'
65
```

Array literals enclose multiple literals in square brackets, separated by commas:
```
[1, 4, 5.6, 'B']
```
Note that there is no typechecking: arrays can mix and match literal types.

String literals are written with double quotes, and expand to character arrays.
```
"Hello"
['H', 'e', 'l', 'l', 'o']
[72, 101, 108, 108, 111]
```

Names, used for constants, static data, and labels, may contain alphanumeric characters and underscores, but must not start with a digit. They are case-sensitive.
```
foo
Bar99
BAZ
_foo_1_bar_
```

## Constant Declarations
Constant declarations create symbolic constants that have all occurrences replaced with the defined value before assembling.

Syntax:
```
const <name> <value>
```

Example:
```
const PI 3.14159
```

## Data Declarations
Data declarations reserve space within the resulting object code for static data. Such data may be read-only or read/write.

The write mode only affects the sections generated in the resulting [object code file](object-code.md), and it is not guaranteed by the assembler that they will be enforced at runtime (this is determined by the linker/loader). Be aware that if you are creating a ROM image, read/write data declarations may be rejected by the linker as ROM is by definition read-only.

Read-Only Syntax:
```
static <type> <name> <initialiser>
```
Read/Write Syntax:
```
static mut <type> <name> <initialiser>
```
where the type is one of `byte`, `half`, or `word`, or any of those three with an array suffix (e.g. `byte[5]`).
The initialiser is a single or array literal as appropriate. Note that arrays of arrays are allowed, and an array initialiser that is too short will be padded with zero.

Types do not persist beyond the point of declaration; they simply determine the size to allocate. Simulatron assembly is not a typed language.

Example:
```
static byte[13] message "Hello, World!"
static mut half counter 0
static word[5][2] primes_and_doubles [[2, 4], [3, 6], [5, 10], [7, 14], [9, 18]]
```

## Labels
Labels create named locations within the resulting object code, referring to the address of the following instruction. These are useful for branching instructions.

Syntax:
```
<name>:
```

Example:
```
loop_start:
```

## Instructions
Instructions form the bulk of most files and are translated into Simulatron machine code in the resulting object code. An instruction consists of an opcode followed by zero or more operands. Operands are always arranged such that data flows from right-to-left; if an instruction writes to a register, that register will be the first operand.

Syntax:
```
<opcode> <operands>...
```

Example:
```
add r0 5
```

### Addressing modes
Many operands can take either a literal value or a register reference; in the case of a register reference, the value in that register will be used. Register references are specified as the non-case-sensitive register name (see the register list in the [instruction set](instruction-set.md)).

### Instruction Set
TODO

## Full Language Grammar Listing
TODO
