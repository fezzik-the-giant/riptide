# Theme: Wallpaper and Album Colours Implementation Plan

> **Amended 2026-11-05:** Wallpaper mode was built and then removed before merge.
> Desktop shells that drive matugen run it with their own generated config, so a
> template in `~/.config/matugen/config.toml` is never rendered. See ROADMAP.md,
> "Theme: colours from the desktop wallpaper". What ships is **Default** and **Album**.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Settings → Theme choice (Default / Wallpaper / Album) that recolours Riptide's accents and selection bar from matugen's wallpaper output or from the playing track's cover art.

**Architecture:** A new top-level `src/colors/` module owns the `Palette` type (eight role colours), its `DEFAULT` (today's exact constants), the matugen-file parser and the cover-art extractor. `App` holds the theme source, the wallpaper palette and the album palette, and answers `active_palette()`. `ui::draw` installs that palette into a thread-local in `src/ui/theme.rs` once per frame, and views read `palette()` instead of the old constants.

**Tech Stack:** Rust 2024 (local toolchain rustc 1.95), ratatui 0.30, `image` 0.25 (already a dependency), `serde_json` (already), new: `material-colors = "0.4.2"` (default features, **not** the `image` feature), `terminal-colorsaurus = "1.0.3"`.

**Spec:** `docs/superpowers/specs/2026-11-05-matugen-theme-design.md`

## Global Constraints

- `material-colors` is pinned to **0.4.2**: 0.5.0 requires rustc 1.97 and this toolchain is 1.95. Do not enable its `image` feature; Riptide decodes with its own `image` crate and calls `QuantizerCelebi` + `Score` directly.
- The matugen output is **JSON**, not TOML: `serde_json` is already a dependency and there is no TOML crate. Path: `dirs::config_dir()/riptide/colors.json` (the directory of `auth::config_path()`). The spec's `colors.toml` is amended to match in Task 3.
- Default theme renders **exactly** today's colours, palette entries (`Color::Cyan`, `Color::DarkGray`) included. Upgrading changes nothing for anyone who leaves the setting alone.
- Themed modes are `Color::Rgb` in every field; never mix in a palette entry.
- The terminal background is never painted; only the selection bar and `SELECT_*` highlights carry a background.
- Body text (`Color::White`) and status colours (`Color::Red`, `Color::Green`) are untouched.
- AGENTS.md comment rules apply: comments say why, never what; no section banners.
- AGENTS.md logging rules: one `warn!` per distinct failure, never per tick.
- **No commits.** The maintainer commits after testing. Each task ends at a checkpoint where `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
- Before Task 1: an issue exists for this feature on the Riptide Development board, and the branch `feature/theme-colours` is cut from latest `master` and linked to it. Verify, don't assume.

## Review Focus

1. **A cover that is almost one colour** (black-and-white photo, solid-colour sleeve). `Score::score` falls back to Google blue; a reasonable person expects a usable palette rather than a crash or an invisible bar. Test in Task 4.
2. **matugen rewriting the file while Riptide reads it** (truncated or half-written JSON). Expected: Default for that read, then the right colours on the next poll. A parse failure must not latch. Test in Task 3.
3. **Album mode on the very first frame**, when `now_playing` holds the `DEFAULT_ART` logo and no track. Expected: Default colours, never a palette themed from the logo. Test in Task 4.
4. **Switching Theme while a track is playing.** Expected: Album mode recolours at once from the cover already showing, without waiting for the next track. Test in Task 4.
5. **A terminal that ignores the background query** (tmux without passthrough, the Linux console). Expected: startup is not visibly delayed, and Album picks dark. Covered by a fixed 200 ms timeout in Task 4. It is verified manually because there is no automated test for a terminal reply.

---

### Task 1: `Palette` type and moving every view onto it (no visible change)

**Files:**
- Create: `src/colors/mod.rs`
- Modify: `src/main.rs` (add `mod colors;`)
- Modify: `src/ui/theme.rs` (constants become `Palette::DEFAULT`; thread-local; `palette()`)
- Modify: `src/ui/mod.rs:58` (`draw` installs the palette)
- Modify: every `src/ui/*.rs` that names `ACCENT`, `DIM`, `HIGHLIGHT_*`, `SELECT_*`. These are artist_detail, tabs, footer, visualizer, carousel, now_playing, search, lists, playlist_detail, album_detail, home, art, overlays and queue. Also the stray greys: `overlays.rs:805,813` `Color::DarkGray` and `artist_detail.rs:128` `Color::Rgb(180,180,180)`, which become `palette().dim`.
- Modify: `src/app/mod.rs` (`pub fn active_palette(&self) -> Palette`, returning `Palette::DEFAULT` for now)

**Interfaces:**
- Produces, in `crate::colors`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Palette {
      pub accent: Color, pub dim: Color,
      pub highlight_bg: Color, pub highlight_fg: Color,
      pub highlight_dim: Color, pub highlight_accent: Color,
      pub select_bg: Color, pub select_fg: Color,
  }
  impl Palette { pub const DEFAULT: Palette = /* today's eight values verbatim */; }
  ```
- Produces, in `ui::theme`: `pub(super) fn palette() -> Palette` and `pub(super) fn set_palette(Palette)`. The thread-local follows the pattern in `ui/image.rs:65`. The `theme.rs` doc comment must say why a thread-local is used: `row_style` and its siblings are called from dozens of sites that hold no `&App`, and they keep their `(is_selected: bool)` signatures.
- Produces: `App::active_palette(&self) -> Palette`.

- [ ] **Step 1: Write the failing test.** In `src/colors/mod.rs`, add `default_palette_is_todays_look`. It asserts `Palette::DEFAULT.accent == Color::Cyan`, `.dim == Color::DarkGray`, `.highlight_bg == Color::Rgb(40, 40, 55)`, `.highlight_fg == Color::Rgb(236, 236, 245)`, `.highlight_dim == Color::Rgb(150, 150, 170)`, `.highlight_accent == Color::Rgb(120, 210, 232)`, `.select_bg == Color::Rgb(30, 100, 200)` and `.select_fg == Color::Rgb(236, 236, 245)`.
- [ ] **Step 2: Run** `cargo test colors::` and confirm it fails because `Palette` does not exist yet.
- [ ] **Step 3: Implement** `Palette` and `DEFAULT`. Delete the eight constants from `ui/theme.rs` and move their doc comment (why true colour) onto `Palette`. Add `palette()` / `set_palette()`, and call `set_palette(app.active_palette())` as the first line of `ui::draw`. Replace every constant use with `palette().<field>`. The two test helpers that compare against `HIGHLIGHT_BG` (`queue.rs:137`, `lists.rs:646`) switch to `Palette::DEFAULT.highlight_bg`.
- [ ] **Step 4: Run** `cargo test` and `cargo clippy --all-targets -- -D warnings`. Expect all tests to pass, including the existing row and queue rendering tests, which now act as the regression check that Default looks the same.
- [ ] **Step 5: Run** `grep -rnE "\b(ACCENT|DIM|HIGHLIGHT_[A-Z]+|SELECT_(BG|FG))\b" src/ui`. Expect no output.
- [ ] **Checkpoint.** Run `cargo run` and confirm the app looks identical to master. Do not commit.

### Task 2: The Theme setting and its persistence

**Files:**
- Modify: `src/colors/mod.rs` (`ThemeSource`)
- Modify: `src/app/state/sort.rs:38-90` (`Preferences.theme`)
- Modify: `src/app/state/settings.rs` (`Setting::Theme`, label, help, `ALL`)
- Modify: `src/app/settings.rs` (`setting_value`, `step_choice`, `set_theme_source`)
- Modify: `src/app/mod.rs` (`theme_source` field, the `preferences()` snapshot, construction)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "lowercase")]
  pub enum ThemeSource { #[default] #[serde(other)] Default, Wallpaper, Album }
  impl ThemeSource { pub fn label(self) -> &'static str; pub fn next(self) -> Self; pub fn previous(self) -> Self; }
  ```
  The labels are `"Default"`, `"Wallpaper"` and `"Album"`, and the cycle runs in that order and wraps. `#[serde(other)]` exists for the same reason as on `VisualizerMode`, so copy that comment's rationale.
- Produces: `App.theme_source: ThemeSource` and `pub(crate) fn set_theme_source(&mut self, source: ThemeSource)`. The setter announces `"Theme: <label>"` through `set_status` with `StatusLevel::Info`.
- `Setting::Theme` comes after `Visualizer` in `ALL`. Its label is `"Theme"`. Its help text is fixed: `"Wallpaper reads matugen's output (see README); Album follows the playing cover"`.

- [ ] **Step 1: Write the failing tests.**
  - `theme_source_cycles_and_wraps`: `Default.next() == Wallpaper`, `Album.next() == Default` and `Default.previous() == Album`.
  - `unknown_theme_source_loads_as_default`: `serde_json::from_str::<ThemeSource>("\"neon\"")` gives `Default`.
  - `preferences_without_theme_load_as_default`: `Preferences` deserialized from `{}` has `theme == ThemeSource::Default`.
  - Update `selection_stays_in_range` in `state/settings.rs` if its last-row assertion moves. It shouldn't, because Theme goes before Volume and Scrobbling stays last.
- [ ] **Step 2: Run** `cargo test theme`. Expect a failure.
- [ ] **Step 3: Implement** the enum, the preference field (`#[serde(default)]`), the setting row and the setter. `active_palette()` still returns `DEFAULT` regardless of source.
- [ ] **Step 4: Run** `cargo test` and clippy. Expect a pass.
- [ ] **Checkpoint.** In `cargo run`, the Settings row steps through Default → Wallpaper → Album, the choice survives a restart, and colours don't change yet.

### Task 3: Wallpaper mode (matugen file, live reload, template, README)

**Files:**
- Create: `src/colors/matugen.rs`
- Create: `assets/matugen/riptide.json` (the template)
- Modify: `src/colors/mod.rs` (`Palette::from_roles`)
- Modify: `src/app/mod.rs` (`wallpaper: WallpaperWatch` field; `tick` polls it; `active_palette` uses it)
- Modify: `README.md` (add a "Matching your desktop with matugen" section)
- Modify: `docs/superpowers/specs/2026-11-05-matugen-theme-design.md` (`colors.toml` → `colors.json`, stat polling chosen)

**Interfaces:**
- Produces, in `colors`:
  ```rust
  /// The six Material roles Riptide reads; both sources produce exactly these.
  pub struct Roles { pub primary: Rgb, pub on_primary: Rgb, pub primary_container: Rgb,
                     pub on_primary_container: Rgb, pub outline: Rgb, pub on_surface_variant: Rgb }
  impl Palette { pub fn from_roles(r: &Roles) -> Palette; }
  ```
  `Rgb` here is `(u8, u8, u8)` via a small `pub struct Rgb(pub u8, pub u8, pub u8)`, not a `material-colors` type, so this module does not need that crate. The mapping is fixed:

  | Palette field | Role |
  |---|---|
  | `accent` | `primary` |
  | `dim` | `outline` |
  | `highlight_bg` | `primary_container` |
  | `highlight_fg` | `on_primary_container` |
  | `highlight_dim` | `on_surface_variant` |
  | `highlight_accent` | `primary` |
  | `select_bg` | `primary` |
  | `select_fg` | `on_primary` |

  Every field is `Color::Rgb`.
- Produces, in `colors::matugen`:
  ```rust
  pub fn colors_path() -> PathBuf;                       // config_dir()/riptide/colors.json
  pub fn parse(json: &str) -> Result<Roles>;             // all six keys, "#rrggbb", or Err
  pub struct WallpaperWatch { /* path, last mtime, last error string, cached Option<Palette>, last poll Instant */ }
  impl WallpaperWatch {
      pub fn new(path: PathBuf) -> Self;
      pub fn poll(&mut self, now: Instant) -> bool;      // re-reads when ≥1 s since last poll AND mtime changed; true if palette changed
      pub fn palette(&self) -> Option<Palette>;
  }
  ```
  The JSON file holds exactly the six keys, with names matching the `Roles` fields (`{"primary": "#ffb68a", ...}`). Unknown extra keys are ignored. A missing key, a value that isn't `#rrggbb`, or invalid JSON rejects the whole file. A missing file is `None`, with no warning. Any other failure is logged once with `warn!("theme: ignoring {path}: {reason}")`, again only when the reason changes, and gives `None`. A later successful read replaces the palette, so a failure never sticks.
- The template `assets/matugen/riptide.json` uses one line per key, in the form `"primary": "{{colors.primary.default.hex}}"`. It stays `.default` so matugen's light/dark mode carries through.
- `App::active_palette()` is now `match self.theme_source { Default => DEFAULT, Wallpaper => self.wallpaper.palette().unwrap_or(DEFAULT), Album => DEFAULT /* Task 4 */ }`.
- `App::tick` calls `self.wallpaper.poll(Instant::now())` only when `theme_source == Wallpaper`. Switching to Wallpaper with `set_theme_source` forces an immediate poll, so the colours don't lag by up to a second.

- [ ] **Step 1: Write the failing tests** in `matugen.rs`:
  - `parses_all_six_roles`: a full JSON fixture gives `Roles` with `primary == Rgb(0xff, 0xb6, 0x8a)`.
  - `missing_role_rejects_the_whole_file`: dropping `on_primary` gives `Err`.
  - `bad_hex_rejects_the_whole_file`: `"primary": "orange"` gives `Err`.
  - `truncated_json_is_an_error`: the fixture cut in half gives `Err`.
  - `from_roles_is_true_colour_throughout`: every `Palette` field matches `Color::Rgb(..)`, and `select_fg` is `on_primary`.
  - `watch_recovers_after_a_bad_write` (Review Focus 2): in a `tempfile`-free temp dir under `std::env::temp_dir()`, write a good file, poll, then write a truncated one with a newer mtime. Poll again, expect `palette() == None`. Write the good file once more, poll, and expect `Some`. Drive time with explicit `Instant`s, 2 s apart, and set the mtime explicitly so the test doesn't depend on filesystem timing.
  - `missing_file_is_none`: `WallpaperWatch` on a path that doesn't exist gives `palette() == None` after `poll`.
- [ ] **Step 2: Run** `cargo test colors::matugen`. Expect a failure.
- [ ] **Step 3: Implement** `matugen.rs` and `from_roles`, and wire up `App`.
- [ ] **Step 4: Run** `cargo test` and clippy. Expect a pass.
- [ ] **Step 5: Write the template and README section.** The README section gives the `[templates.riptide]` snippet, with `input_path` pointing at the template's install location (check how `install.sh`, the AUR package and the spec install `assets/`; if they don't install it, the README tells users to copy it from the repo) and `output_path = '~/.config/riptide/colors.json'`. Amend the spec's Wallpaper section to say JSON and stat polling.
- [ ] **Step 6: Verify against a real matugen run.** Run `matugen image <any wallpaper> -m dark` with a scratch config that has only the riptide template, and confirm the output parses (`cargo test` fixture or a debug `cargo run`). Then run it again with `-m light` and confirm the values differ.
- [ ] **Checkpoint.** In `cargo run` with Theme set to Wallpaper, changing the wallpaper (or rerunning matugen) recolours the app within about a second, and deleting `colors.json` brings back Default.

### Task 4: Album mode (cover extraction, terminal light/dark, art hooks)

**Files:**
- Create: `src/colors/album.rs`
- Modify: `Cargo.toml` (`material-colors = "0.4.2"`, `terminal-colorsaurus = "1.0.3"`)
- Modify: `THIRD_PARTY_NOTICES.md` (both crates, following the file's existing format)
- Modify: `src/main.rs` (query the terminal before `enable_raw_mode()`, line 165)
- Modify: `src/app/mod.rs` (`terminal_dark: bool` and `album_palette: Option<Palette>` fields; `active_palette`)
- Modify: `src/app/responses.rs` (AlbumArt around line 295, AlbumArtFailed around 306, TrackArt around 751, TrackArtFailed around 757)
- Modify: `src/app/loading.rs:132-136` (the no-cover branch)
- Modify: `src/app/playback.rs:426` (where playback stops)

**Interfaces:**
- Produces, in `colors::album`:
  ```rust
  pub fn palette_from_cover(bytes: &[u8], dark: bool) -> Option<Palette>;
  ```
  It decodes with `image::load_from_memory`, then resizes to 128×128 with `FilterType::Triangle`, as the crate docs advise. It maps the pixels to `material_colors::color::Argb::new(255, r, g, b)` and calls `QuantizerCelebi::quantize(&pixels, 128)`. It takes `Score::score(&result.color_to_count, None, None, None)[0]`. That goes to `ThemeBuilder::with_source(seed).build()`, and the function reads `.schemes.dark` or `.schemes.light`, converts the six roles to `colors::Roles`, and returns `Palette::from_roles`. Empty bytes or a failed decode return `None`. It wraps the whole call in one `debug!` with elapsed milliseconds, so the anti-goal "visible stutter" can be measured from a log.
- Produces, in `main.rs`: `terminal_colorsaurus::theme_mode(QueryOptions { timeout: Duration::from_millis(200), ..Default::default() })`. `Ok(ThemeMode::Light)` sets `dark = false`, and anything else (including `Err`) sets `dark = true`. It's passed into `App::new` as a new `terminal_dark: bool` argument; update `test_support.rs` to pass `true`. It's logged once at `info!`.
- Produces, on `App`: `pub(crate) fn set_album_art(&mut self, bytes: Option<&[u8]>)`, which sets `self.album_palette = bytes.and_then(|b| palette_from_cover(b, self.terminal_dark))`. It's the only writer of `album_palette`. Call sites:
  - AlbumArt / TrackArt for the playing track: `Some(&image_data)`.
  - AlbumArtFailed / TrackArtFailed for the playing track, and the `else` branch in `fetch_now_playing_art` (no cover and no album id): `None`.
  - **Do not** call it where a track change clears art (`responses.rs:660`). Keeping the previous palette while the new cover loads is the spec's decision. Add a one-line why-comment there.
  - **Do not** call it from `App::new`'s `DEFAULT_ART` seeding.
- `active_palette()` Album arm: `if self.now_playing.track.is_none() { DEFAULT } else { self.album_palette.unwrap_or(DEFAULT) }`.
- `set_theme_source(Album)` recomputes from `self.now_playing.art_bytes()` when a track is playing and `album_palette` is `None` (Review Focus 4). The `track.is_some()` guard is what keeps the logo from being themed.

- [ ] **Step 1: Write the failing tests.**
  - In `album.rs`, `warm_cover_gives_a_warm_accent`: encode a 64×64 solid `#d2691e` PNG in-memory with `image`, then call `palette_from_cover(&png, true)`. The result is `Some`, and its `accent` is a `Color::Rgb(r, g, b)` with `r > b`.
  - In `album.rs`, `light_and_dark_schemes_differ`: from the same PNG, `palette_from_cover(&png, true) != palette_from_cover(&png, false)`.
  - In `album.rs`, `grey_cover_still_gives_a_palette` (Review Focus 1): a solid `#808080` PNG gives `Some`, with `highlight_bg != highlight_fg`.
  - In `album.rs`, `garbage_bytes_give_none`: `palette_from_cover(b"not an image", true)` gives `None`, and `&[]` gives `None`.
  - In `app` tests (using `test_support`), `album_mode_without_a_track_is_default` (Review Focus 3): a fresh app with `theme_source = Album` has `active_palette() == Palette::DEFAULT`.
  - `album_palette_survives_a_track_change_until_new_art`: with a palette set from art, simulate the track-change branch. `active_palette()` stays the old palette. Then deliver `TrackArtFailed` for the new track, and it becomes `DEFAULT`.
  - `switching_to_album_mid_track_recolours_at_once` (Review Focus 4): with a track playing and real PNG bytes in `now_playing`, `set_theme_source(Album)` makes `active_palette() != DEFAULT`.
- [ ] **Step 2: Run** `cargo test album` and expect a failure.
- [ ] **Step 3: Implement** the dependencies, `album.rs`, the startup query, the `App` fields, `set_album_art` and the call sites.
- [ ] **Step 4: Run** `cargo test` (several times: `playback.rs` tests depend on shuffle order, per AGENTS.md) and clippy. Expect a pass.
- [ ] **Step 5: Manual check.** In `cargo run` with Theme set to Album:
  - Playing tracks from albums with very different covers recolours the app on each change, with no cyan flash between tracks.
  - Stopping playback brings back Default.
  - `RIPTIDE_LOG_LEVEL=debug` shows the extraction time. Report the figure, and if it's above roughly 30 ms, flag it to the maintainer before moving the work off the UI thread.
  - Startup inside `tmux` without passthrough isn't noticeably slower.
- [ ] **Checkpoint.**

### Task 5: Changelog, roadmap and AGENTS.md

**Files:**
- Modify: `CHANGELOG.md` (`## [Unreleased]` → `### Added`)
- Modify: `ROADMAP.md` (move a matching entry to Shipped if one exists, and update the status table)
- Modify: `AGENTS.md` (`UI Patterns`: a short paragraph on colour roles)

- [ ] **Step 1: Write the CHANGELOG entry** in user language: a new Theme setting. Wallpaper follows matugen, including its light/dark mode, through the shipped template. Album takes its colours from the playing cover and picks light or dark from the terminal's background. Default is unchanged. Reference the issue number.
- [ ] **Step 2: Add to AGENTS.md:**
  - Views take colours from `palette()`, never `Color::` literals for themed roles.
  - Body text and status colours are left alone on purpose.
  - Album mode must never theme from `DEFAULT_ART`, and `set_album_art` is the only writer.
  - `material-colors` is pinned to 0.4.2 until the toolchain reaches 1.97.
- [ ] **Step 3: Run** `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo build --release`. Expect them all to pass.
- [ ] **Checkpoint: hand to the maintainer for testing and commit.**
