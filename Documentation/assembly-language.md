# Simulatron Assembly Language Definition
### Version 2.0.0-alpha

## Overview
A single assembly file (`*.simasm`) gets [translated](../simulatron-salt/README.md) to a single [object code](object-code.md) file (`*.simobj`). One or more object code files can then be [linked](../simulatron-silk/README.md) together to form a final executable.

An assembly file contains a combination of four types of element:
1. Constant declarations
2. Data declarations
3. Labels
4. Instructions

A file must contain at least one instruction.

It is conventional to place all constant declarations at the top of the file, followed by data declarations, and then instruction blocks. Each instruction block (except optionally the first) should be preceded by a label, and labels may also appear in the middle of instruction blocks. However, these are just conventions, and any ordering is legal.

Comments are started by a double forward slash (`//`) and continue to the end of the line. These are ignored by the assembler.

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
Scientific notation is also allowed on decimal literals:
```
1e3
```

Floating point literals must be in decimal and have a decimal point:
```
1000.0
1.0e3

-0.01
-1.0e-2
```

Character literals are written with single quotes, and are converted to values according to their [character set representation](character-set.md):
```
'A'
65
```

Array literals enclose multiple literals in square brackets, separated by commas:
```
[1, 4, 5.6, 'B']
```
Note that there is no typechecking: arrays can mix and match literal types.

String literals are written with double quotes, and expand to character arrays:
```
"Hello"
['H', 'e', 'l', 'l', 'o']
[72, 101, 108, 108, 111]
```

Names, used for constants, static data, and labels, may contain alphanumeric characters and underscores, but must not start with a digit. They are case-sensitive.
```
foo
FOO
Bar99
BaZ
_foo_1_bar_
```

## Constant Declarations
Constant declarations create symbolic constants that have all occurrences replaced with the defined value before assembling. It is conventional that constant names are `UPPER_SNAKE_CASE`.

Syntax:
```
const <name> <value>
```
The value can be any non-array literal.

Example:
```
const PI 3.14159
```

## Data Declarations
Data declarations reserve space within the resulting object code for static data. Such data may be read-only or read/write.

The write mode only affects the sections generated in the resulting [object code file](object-code.md), and it is not guaranteed by the assembler that they will be enforced at runtime (this is determined by the linker/loader). Be aware that if you are creating a ROM image, read/write data declarations may be rejected by the linker as ROM is by definition read-only.

It is conventional that static data names are `lower_snake_case`.

Read-Only Syntax:
```
static <type> <name> <initialiser>
```
Read/Write Syntax:
```
static mut <type> <name> <initialiser>
```
where the type is one of `byte`, `half`, or `word`, or any of those three with an array suffix (e.g. `byte[5]`).
The initialiser is a literal. Note that arrays of arrays are allowed, and an array initialiser that is too short will be padded with zero.

Types do not persist beyond the point of declaration; they simply determine the size to allocate. Simulatron assembly is not a typed language.

Example:
```
static byte[13] message "Hello, World!"
static mut half counter 0
static word[5][2] primes_and_doubles [[2, 4], [3, 6], [5, 10], [7, 14], [9, 18]]
```

## Labels
Labels create named locations within the resulting object code, referring to the address of the following instruction. These are useful for branching instructions. It is conventional that label names are `lower_snake_case`.

Syntax:
```
<name>:
```

Example:
```
loop_start:
```

## Instructions
Instructions form the bulk of most files and are translated into Simulatron machine code in the resulting object code. An instruction consists of an opcode followed by zero or more operands. Opcodes are case-insensitive. Operands are always arranged such that data flows from right-to-left; if an instruction writes to a register, that register will be the first operand.

Note that due to different addressing modes, the same assembly opcode may map to multiple possible binary opcodes.

Syntax:
```
<opcode> <operands>...
```

Example:
```
add r0 5
```

### Addressing Modes
Many operands can take either a literal value or a register reference; in the case of a register reference, the value in that register will be used. Register references are specified as the lowercase register name (see the register list in the [instruction set](instruction-set.md#Registers-Available)).

### Calling Convention
Calling a subroutine:
1. The caller saves any registers they care about, typically by pushing them to the stack.
2. The caller pushes parameters onto the stack in reverse order, so the first parameter is pushed last.
3. The caller executes the `CALL` instruction, pushing the return address onto the stack and jumping to the callee.
4. The callee begins executing and can write to any register.

Returning from a subroutine:
1. The callee removes all of its local data and parameters from the stack.
2. The callee executes the `RETURN` instruction, popping the return address off the stack and jumping to it.
3. The caller restores the values of any registers it saved.

### Instruction Set
The main instruction set listing can be found [here](instruction-set.md#Instructions); Simulatron assembly uses the same set of instruction names. This document provides an additional summary of the available addressing modes for each instruction. Each operand's available modes are specified by a flag string of the following format:
```
bhwf

b: byte literal or register reference.
h: half literal or register reference.
w: word literal or register reference.
f: float literal or register reference.
```

A capital letter means that only register references are accepted, not literals.  A dot instead of a letter means this mode is not available. Some `w`/`W` entries are replaced with `a`/`A`; this indicates that the word is interpreted as an address. This distinction only exists in the mind of the programmer.

A constant name can be used anywhere a literal is expected, and a label or static data name can be used anywhere a word literal is expected (though you probably only want to use them for address operands).

Examples:
```
bhwf - any literal or register reference.
BHWF - any register reference.
..w. - a word literal or register reference.
..WF - a word or float register reference.
```

| Opcode      | Operand 1 | Operand 2 | Operand 3 |
| ----------- |:---------:|:---------:|:---------:|
| HALT        |           |           |           |
| PAUSE       |           |           |           |
| TIMER       |  `..w.`   |           |           |
| USERMODE    |           |           |           |
| IRETURN     |           |           |           |
| LOAD        |  `BHWF`   |  `..a.`   |           |
| STORE       |  `..a.`   |  `BHWF`   |           |
| COPY        |  `BHWF`   |  `bhwf`   |           |
| SWAP        |  `BHWF`   |  `..a.`   |           |
| PUSH        |  `BHWF`   |           |           |
| POP         |  `BHWF`   |           |           |
| BLOCKCOPY   |  `..w.`   |  `..a.`   |  `..a.`   |
| BLOCKSET    |  `..w.`   |  `..a.`   |  `b...`   |
| SCONVERT    |  `..WF`   |  `..WF`   |           |
| UCONVERT    |  `..WF`   |  `..WF`   |           |
| NEGATE      |  `BHWF`   |           |           |
| ADD         |  `BHWF`   |  `bhwf`   |           |
| ADDCARRY    |  `BHW.`   |  `bhw.`   |           |
| SUB         |  `BHWF`   |  `bhwf`   |           |
| SUBBORROW   |  `BHW.`   |  `bhw.`   |           |
| MULT        |  `BHWF`   |  `bhwf`   |           |
| SDIV        |  `BHWF`   |  `bhwf`   |           |
| UDIV        |  `BHW.`   |  `bhw.`   |           |
| SREM        |  `BHWF`   |  `bhwf`   |           |
| UREM        |  `BHW.`   |  `bhw.`   |           |
| NOT         |  `BHW.`   |           |           |
| AND         |  `BHW.`   |  `bhw.`   |           |
| OR          |  `BHW.`   |  `bhw.`   |           |
| XOR         |  `BHW.`   |  `bhw.`   |           |
| LSHIFT      |  `BHW.`   |  `b...`   |           |
| SRSHIFT     |  `BHW.`   |  `b...`   |           |
| URSHIFT     |  `BHW.`   |  `b...`   |           |
| LROT        |  `BHW.`   |  `b...`   |           |
| RROT        |  `BHW.`   |  `b...`   |           |
| LROTCARRY   |  `BHW.`   |  `b...`   |           |
| RROTCARRY   |  `BHW.`   |  `b...`   |           |
| JUMP        |  `..a.`   |           |           |
| COMPARE     |  `BHWF`   |  `bhwf`   |           |
| BLOCKCMP    |  `..w.`   |  `..a.`   |  `..a.`   |
| JEQUAL      |  `..a.`   |           |           |
| JNOTEQUAL   |  `..a.`   |           |           |
| SJGREATER   |  `..a.`   |           |           |
| SJGREATEREQ |  `..a.`   |           |           |
| UJGREATER   |  `..a.`   |           |           |
| UJGREATEREQ |  `..a.`   |           |           |
| SJLESSER    |  `..a.`   |           |           |
| SJLESSEREQ  |  `..a.`   |           |           |
| UJLESSER    |  `..a.`   |           |           |
| UJLESSEREQ  |  `..a.`   |           |           |
| CALL        |  `..a.`   |           |           |
| RETURN      |           |           |           |
| SYSCALL     |           |           |           |

## Language Grammar (EBNF)
```
Program = { Line "\n" } [ Line ] EOF ;

Line = [ Const | Data | Label | Instruction ] [ Comment ] ;

Comment = "//" { ? Any non-newline character ? } ;

Const = "const" Identifier Literal ;

Data = "static" [ "mut" ] Type Identifier ArrayLiteral ;

Type = "byte" | "half" | "word"
    | Type "[" IntLiteral "]" ;

Label = Identifier ":" ;

Instruction = Identifier { Operand } ;

Operand = Identifier | Literal ;

Identifier = Alphabetic { Alphanumeric } ;

Alphanumeric = Alphabetic | Digit ;

Alphabetic = "A" | "B" | ... | "Z" | "a" | "b" | ... | "z" | "_" ;

ArrayLiteral = Literal
    | StringLiteral
    | "[" [ ArrayLiteral { "," ArrayLiteral } ] "]" ;

StringLiteral = Quote { Character } Quote ;

Quote = ? Literal " ? ;

Literal = IntLiteral | FloatLiteral | CharLiteral ;

IntLiteral = [ "-" ] ( DecLiteral [ Exponent ] | BinLiteral | HexLiteral ) ;

FloatLiteral = [ "-" ] DecLiteral "." DecLiteral [ Exponent ] ;

Exponent = "e" [ "-" ] DecLiteral ;

DecLiteral = Digit { Digit } ;

BinLiteral = "0b" BinDigit { BinDigit } ;

HexLiteral = "0x" HexDigit { HexDigit } ;

HexDigit = Digit | "A" | "B" | ... | "F" | "a" | "b" | ... | "f" ;

Digit = BinDigit | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;

BinDigit = "0" | "1" ;

CharLiteral = "'" Character "'" ;

Character = ? Any non-newline and non-backslash character ?
    | ? Escaped newline \n ?
    | ? Escaped quote \" ?
    | ? Escaped backslash \\ ? ;
```
