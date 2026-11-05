# Fixed Themes and a Theme Picker — Design

Builds on `2026-11-05-matugen-theme-design.md` (shipped as Default + Album in
`3553ec8`, issue #83). Same branch, `feature/theme-colours`.

## Intent

Default and Album only recolour accents. The maintainer also wants named, fixed
themes — Catppuccin, Nord, a Cyberpunk palette of their own — that make the
whole window look like the theme, plus the ability to drop in their own theme
files. With a dozen-plus choices, stepping through them one at a time in
Settings stops working, so the Theme row gains a picker.

## Decided by the maintainer

- **Built-in set (10):** Catppuccin Latte, Frappé, Macchiato, Mocha; Nord;
  Gruvbox Dark; Tokyo Night; Dracula; Rosé Pine; Cyberpunk. Plus the existing
  Default and Album — 12 entries.
- **Fixed themes paint the whole window, background included.** Default and
  Album keep leaving the terminal's background and body text alone, exactly as
  shipped.
- **Theme file format is Catppuccin's role vocabulary.** A JSON object with a
  `name` and a `colors` map keyed by Catppuccin role names. Every built-in is
  expressed in it; the maintainer's Cyberpunk values are, verbatim:
  ```json
  {
    "name": "Cyberpunk",
    "colors": {
      "base": "#101014", "mantle": "#0a0a0d", "crust": "#050506",
      "text": "#00f0ff", "subtext0": "#d0006f", "subtext1": "#ff007f",
      "surface0": "#1a1a24", "surface1": "#262636", "surface2": "#333348",
      "overlay0": "#4d4d6c", "overlay1": "#666690", "overlay2": "#8080b3",
      "blue": "#00f0ff", "sapphire": "#00a8ff", "peach": "#ffb800",
      "green": "#00ff66", "red": "#ff0055", "mauve": "#e600ff",
      "pink": "#ff007f", "yellow": "#ffea00", "maroon": "#ff0055",
      "teal": "#00f0ff"
    }
  }
  ```
  That is the 22-key set every theme file carries.
- **Users add their own** as `.json` files in `~/.config/riptide/themes/`
  (`dirs::config_dir()/riptide/themes`).
- **The picker is its own filterable overlay** (approved mockup "B"), like the
  command palette: Enter on the Theme row opens it; typing narrows the list;
  built-ins first, then a "yours" heading listing user files.
- **Live preview:** moving the cursor repaints the app in the highlighted theme;
  Enter keeps it; Esc restores what was active when the picker opened.

## Role mapping (approved as proposed)

One function maps a theme file onto Riptide's palette. Fixed themes fill every
field; Default and Album leave the "paint" fields empty.

| Riptide role                          | Catppuccin role |
|---------------------------------------|-----------------|
| window background (new)               | `base`          |
| body text (new; today `Color::White`) | `text`          |
| `dim` — secondary text                | `subtext0`      |
| borders, separators (new split from `dim`, see below) | `overlay0` |
| `accent` — tabs, badges, progress, visualizer, focused borders, key hints | `mauve` |
| `highlight_bg` — selection bar        | `surface1`      |
| `highlight_fg`                        | `text`          |
| `highlight_dim`                       | `subtext1`      |
| `highlight_accent`                    | `mauve`         |
| `select_bg` / `select_fg` (overlays)  | `mauve` / `base` |
| error (today `Color::Red`)            | `red`           |
| success (today `Color::Green`)        | `green`         |

Notes:
- Today `dim` serves both secondary text and borders. Cyberpunk's magenta
  `subtext0` is right for the second line of a row and wrong for every border,
  so the palette gains a separate `border` role. In Default and Album, `border`
  equals `dim`, so nothing they draw changes.
- Error and success come from the theme in fixed themes only. In Default and
  Album they stay `Color::Red` / `Color::Green`, as shipped.
- Unused keys (`mantle`, `crust`, `surface0`, `surface2`, `overlay1/2`,
  `blue`, `sapphire`, `peach`, `pink`, `yellow`, `maroon`, `teal`) are required
  anyway (my call): the format is the full Catppuccin set so a theme file can be
  copied from any Catppuccin port, and future roles (e.g. a warning colour) can
  use them without a format change.

## Behaviour

- **Theme row.** Shows the active theme's name. Enter opens the picker.
  Left/Right on the row still step through the list without opening it (my
  call — keeps the old muscle memory and the existing `step_choice` path). They
  step through the full list, user themes included, as of the last read, which
  is startup or the last time the picker opened. So the list lives on `App`, not
  only in the picker's state.
- **Picker over Settings** (my call). The picker stacks on top of the Settings
  modal; Settings stays drawn underneath, so the preview shows on it too. Enter
  keeps the theme and Esc restores the original, and both return to Settings,
  closing only the picker. Esc in Settings then closes Settings, as today.
- **Picker.** Filter is case-insensitive substring on the name (same as the
  command palette's matching). Up/Down move; Enter keeps; Esc restores. Groups:
  "built in" (Default, Album, then the ten in the order above), then
  "yours  ~/.config/riptide/themes" (alphabetical by `name`). An empty group is
  not drawn.
- **Reading user files.** The themes directory is read each time the picker
  opens (my call) — a file just saved shows up without a restart, and there is
  no watcher. At startup it is read once, to resolve the saved theme.
- **Names are matched case-insensitively** (my call): saved-name lookup,
  collisions and duplicates all compare names folded to lowercase. That is what
  lets the `"default"` / `"album"` already in configs keep resolving.
- **"Default" and "Album" are reserved** (my call). A user file with either
  name is skipped and reported like an invalid file. Those two are modes, not
  palettes, and letting a file replace them would break the anti-goal "Default
  or Album changing in any way".
- **Name collisions.** A user file whose `name` matches a fixed built-in's replaces
  that built-in in the list (my call) — it is how someone tweaks Mocha. Two user
  files with the same name: the alphabetically first filename wins and the other
  is reported as skipped.
- **Invalid files.** All-or-nothing, as in the matugen design: a file that is
  not valid JSON, lacks `name`, or lacks or mis-formats any of the 22 colours
  (`#rrggbb`) is left out. One `warn!` per file naming it and the problem. The
  picker shows a dim footer line "N theme file(s) skipped — see log" when N > 0
  (my call).
- **Persistence.** `Preferences.theme` stores the theme's name as a string:
  `"default"`, `"album"`, or a fixed theme's `name`. Configs written by the
  previous commit already store `"default"` / `"album"` lowercase and must load
  unchanged. A saved name that no longer resolves → Default, with one status
  message at startup ("Theme <name> not found — using Default").
- **Painting the window.** With a fixed theme active, every cell Riptide draws
  has the theme's background, including overlays (which call `Clear`, resetting
  cells to the terminal default — those areas must be repainted) and the padding
  `layout_row` adds. Cover art is unchanged; a transparent image (the
  `DEFAULT_ART` logo) showing the terminal colour through it is acceptable.
- **Artist bio text** (the one stray `Color::Rgb(180, 180, 180)` in
  `artist_detail.rs`) uses `subtext1` in fixed themes (my call). In Default
  and Album it stays `Rgb(180, 180, 180)`. Left alone, it would be grey on a
  themed background.
- **Body text.** The ~30 `Color::White` sites in `src/ui` become the palette's
  body-text role. In Default/Album that role is `Color::White`, so they render
  as today.

## Constraints

- Everything from the first design's constraints still holds: Default renders
  exactly as before; themed colours are `Color::Rgb`; comments explain why;
  AGENTS.md logging rules; no commits until the maintainer has tested.
- Built-in theme files live in the repo (e.g. `assets/themes/*.json`) and are
  compiled in with `include_str!` — packages do not install `assets/`, and the
  Nix flake already copies the directory for the existing `include_bytes!`.
  They are parsed by the same code as user files, and a test asserts all ten
  parse.
- Built-in palettes other than Cyberpunk are transcribed from each project's
  published palette into the Catppuccin role set. Catppuccin's four are exact.
  For the others, the builder assigns each source colour to the nearest role;
  credit and license (all MIT or similar) go in `THIRD_PARTY_NOTICES.md`, since
  these are adapted designs, which is what that file records.
- The picker reuses the command palette's overlay shape and matching rather
  than inventing a new widget.
- File an issue for this work and put it on the board before coding (maintainer
  rule); reference it in the CHANGELOG entry beside #83.

## Non-goals

- Editing themes inside Riptide.
- Watching the themes directory while the picker is closed.
- Light/dark switching for fixed themes (each theme is what it is; Latte is the
  light Catppuccin).
- Fixed themes recolouring cover art, or Album mode painting the background.
- Any format other than the Catppuccin role JSON (no TOML, no base16 import).

## Anti-goals

- A fixed theme leaving patches of terminal background — around overlays, row
  padding, the visualizer — so it looks half-applied.
- Esc in the picker leaving the previewed theme in place.
- A broken user file crashing Riptide or silently vanishing with no clue why.
- Default or Album changing in any way.

## Left to the builder

Exact data structures (enum vs. string for the active theme internally);
where the picker state lives; how the background fill is done (a full-frame
fill at the top of `draw` plus repaint after `Clear`, or a wrapper); the
nearest-role transcription of the non-Catppuccin palettes; picker sizing.
