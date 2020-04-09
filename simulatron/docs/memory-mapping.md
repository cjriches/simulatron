# Memory Mapping
### Version 1.0

The Simulatron has a 32-bit virtual and physical address space. When in kernel mode, the virtual addressing is bypassed, and physical addresses are accessed directly. When in user mode, addresses undergo virtual->physical translation first.

The frame size is 4096 bytes.

Physical addresses are mapped as follows. All ranges are inclusive.

|     Addresses    |      Mapping
| ----------------:| -----------------
|             0-31 | Interrupt Handlers (32 bytes)
|            32-63 | Reserved (32 bytes)
|           64-575 | ROM (512 bytes)
|         576-2575 | Display Characters (2000 bytes)
|        2576-4575 | Display Foreground Colours (2000 bytes)
|        4576-6575 | Display Background Colours (2000 bytes)
|             6576 | Keyboard Key Buffer (1 byte)
|             6577 | Keyboard Metadata Buffer (1 byte)
|        6578-8171 | Reserved (1594 bytes)
|             8172 | Disk A status (1 byte)
|        8173-8176 | Disk A blocks available (4 bytes)
|        8177-8180 | Disk A block address (4 bytes)
|             8181 | Disk A command register (1 byte)
|             8182 | Disk B status (1 byte)
|        8183-8186 | Disk B blocks available (4 bytes)
|        8187-8190 | Disk B block address (4 bytes)
|             8191 | Disk B command register (1 byte)
|       8192-12287 | Disk A data (4096 bytes)
|      12288-16383 | Disk B data (4096 bytes)
| 16384-4294967295 | RAM (4,294,950,912 bytes = just under 4 GiB)

Note that the first two frames are taken up by various mappings, frames 3 and 4 are Disk A and B data respectively, and all remaining frames are RAM.
