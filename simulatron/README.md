# Simulatron
### Version 2.0.0-alpha

## Overview
Simulatron is a virtual machine with its own custom architecture, designed to aid understanding of hardware and low-level software, and to be fun to play around with.

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
