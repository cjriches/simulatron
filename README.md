# Simulatron Repository
The Simulatron project consists of a CPU architecture definition, a virtual machine emulating this architecture, and a compilation toolchain for this architecture. Its purpose is to aid understanding of hardware and low-level software, and to be fun to play around with.

This is the root repository for Simulatron and its associated tools. This project currently consists of the following packages:
* `simulatron` - the virtual machine itself.
* `simulatron-salt` - an assembler for Simulatron.
* `simulatron-silk` - a linker for Simulatron.

General documentation can be found in [Documentation](Documentation). Package-specific information like usage and build guides can be found in each package's `README.md`.

## Features
* 32-bit architecture.
* Memory management and virtual-to-physical address translation.
* User/Kernel modes.
* Built-in peripherals: console, keyboard, and two removable disks.
* Assembly language and object code format specification, with an assembler and linker.

## History
Simulatron V1 was a similar project smaller in scope, with a 16-bit CPU design that lacked memory management or privilege modes. It was never made open-source. Simulatron V2 is a from-scratch redesign and rewrite, designed to be more powerful, more performant, and capable of supporting an operating system. Indeed, the long-term goal is to create an operating system that runs on Simulatron.

## Project State
Simulatron V2 is currently in alpha while a minimal working system is developed. The VM and linker are feature-complete although not polished, and the assembler is in-progress. The VM compiles and runs on Linux, but does not work properly on Windows. MacOS is untested. Broader OS support is planned at some point in the future.

## Contributing
As Simulatron is a hobby project undertaken for my own learning, I neither expect nor desire any other contributors. However, feel free to hack the code locally. The project is licensed under MIT; see [the full license](LICENSE).
