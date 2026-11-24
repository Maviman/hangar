# HANGAR // Design System v0.1

Original visual language for the HANGAR launcher. Inspired by the *feel* of
high-contrast sci-fi game UI (stark flat color blocks, acid accents, technical
labeling, registration marks). It uses no third-party logos, artwork, or
proprietary typefaces. All shapes are plain egui painter primitives
(polygons, circles, line segments, dashed lines, text galleys).

## 1. Tokens

### Color
| Token   | Hex       | Role |
|---------|-----------|------|
| `VOID`  | `#0A0B0D` | App background |
| `PANEL` | `#121418` | Bars, bubble fill, menus |
| `RAISED`| `#1A1D22` | Hovered surfaces, inputs |
| `GRID`  | `#2A2F37` | Minor dot grid |
| `LINE`  | `#30363E` | Hairlines, idle bubble stroke |
| `ASH`   | `#8A9099` | Secondary text, codes |
| `BONE`  | `#ECE8DD` | Primary text, neutral accent |
| `ACID`  | `#D4FF1F` | Primary accent, focus, selection |
| `FLARE` | `#FF2E88` | Cluster accent |
| `EMBER` | `#FF6B1A` | Cluster accent, **danger / missing target** |
| `ION`   | `#2EF2FF` | Cluster accent |
| `UV`    | `#9D6BFF` | Cluster accent |

Cluster accent rotation: `ACID → FLARE → ION → EMBER → UV → BONE`.
Text on any accent fill is always `VOID` (contrast ≥ 9:1 on all six).

### Type
| Token        | Face                       | Size (pt @1x zoom) | Use |
|--------------|----------------------------|--------------------|-----|
| `display.lg` | Oxanium Bold (OFL)         | 20 | Cluster plates |
| `display.sm` | Oxanium Bold               | 16 | Wordmark, dialogs |
| `mono.md`    | JetBrains Mono Medium (OFL)| 12 | App labels, buttons |
| `mono.sm`    | JetBrains Mono Medium      | 10 | Codes, type tags, status bar |

All labels are UPPERCASE with +1px letter spacing (mono.sm: +1.5px).
Canvas text scales with zoom and is hidden below 6pt effective size.

### Geometry
- **Spacing scale:** 4 / 8 / 12 / 16 / 24 / 32.
- **Corner radius:** 0 everywhere. Corners are *chamfered* (45° cut) instead:
  `chamfer.sm = 4`, `chamfer.md = 8`, `chamfer.lg = 12`.
- **Stroke:** hairline 1.0, standard 1.5, focus 2.0.
- **Bubble radius:** 26 world units. Plate height: 34 world units.
- **Grid:** dot every 40 units; `+` registration mark every 200 units.

### Motion
- Physics settles with velocity damping 0.82/step; rendering stops when settled
  (zero idle CPU).
- Hover: orbit ticks rotate at 0.6 rad/s (only while hovered).
- Launch flash: 350 ms ring expansion in accent color.
- Toasts: 2.6 s, no fade-in, 300 ms fade-out.

## 2. Components

### Cluster plate
Chamfered rectangle (`chamfer.md` on top-left and bottom-right only), filled
with the cluster accent, `display.lg` name in `VOID`. A code tag sits under it
in `mono.sm`/`ASH`: `CL-03 // 05 NODES`. A dashed orbit ring (accent @ 22%)
encloses its bubbles.
States: idle · hover (2px `BONE` outline offset 3px) · drag (plate lifts, orbit
ring solid) · drop-target (orbit ring solid `ACID`, filled @ 6%) · renaming
(inline mono input replaces label).

### App bubble
Circle, `PANEL` fill, 1.5px `LINE` stroke, 32px OS icon centered (or a 2-letter
monogram in `display.sm` when no icon). Label below in `mono.md`/`BONE`
(truncated at 22 chars), type tag below that in `mono.sm`/`ASH` (`EXE`, `LNK`,
`HTML`, `BAT`, `URL`, `DIR`, `FILE`).
States: idle · hover (stroke → cluster accent 2px, four orbit ticks rotate) ·
search match (accent stroke, others dim to 25%) · missing target (dashed
`EMBER` stroke, tag `// NOT FOUND`) · launching (accent ring flash).

### Link edge
Straight 1px line, cluster accent @ 35%, from plate center to bubble rim,
terminated by a 4×4 square at the rim.

### Toolbar (top, 40px)
`PANEL` fill, 1px `LINE` bottom rule. Left: wordmark `HANGAR` (`display.sm`
`BONE`) + `ACID` slash block + `mono.sm` subtitle. Center: search input.
Right: `+ CLUSTER`, `FIT`, `?` buttons.
Button: outlined chamfer.sm, `BONE` text; hover fills `ACID` with `VOID` text.

### Status bar (bottom, 24px)
`mono.sm`/`ASH`, segments separated by ` // `.

### Context menu
`PANEL` fill, 1px `LINE` border, square corners; hover row fills `ACID`,
text `VOID`. Destructive item text `EMBER` and requires a second click
("CONFIRM DELETE").

### Rename field
Single-line input, `RAISED` fill, 2px `ACID` stroke, mono text. Enter commits,
Esc cancels, click-away commits.

### Toast
Bottom-right, `PANEL` plate with 4px accent bar on the left, `mono.sm` text.
Accent `ACID` for info, `EMBER` for errors.

### Drop overlay
While files hover the window: `ACID` 2px inset frame, hazard-stripe corners,
centered plate `DROP TO DOCK // N FILES`.
