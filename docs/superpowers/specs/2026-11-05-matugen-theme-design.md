# Theme: Wallpaper and Album Colours — Design

> **Amended 2026-11-05:** Wallpaper mode was built and then removed before merge.
> Desktop shells that drive matugen run it with their own generated config, so a
> template in `~/.config/matugen/config.toml` is never rendered. See ROADMAP.md,
> "Theme: colours from the desktop wallpaper". What ships is **Default** and **Album**.

## Intent

Riptide is nearly monochrome: one cyan accent (`theme::ACCENT`), grey borders
and secondary text (`theme::DIM`), a fixed blue-grey selection bar, and white
everywhere else. The maintainer runs a matugen-themed desktop (Hyprland, GTK,
fuzzel, quickshell all generated from the wallpaper) and wants Riptide to belong
to it — and, as a second mode, to take its colours from the album that is
playing.

## The setting

A new **Theme** row in the Settings modal, a `SettingValue::Choice` stepped like
Repeat and Visualizer, persisted in `Preferences` (`src/app/state/sort.rs`) with
`#[serde(default)]`:

| Choice      | Colours come from                                                        |
|-------------|--------------------------------------------------------------------------|
| `Default`   | Today's look, unchanged. The default, so an upgrade recolours nothing.   |
| `Wallpaper` | A colours file matugen writes from the desktop wallpaper.                 |
| `Album`     | The cover art of the playing track, computed in-process.                  |

Help line under the row (`Setting::help`) — the builder words it; it should
mention that Wallpaper needs the matugen template (see below).

## Where colour goes (approved: mockup option "C")

Accents and highlights only. **The terminal background is never painted.** The
selection bar is the only background Riptide draws, as today.

| Role (today's constant)                         | Material role in a themed mode          |
|-------------------------------------------------|-----------------------------------------|
| `ACCENT` — active tab, badges, progress, visualizer, focused borders, key hints | `primary`             |
| `DIM` — unfocused borders, secondary text, separators | `outline` (builder may pick `on_surface_variant` if `outline` reads too dark for text) |
| `HIGHLIGHT_BG` — selection bar                  | `primary_container`                     |
| `HIGHLIGHT_FG` — text on the bar                | `on_primary_container`                  |
| `HIGHLIGHT_DIM` — secondary text on the bar     | builder's call; must stay legible on `primary_container` |
| `HIGHLIGHT_ACCENT` — badge on the bar           | `primary`                               |
| `SELECT_BG` / `SELECT_FG`                       | builder's call, consistent with the above |
| `Color::White` body text (29 sites)             | **unchanged** — left to the terminal's foreground |
| Status colours (`Color::Red` errors, `Color::Green`) | **unchanged** — they carry meaning, not decoration |
| Stray greys — `Color::DarkGray` in `overlays.rs` (×2), `Color::Rgb(180,180,180)` in `artist_detail.rs` | fold into the `DIM` role (my call) |

**Light or dark follows the user, not a fixed choice** (decided by the
maintainer):

- **Wallpaper** uses whichever scheme matugen generated — the template reads
  `.default`, which follows matugen's `-m light|dark`.
- **Album** asks the terminal for its background colour at startup (OSC 11;
  the `terminal-colorsaurus` crate or equivalent is the builder's call) and
  picks the light scheme on a light background, dark otherwise. A terminal
  that does not answer within a short timeout gets dark. Query once, before
  the TUI takes over stdin, not per track.

`theme.rs`'s existing note stands: every colour drawn over a fixed background
must be true colour. Themed modes emit `Color::Rgb` throughout. In `Default`
mode the current constants are used exactly, palette entries included.

## Wallpaper mode

Riptide does **not** invoke matugen. It reads a file matugen writes, the same
way the maintainer's Hyprland, fuzzel and GTK configs already work.

- Riptide ships a template (e.g. `assets/matugen/riptide.json`; no package installs `assets/`, so the README carries it inline
  say where) using the `{{colors.<role>.default.hex}}` form, e.g.
  `primary = "{{colors.primary.default.hex}}"`, one key per Material role used
  above — the form the maintainer's own templates use under matugen 4.2.0.
  `.default` is deliberate: it carries matugen's light/dark mode through.
- The user adds it to `~/.config/matugen/config.toml`:
  ```toml
  [templates.riptide]
  input_path  = '<path to shipped template>'
  output_path = '~/.config/riptide/colors.json'
  ```
  The output path sits beside `config.json` (`auth::config_path()`'s directory).
- Riptide watches `colors.json` (JSON: `serde_json` is already a dependency, TOML is not) and recolours live when matugen rewrites it — a
  wallpaper change reaches Riptide without a restart. It checks the file's mtime
  once a second from the existing tick, which needs no file-watch crate.
- **The file is all-or-nothing** (my call): every key must be present and parse,
  or the whole file is rejected. Filling a missing key from a Default constant
  would mix palette `Cyan`/`DarkGray` into a true-colour scheme, which
  `theme.rs`'s note forbids. A partial file is just one more kind of "malformed".
- **Missing, unreadable or malformed file → Default colours.** Log a single
  `warn!` naming the path and the problem (once per distinct failure, not per
  tick). The Settings help line is where the user learns about the template.
- README gets a short "Matching your desktop with matugen" section with the
  config snippet.

## Album mode

Computed in-process with the `material-colors` crate (port of Google's
material-color-utilities — the same algorithm matugen uses), so it works for
AUR/COPR/Nix users without matugen installed.

- Source image: the now-playing cover art bytes already fetched
  (`NowPlaying::art_bytes()`), decoded with the `image` crate Riptide already
  depends on. Downscale before quantising; quantising a full cover per track
  is wasteful.
- Seed: the crate's standard "source colour from image" (quantise + score), then
  a `SchemeTonalSpot` in the light/dark mode detected from the terminal (matugen's default scheme type) — builder may choose
  another scheme variant if it looks better, but it should match what matugen
  would produce for the same image so the two modes feel related.
- Recompute when the now-playing art changes, not per frame. Cache the result
  alongside the art. Doing the work off the render path (worker/spawn) is the
  builder's call based on measured cost.
- Colour follows **the playing track only**, not the row selected in a list.
- Switching tracks swaps the palette at once; no animated transition.
- **Between tracks, keep the previous palette while the new cover loads** (my
  call). A track change clears the art (`set_art_bytes(None)`, `art_loading =
  true` in `responses.rs`) before the new cover arrives; dropping to Default in
  that window would flash cyan on every track change.
- **Falls back to Default colours** when nothing is playing, when the track has
  no cover art, when the art request fails (`AlbumArtFailed`), comes back empty, or fails to decode. Note: `App::new` seeds
  `now_playing` with `DEFAULT_ART` (the Riptide logo) — that placeholder must
  count as "no art", not be themed from.

## Constraints

- Follow AGENTS.md: comments explain why, not what; no feature flags; YAGNI.
- One module owns the palette. Views ask it for styles; they stop naming
  `ACCENT`/`DIM`/`HIGHLIGHT_*` constants directly. How the active palette
  reaches render functions (field on `App`, passed through, thread-local like the
  image picker) is the builder's call — keep it simple.
- `row_style` / `row_dim_style` / `row_accent_style` keep their contract: every
  cell of a selected row carries the bar background.
- Tests: parsing the matugen output file (good, partial, malformed), the
  fallback rules, and "placeholder art is not themed from". Not per-view
  rendering snapshots.

## Non-goals

- Painting the terminal background.
- A Riptide-side light/dark override, user-picked scheme variants, contrast settings.
- Hand-written theme files or arbitrary colour customisation. The file format
  is whatever matugen's template produces; it is not a user-facing theme API.
- Running matugen from Riptide.
- Recolouring body text, error/status colours, or the cover art itself.

## Anti-goals

- An upgrade that changes how Riptide looks for someone who never touched the
  setting.
- Low-contrast text: a dark-on-dark or washed-out selection bar on some cover
  is a failure even if the code is correct. Material's container/on-container
  pairs exist for this; use them as pairs.
- Visible stutter at track change from palette extraction.

## Left to the builder

Template file location and exact key names; file-watch mechanism; where palette
state lives and how views reach it; scheme variant fine-tuning; exact Material
role for `HIGHLIGHT_DIM` and `SELECT_*`; wording of the Settings help line.

## Process

Per `ROADMAP.md`/AGENTS.md: file the issue and add it to the Riptide Development
board before branching (`feature/theme-colours` or similar, off latest master);
add a CHANGELOG `Unreleased → Added` entry; move the roadmap entry if one exists.
