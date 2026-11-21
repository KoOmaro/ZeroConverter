# ZeroFoundry steel letter mark

Every ZeroFoundry product gets one steel letter on a graphite tile. ZeroConverter
is "C". `steel_mark.py` builds the mark from one formula, so a new
letter matches the others side by side and when the marks are laid on top of each other.

```bash
python branding/steel_mark.py C out/        # out/c-icon.svg (app icon), out/c-mark.svg (in-app)
```

## Grid

- Glyph space is the 52-unit square `6..58`.
- Letters fit inside the box `x 8..56, y 10..54` and never go past it.
- Stroke width is 12–13 units.
- Outer corners are chamfered by 6, stroke ends by 4.
- Diagonals are 45° or follow the letter's own diagonal.
- Inner corners stay sharp.
- Outline points run clockwise, y pointing down.

## Material (fixed, the same for every letter)

| Part | Value |
|---|---|
| Bevel | 1.6 units, colour picked from the edge's outward direction (8-point table, light from the top left) |
| Face metal | The studio stops, applied along the **glyph box diagonal** `(8,10) → (56,54)`. The grey sheen band crosses the letter instead of collecting on one stem |
| Highlight | Hairline on face edges that face up or left |
| Contour | `#24343F`, 0.5 units |
| Finish | `brushed` for the app icon, `satin` for the in-app title bar |
| Tile | 120 units, radius 25, `#303840 → #12171C`, edge `#5B6873`, no top highlight line |

## Adding a letter

1. Add its outline to `GLYPHS`, following the grid rules.
2. Generate it and compare it with the existing letters at 256 px, 32 px and 16 px, side by side and laid on top.
3. Generate the app icons: `npx tauri icon <letter>-icon.svg`.

Run with the `Z` outline, the formula reproduces the studio's original mark exactly. The
face metal differs only in the sheen axis.
