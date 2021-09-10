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
TODO

## Build Prerequisites
For `web-view` to build, various other things may need to be installed. Running on Kubuntu 20.04 LTS, the following additional packages were required:
* `libcairo2-dev`
* `libpango1.0-dev`
* `libatk1.0-dev`
* `libsoup2.4-dev`
* `libgdk-pixbuf2.0-dev`
* `libgtk-3-dev` (this one required a bit of googling; the error wasn't so obvious)
* `libwebkit2gtk-4.0-dev` (only this one was mentioned in the official docs)

All in one command:
```
sudo apt install libcairo2-dev libpango1.0-dev libatk1.0-dev libsoup2.4-dev libgdk-pixbuf2.0-dev libgtk-3-dev libwebkit2gtk-4.0-dev
```


These were all determined from build errors such as:
```
error: failed to run custom build command for `atk-sys v0.9.1`

Caused by:
  process didn't exit successfully: `/home/chris/repos/simulatron-v2/simulatron/target/debug/build/atk-sys-a278f11f7cd4abf8/build-script-build` (exit code: 1)
--- stderr
`"pkg-config" "--libs" "--cflags" "atk" "atk >= 2.14"` did not exit successfully: exit code: 1
--- stderr
Package atk was not found in the pkg-config search path.
Perhaps you should add the directory containing `atk.pc'
to the PKG_CONFIG_PATH environment variable
No package 'atk' found
Package atk was not found in the pkg-config search path.
Perhaps you should add the directory containing `atk.pc'
to the PKG_CONFIG_PATH environment variable
No package 'atk' found
```
So if you see one of those, try installing a similarly named dev package.
