# Simulatron
### Version 2.0.0-alpha

## Overview
Simulatron is a virtual machine with its own custom architecture, designed to aid understanding of hardware and low-level software, and to be fun to play around with.

## Cargo Features
Simulatron provides two RAM implementations: `ram_eager`, which eagerly allocates the full ~4GiB, and `ram_lazy`, which lazily allocates per-page when written to.
`ram_eager` behaves differently on different platforms due to differences in memory allocation strategy.
On Linux, optimistic memory allocation with demand paging is enabled by default; this means that `ram_eager` will only consume physical memory for each host page when actually written to. Therefore, assuming the host page size is the same as Simulatron's page size (4 KiB), `ram_eager` behaves indistinguishably from `ram_lazy`.
Other platforms do not support this, and `ram_eager` will allocate the full amount up-front.

If you have enough free memory or are running on Linux, it is recommended to use the default of `ram_eager`, as this is marginally more performant.
However, if you do not have enough free memory and the 4GiB allocation is failing, or if you want to minimise Simulatron's memory usage, you can enable the `lazy-ram` cargo feature (`cargo <subcommand> --features lazy-ram`) to switch over to `ram_lazy`.
This has a small performance penalty due to the extra indirection.

## Usage
`simulatron --help`
