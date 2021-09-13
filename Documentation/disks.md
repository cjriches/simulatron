# Disks
### Version 2.0.0-beta

## Overview
Simulatron has two identical disk controllers, A and B. It expects to find two corresponding directories, `./DiskA/` and `./DiskB/`, in the working directory. If either of these does not exist, Simulatron will fail to start. The rest of this document references the controllers in the singular, applying identically to both.

The disk directory acts like a slot for a removable disk file. If a single file exists within the disk directory, it will be mounted by Simulatron. Otherwise, the disk is considered disconnected.

A disk file is interpreted as raw binary, readable and writable by Simulatron in 4096-byte blocks. A disk file must be a non-zero multiple of 4096 bytes to be recognised. The maximum disk file size is 16 TiB (17,592,186,044,416 bytes), as Simulatron can address a 32-bit space of 4096-byte blocks.

The following table summarises all the memory mappings for a disk controller. For the addresses that they are mapped to, see [Memory Management](memory-management.md).

| Description      | Size       | Read/Write |
| ---------------- | ---------- | ---------- |
| Status           | 1 byte     | Read       |
| Data Buffer      | 4096 bytes | Read/Write |
| Blocks available | 4 bytes    | Read       |
| Block address    | 4 bytes    | Read/Write |
| Command register | 1 byte     | Write      |

## Disk Change Interrupts
The disk controller will send an interrupt to the CPU when it detects a change in disk: either a removal, addition, or replacement. Since the disk controller performs this check on boot to set the initial state, you can expect to instantly receive an interrupt the first time you enable disk interrupts.

Beware of swapping a disk file too quickly. If you replace a file so fast that there is no observed disconnected state in-between, you will likely trigger incorrect behaviour in any program that interacts with the disk as it may not notice the change. Of course, the severity and time-sensitivity depends on the nature of the program running, but a simple second gap is a wise precaution.

## Disk Status
The disk controller reports its status through a single memory-mapped byte. The bits are laid out as follows:

```
_________________________
|7 |6 |5 |4 |3 |2 |1 |0 |
|  RESERVED |B |S |F |C |
_________________________
```

| Name          | Description                             |
| ------------- | --------------------------------------- | 
| C(onnected)   | Set if there is a disk present.         |
| F(inished)    | Flipped every time a command finishes.  |
| S(uccess)     | Set if the last command was successful. |
| B(ad command) | Set if the last command was invalid.    |

Assuming only one disk command is "in flight" at a time, the `F` bit allows you to track its completion. If a command has finished, but neither `S` nor `B` is set, then this implies there was an IO error.

All flags are zero upon boot, but if a disk is present, the `C` bit will rapidly become set. You can safely assume that every time the disk status byte is updated, an interrupt will be sent to the CPU.

## Data Buffer
A single page of memory is mapped into the disk controller's buffer; this is the buffer that the controller will read into or write from. A page is 4096 bytes, so this directly corresponds to a single disk block.

## Blocks Available
These four bytes report the size of a connected disk as a number of blocks. If no disk is connected, then blocks available will be zero.

## Block Address
This is a pointer to a block on disk. The valid blocks are zero-indexed, from `0` to `blocks_available - 1` inclusive.

## Command Register
Writing to this byte causes the disk controller to act. The following commands are available:

| Name             | Value |
| ---------------- |:-----:| 
| Read             | 0x01  |
| Write            | 0x02  |
| Contiguous Read  | 0x03  |
| Contiguous Write | 0x04  |

A read command will read from the disk block pointed to by the block address, placing the result in the data buffer. A write command will write the contents of the data buffer to the disk block pointed to by the block address. The contiguous variants do the same, but additionally increment the block address by one after the operation (successfully) completes.

Any malformed operation, such as an invalid command number, or having `block_address >= blocks_available` will cause the command to fail with the `B` status flag set.
