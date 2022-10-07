# Simulatron Display
### Version 2.0.0

The Simulatron is attached to a simple terminal display. The terminal has 25 rows and 80 columns; each cell can display a single character, with a configurable foreground and background colour. Both columns and rows are zero-indexed.

The controls are mapped directly into memory; to set the character, foreground colour, or background colour for a specific cell, a value should be written to the corresponding memory location. See the memory management docs for the exact ranges.

Each of the three configurable parameters has a 2000-byte block allocated. If the index within this block is `i`, then this represents row `i / 80` and column `i % 80`, i.e. the cells are represented left to right, then top to bottom.

To set the character, write the representation of that character as in the character set docs.

To set a colour, write it in the following RGB format: `0b00RRGGBB`. From high to low, the first bit pair is ignored, then the remaining bit pairs represent R, G, and B. This allows each R, G, or B value to be one of (0, 85, 170, 255) from 0b00 to 0b11.

### Examples
* **Action**: Write 0x21 to byte 432 of the character range.
* **Processing**: 0x21 is '!'; 432 is row 5, column 32.
* **Result**: '!' appears in row 5, column 32.


* **Action**: Write 0b00111100 to byte 0 of the background colour range.
* **Processing**: 0b00111100 is RGB(255, 255, 0); 0 is row 0, column 0.
* **Result**: The background of the top-left cell turns yellow.
