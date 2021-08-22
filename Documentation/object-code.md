# Simulatron Object Code Definition
### Version 2.0.0-alpha

## Overview
A single object code file (`*.simobj`) contains four things:
1. One SIMOBJ header.
2. One symbol table.
3. X section headers.
4. X sections.

The header describes overall metadata and the layout of the other elements. The symbol table describes symbols that are spliced in to the sections at link time, allowing code to be relocated and multiple files to be linked together. The section headers describe the metadata of the sections, and the sections contain actual program data and code.

You may notice that the presence of distinct program sections is overkill for making a directly executable image. However, this format is designed with the future in mind, and could theoretically be loadable by an operating system.

## Header Format
| Offset | Size    | Description                                  |
| ------:| ------- | -------------------------------------------- |
|   0x00 | 6 bytes | Magic header, equal to `SIMOBJ`.             |
|   0x06 | 2 bytes | ABI version identifier, equal to `0x0001`.   |
|   0x08 | 4 bytes | Pointer to the start of the symbol table.    |
|   0x0C | 4 bytes | Number of entries in the symbol table.       |
|   0x10 | 4 bytes | Pointer to the start of the section headers. |
|   0x14 | 1 byte  | Number of sections / section headers.        |
|   0x15 | 3 bytes | Zero padding.                                |
|   0x18 | N/A     | End of header (size).                        |

## Symbol Table Format
The symbol table is a continuous sequence of variable-length entries. Each entry has the following format:

|   Offset | Size        | Description                                                                 |
| --------:| ----------- | --------------------------------------------------------------------------- |
|     0x00 | 1 byte      | Symbol type.                                                                |
|     0x01 | 4 bytes     | Symbol value (optional).                                                    |
|     0x05 | 1 byte      | Symbol name length (`L`).                                                   |
|     0x06 | `L` bytes   | Symbol name.                                                                |
| `L`+0x06 | 4 bytes     | Number of references to the symbol in this file (`N`).                      |
| `L`+0x0A | `4*N` bytes | List of offsets in this file that need replacing with the symbol's address. |

The symbol type takes one of the following values:

| Value | Meaning                                                                                                             |
| -----:| ------------------------------------------------------------------------------------------------------------------- |
|  `I`  | Internal: the symbol is defined in this file and is private to it. The symbol value must be present.                |
|  `P`  | Public: the symbol is defined in this file and is public (can be linked against). The symbol value must be present. |
|  `E`  | External: the symbol is defined in another file. The symbol value is ignored.                                       |

If an external symbol cannot be found, linking will fail. The linking process means replacing the listed offsets with the value of the symbol. As a symbol's value is its address, values are always 4 bytes.

## Section Header Format
| Offset | Size    | Description                             |
| ------:| ------- | --------------------------------------- |
|   0x00 | 1 byte  | Flags.                                  |
|   0x01 | 3 bytes | Zero padding.                           |
|   0x04 | 4 bytes | Pointer to the section within the file. |
|   0x08 | 4 bytes | Length of the section.                  |

There are four flags within the flag byte: `E`, `R`, `W`, and `X`. `E` specifies that this section is the entry point of the program, while the others are set to enable read, write, and execute permissions. The eagle-eyed reader will notice that the permissions bits match the lower byte of a page table entry (see [Memory Management](memory-management.md)):
```
_________________________
|7 |6 |5 |4 |3 |2 |1 |0 |
|        |X |W |R |  |E |
_________________________
```

There must be only one section with the `E` flag set, and this section must also have the `X` flag set. Note that the effect of the permission bits is up to whatever is consuming the object code, and they may be ignored.

## Section Format
A section is arbitrary binary code, that may be instructions and/or data for Simulatron. Any location where a symbol value is to be inserted must be left as zero; this helps detect malformed code in some cases.
