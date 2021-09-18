# Simulatron Example Programs
This directory contains a number of example programs written in Simulatron Assembly.

## Initial Setup
Before you can run any example, you should follow these steps:

1. Ensure you have [an up-to-date Rust toolchain](https://www.rust-lang.org/learn/get-started).
2. Install the core tools (`simulatron`, `salt`, `silk`) by navigating to each package (e.g. [/Crates/simulatron-vm/](../Crates/simulatron-vm)) and running `cargo install --path .`. If you wish, you may run each test suite first (`cargo test`). If you have problems with `simulatron-vm` due to out-of-memory errors, enable the lazy ram feature (`<cargo command> --features lazy-ram`).
3. Navigate back to this (`Examples`) directory.
4. Set up the Simulatron directory structure: `simulatron --init`.
5. Compile the ROM: `salt -E ROM.simasm && silk -t rom -o ROM ROM.simobj`.
6. Copy the compiled ROM into the new `./simulatron` directory, overwriting the placeholder.

## Running An Example
To compile and run an example, follow these steps:

1. Enter the directory of a specific example.
2. Run `salt *.simasm -E main.simasm` to compile all the assembly files.
3. Run `silk -t disk -o prog.simdisk *.simobj` to link the object code into a disk image.
4. Copy the resulting disk image into `DiskA` of your Simulatron.
5. Start the VM by running `simulatron` from its directory.
