# silk: SImulatron LinKer
### Version 2.0.0-alpha

## Overview
Silk links together one or more object code files (`*.simobj`) into various forms of executable.

For a definition of the object code format, see [Object Code](../Documentation/object-code.md).

## Usage
`silk -t <link_target> -o <out_path> OBJECT_FILES...`

## Link Targets
### Disk Image
Silk can create a disk image when passed `-t 'DISK'`. This produces a Simulatron-executable file padded to a multiple of 4096 bytes that can thus be mounted as a disk. Of course, the ROM must be configured to read the program from the disk into memory and then execute it.

### ROM Image
Silk can create ROM images when passed `-t 'ROM'`. This produces a Simulatron-executable file exactly 512 bytes in size, padding with zero if too small and failing if too large. Since ROM is by definition read-only, any section with the write permission set will also produce an error.

## Build Prerequisites
None; a simple `cargo build` should succeed.