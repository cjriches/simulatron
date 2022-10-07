# salt: Simulatron Assembly Language Translator
### Version 2.0.0

## Overview
Salt transforms assembly language files (`*.simasm`) into object code files (`*.simobj`). Object code files can then be linked into different forms of executable by [Silk](../simulatron-silk/README.md).

For a definition of the assembly language, see [Assembly Language](../../Documentation/assembly-language.md).

## Implementation-Specific Behaviour
The SimAsm assembly specification leaves a few things as implementation-defined: the behaviour of public constants, and the selection of an entrypoint.

Salt can compile multiple assembly files in a single invocation, and any public constants will be visible in all files.

Files specified with the `-E` option will be compiled as entrypoints, starting at the first instruction in the file. Remember that when linking, you must have exactly one entrypoint section amongst the object code files being linked.

## Usage
`salt --help`
