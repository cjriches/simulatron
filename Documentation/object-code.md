# Simulatron Object Code Definition
### Version 2.0.0-alpha

## Overview
A single object code file (`*.simobj`) contains four things in the following order:
1. One SIMOBJ header.
2. One symbol table.
3. X section headers.
4. X sections.

The header describes overall metadata and the layout of the other elements. The symbol table describes symbols that are spliced in to the sections at link time, allowing code to be relocated and multiple files to be linked together. The section headers describe the metadata of the sections, and the sections contain actual program data and code.

A symbol's value represents an address within a section in its object file. If you wish to have a symbol represent a constant value, you must write the constant value in a section and have a symbol point to it. This concept of symbols = addresses allows for relocatable code to be written, as symbol values are offset whenever the relevant section is moved.

You may notice that the presence of distinct program sections is overkill for making a directly executable image. However, this format is designed with the future in mind, and could theoretically be loadable by an operating system.

This file format is always big-endian, as Simulatron is big-endian.

Note that due to the use of 32-bit addresses, the maximum size of any object code file or linked combination of object code files is 4 GiB. This is also the amount of memory addressable by Simulatron, so you really shouldn't be making executables that big.

## Header Format
| Offset | Size    | Description                                |
| ------:| ------- | ------------------------------------------ |
|   0x00 | 6 bytes | Magic header, equal to `SIMOBJ`.           |
|   0x06 | 2 bytes | ABI version identifier, equal to `0x0001`. |
|   0x08 | 4 bytes | Number of entries in the symbol table.     |
|   0x0C | 4 bytes | Number of section headers / sections.      |

## Symbol Table Format
The symbol table is a continuous sequence of variable-length entries. Each entry has the following format:

|   Offset | Size        | Description                                                               |
| --------:| ----------- | ------------------------------------------------------------------------- |
|     0x00 | 1 byte      | Symbol type.                                                              |
|     0x01 | 4 bytes     | Symbol value (optional).                                                  |
|     0x05 | 1 byte      | Symbol name length (`L`).                                                 |
|     0x06 | `L` bytes   | Symbol name.                                                              |
| `L`+0x06 | 4 bytes     | Number of references to the symbol in this file (`N`).                    |
| `L`+0x0A | `4*N` bytes | List of offsets in this file that need replacing with the symbol's value. |

The symbol type takes one of the following values:

| Value | Meaning                                                                                                             |
| -----:| ------------------------------------------------------------------------------------------------------------------- |
|  `I`  | Internal: the symbol is defined in this file and is private to it. The symbol value must be present.                |
|  `P`  | Public: the symbol is defined in this file and is public (can be linked against). The symbol value must be present. |
|  `E`  | External: the symbol is defined in another file. The symbol value is ignored.                                       |

If an external symbol cannot be found, linking will fail. The linking process means replacing the listed offsets with the value of the symbol. As a symbol's value is an address, values are always 4 bytes.

A symbol's name is a non-null string of characters encoded in the Simulatron character set. Valid characters are alphanumeric or an underscore.

## Section Header Format
| Offset | Size    | Description            |
| ------:| ------- | ---------------------- |
|   0x00 | 1 byte  | Flags.                 |
|   0x01 | 4 bytes | Length of the section. |

There are four flags within the flag byte: `E`, `R`, `W`, and `X`. `E` specifies that this section is the entry point of the program, while the others are set to enable read, write, and execute permissions. The eagle-eyed reader will notice that the permissions bits match the lower byte of a page table entry (see [Memory Management](memory-management.md)):
```
_________________________
|7 |6 |5 |4 |3 |2 |1 |0 |
|        |X |W |R |  |E |
_________________________
```

There must be exactly one section with the `E` flag set (amongst all files linked together), and this section must also have the `X` flag set. Note that the effect of the permission bits is up to whatever is consuming the object code, and they may be ignored.

## Section Format
A section is arbitrary binary code, that may be instructions and/or data for Simulatron. Any location where a symbol value is to be inserted must be left as zero; this helps detect malformed code in some cases.
