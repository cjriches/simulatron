# Simulatron Repository
The Simulatron project consists of a CPU architecture definition, a virtual machine emulating this architecture, and a compilation toolchain for this architecture. Its purpose is to aid understanding of hardware and low-level software, and to be fun to play around with.

This is the root repository for Simulatron and its associated tools. This project currently consists of the following packages:
* `simulatron-vm` - the virtual machine itself.
* `simulatron-salt` - an assembler for Simulatron.
* `simulatron-silk` - a linker for Simulatron.
* `simulatron-utils` - utilities that are shared or don't fit anywhere else.

General documentation can be found in [Documentation](Documentation). Package-specific information can be found in each package's `README.md`.

## Features
* 32-bit architecture.
* Memory management and virtual-to-physical address translation.
* User/Kernel modes.
* Integer and floating point computation.
* Built-in peripherals: console, keyboard, and two removable disks.
* Assembly language and object code format specification, with an assembler and linker.
* Cross-platform support: Simulatron runs on Linux and Windows, and should run on Mac too (although this is untested).

## Getting Started
See the [examples](Examples).

## Project State
The project is currently paused.

The VM, linker, and assembler are feature-complete. The next goal is to create a simple operating system that can run on Simulatron, but I don't fancy doing that in assembly. I intend to make a higher-level language first.

## History
Simulatron V1 was a similar project smaller in scope, with a 16-bit CPU design that lacked memory management or privilege modes. It was never made open-source. Simulatron V2 is a from-scratch redesign and rewrite, designed to be more powerful, more performant, and capable of supporting an operating system. Indeed, the long-term goal is to create an operating system that runs on Simulatron.

## Contributing
As Simulatron is a hobby project undertaken for my own learning, I neither expect nor desire any other contributors. However, feel free to hack the code locally. The project is licensed under MIT; see [the full license](LICENSE).
