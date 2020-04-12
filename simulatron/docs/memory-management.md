# Memory Management
### Version 1.0
The Simulatron has a 32-bit virtual and physical address space. When in kernel mode, the virtual addressing is bypassed, and physical addresses are accessed directly. When in user mode, addresses undergo virtual->physical translation first.

The page/frame size is 4096 bytes.

## Physical address mapping
The Simulatron uses memory-mapped IO, so various devices can be accessed within the physical address space.

Physical addresses are mapped as follows. All ranges are inclusive.

|     Addresses    |                   Mapping                    | Read/Write |
| ----------------:| -------------------------------------------- | ---------- |
|             0-31 | Interrupt Handlers (32 bytes)                | Both       |
|            32-63 | Reserved (32 bytes)                          | Neither    |
|           64-575 | ROM (512 bytes)                              | Read       |
|         576-2575 | Display Characters (2000 bytes)              | Write      |
|        2576-4575 | Display Foreground Colours (2000 bytes)      | Write      |
|        4576-6575 | Display Background Colours (2000 bytes)      | Write      |
|             6576 | Keyboard Key Buffer (1 byte)                 | Read       |
|             6577 | Keyboard Metadata Buffer (1 byte)            | Read       |
|        6578-8171 | Reserved (1594 bytes)                        | Neither    |
|             8172 | Disk A status (1 byte)                       | Read       |
|        8173-8176 | Disk A blocks available (4 bytes)            | Read       |
|        8177-8180 | Disk A block address (4 bytes)               | Write      |
|             8181 | Disk A command register (1 byte)             | Write      |
|             8182 | Disk B status (1 byte)                       | Read       |
|        8183-8186 | Disk B blocks available (4 bytes)            | Read       |
|        8187-8190 | Disk B block address (4 bytes)               | Write      |
|             8191 | Disk B command register (1 byte)             | Write      |
|       8192-12287 | Disk A data (4096 bytes)                     | Both       |
|      12288-16383 | Disk B data (4096 bytes)                     | Both       |
| 16384-4294967295 | RAM (4,294,950,912 bytes = just under 4 GiB) | Both       |

Note that frames 0 and 1 are taken up by various mappings, frames 2 and 3 are Disk A and B data respectively, and all remaining frames are RAM.

If an access of the wrong type is made (e.g. a write to a read-only section), then an illegal operation interrupt will be sent to the CPU.

## Virtual memory system
Virtual memory mapping is achieved via two-level hierarchical page table. The kernel should set the Page Directory Pointer Register to point to a valid page table in memory before entering user mode.

##### Page directory format
The Page Directory may be located anywhere in memory and is pointed to by the PDPR. It consists of 1024 entries, each 32 bytes in size. Each entry has the following structure:

```
_________________________________________________________________________________________________
|31|30|29|28|27|26|25|24|23|22|21|20|19|18|17|16|15|14|13|12|11|10|9 |8 |7 |6 |5 |4 |3 |2 |1 |0 |
|                   Address of Page Table                   |USER-DEF|        RESERVED       |V |
_________________________________________________________________________________________________
```

V stands for Valid. If 0, the linked page table does not exist, and the address is meaningless. Attempting to access an address inside an invalid page table will generate a page fault. Bits 1-8 are reserved for future use and should be set to zero. Bits 9-11 are available for the programmer to use as they wish.

The given address is the upper 20 bits. As a page table must be located precisely within a single frame, the lower 12 bits are considered to all be zero.

##### Page table format
A page table also consists of 1024 32-byte entries. It must be frame-aligned. Each entry has the following structure:

```
_________________________________________________________________________________________________
|31|30|29|28|27|26|25|24|23|22|21|20|19|18|17|16|15|14|13|12|11|10|9 |8 |7 |6 |5 |4 |3 |2 |1 |0 |
|                      Address of Frame                     |USER-DEF|RESERVED|C |E |W |R |P |V |
_________________________________________________________________________________________________
```

V again stands for Valid. If 0, this entry has not been assigned and the address is meaningless; a page fault will be generated.

P stands for Present. If 0, the entry has been assigned, but the page is not present in memory and is not ready to access; a page fault will be generated.

R stands for Read. If 0, attempted reads will generate a page fault.

W stands for Write. If 0, attempted writes will generate a page fault.

E stands for Execute. If 0, attempted instruction fetches will generate a page fault.

C stands for Copy-On-Write. If both W and C are 1, then an attempted write will trigger a page fault.

Bits 6-8 are reserved for future use and should be set to zero. Bits 9-11 are available for the programmer to use as they wish.

Again, the address is the upper 20 bits with lower 12 bits as zero, as the address points to the start of a frame.

##### Page Faults
If a page fault occurs, an interrupt will be sent to the CPU and the Page Fault Status Register will be set with one of the following codes as appropriate:

| Code |               Meaning                 |
| ----:| ------------------------------------- |
|    0 | Invalid page                          |
|    1 | Illegal access (R, W, or E violation) |
|    2 | Page not present                      |
|    3 | Copy-on-write                         |

##### Virtual to Physical translation
The CPU will emit a 32-bit virtual address to the MMU. The first 10 bits specify the page directory entry. This will point to a page table. The second 10 bits specify the page table entry. The 20-bit address in this entry will replace the first 20 bits of the virtual address, resulting in the physical address. Thus, the last 12 bits act as the offset within the page/frame.
