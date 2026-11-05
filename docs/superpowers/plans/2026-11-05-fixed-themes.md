# Fixed Themes and a Theme Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ten built-in themes and user theme files, all in Catppuccin's colour vocabulary, which paint the whole window, plus a filterable picker with live preview on the Settings Theme row.

**Architecture:** A theme file (`name` plus 22 Catppuccin colours) parses into a `Palette`, which gains new roles (`background`, `text`, `border`, `error`, `success`, `bio`). A `ThemeCatalog` merges Default, Album, the ten compiled-in files and `~/.config/riptide/themes/*.json` under case-insensitive names. `App` stores the chosen theme's name and resolves it through the catalog in `active_palette()`. A new `ThemePicker` overlay stacks on Settings, filters, previews by changing that name, and puts back the original name on Esc.

**Tech Stack:** Rust 2024, ratatui 0.30, `serde_json` (already a dependency). No new crates.

**Spec:** `docs/superpowers/specs/2026-11-05-fixed-themes-design.md` (builds on `2026-11-05-matugen-theme-design.md`; commit `3553ec8`)

## Global Constraints

- Default and Album render exactly as in `3553ec8`. For both, every new role takes the value it has today: `background: None`, `text: Color::White`, `border` = `dim`, `error: Color::Red`, `success: Color::Green`, `bio: Color::Rgb(180, 180, 180)`.
- A theme file is JSON with `{"name": String, "colors": {…22 keys…}}`. The keys are `base mantle crust text subtext0 subtext1 surface0 surface1 surface2 overlay0 overlay1 overlay2 blue sapphire peach green red mauve pink yellow maroon teal`, and each value is `#rrggbb`. If any key is missing or invalid, the whole file is rejected. Unknown extra keys are ignored.
- Role mapping, verbatim from the spec:

  | Palette role | Colour |
  |---|---|
  | `background` | `base` |
  | `text` | `text` |
  | `dim` | `subtext0` |
  | `border` | `overlay0` |
  | `accent` | `mauve` |
  | `highlight_bg` | `surface1` |
  | `highlight_fg` | `text` |
  | `highlight_dim` | `subtext1` |
  | `highlight_accent` | `mauve` |
  | `select_bg` | `mauve` |
  | `select_fg` | `base` |
  | `error` | `red` |
  | `success` | `green` |
  | `bio` | `subtext1` |

- Names compare case-insensitively everywhere. A user file named "Default" or "Album" is rejected.
- A user file replaces a built-in with the same name. If two user files share a name, the alphabetically first filename wins and the other is skipped.
- The themes folder is `dirs::config_dir()/riptide/themes`. It's read at startup and each time the picker opens, never watched.
- `Preferences.theme` is the theme's name as a string, defaulting to `"default"`. Configs from `3553ec8` hold `"default"` or `"album"` and must load unchanged.
- Built-in theme files live in `assets/themes/*.json` and are compiled in with `include_str!`. `THIRD_PARTY_NOTICES.md` credits each adapted palette.
- AGENTS.md rules apply: comments explain why, one `warn!` per skipped file, and no per-item log lines.
- **No commits.** The maintainer commits after testing. Each task ends with `cargo test` and `cargo clippy --all-targets -- -D warnings` passing.
- Before Task 1: an issue for this work exists, is on the board as In progress, and is linked to `feature/theme-colours`. Verify, don't assume.

## Review Focus

1. **Typing a filter in the picker.** `handle_key` runs `vim_arrows` before the settings branch (`src/events/mod.rs:313`), so a `j` typed into the filter would arrive as Down. Expected: every printable character goes into the query. Test in Task 4.
2. **Any overlay under a fixed theme.** Every overlay calls `Clear`, which resets cells to the terminal default. Expected: no patch of terminal background anywhere. Test in Task 2, rendering the command overlay under Cyberpunk.
3. **Esc after previewing several themes.** Expected: the theme active when the picker opened, not the last one previewed. Test in Task 4.
4. **A saved custom theme whose file was deleted.** Expected: Riptide starts in Default, shows one status message, and keeps the saved name until the user picks another theme, so restoring the file brings it back. Test in Task 3.
5. **A filter that matches nothing, then Enter.** Expected: nothing changes and the picker stays open, with no panic on an empty selection. Test in Task 4.

---

### Task 1: Theme files, new palette roles, and the ten built-ins

**Files:**
- Create: `src/colors/theme_file.rs`
- Create: `assets/themes/{catppuccin-latte,catppuccin-frappe,catppuccin-macchiato,catppuccin-mocha,nord,gruvbox-dark,tokyo-night,dracula,rose-pine,cyberpunk}.json` (ten files; Cyberpunk takes the spec's values verbatim)
- Modify: `src/colors/mod.rs` (new `Palette` fields, `DEFAULT`, `from_roles`)
- Modify: `THIRD_PARTY_NOTICES.md`

**Interfaces:**
- Produces:
  ```rust
  // colors/mod.rs: Palette gains
  pub background: Option<Color>, pub text: Color, pub border: Color,
  pub error: Color, pub success: Color, pub bio: Color,
  // colors/theme_file.rs
  pub struct ThemeFile { pub name: String, pub palette: Palette }
  pub fn parse(json: &str) -> anyhow::Result<ThemeFile>;
  pub const BUILTIN: &[&str] = &[ /* include_str! of the ten files, in the spec's order */ ];
  pub fn builtins() -> Vec<ThemeFile>;   // panics only if a compiled-in file is bad; the test below guarantees it isn't
  ```
- `Palette::from_roles` (Album) fills the new fields with the Default values from Global Constraints.

- [ ] **Step 1: Write the failing tests** in `theme_file.rs`:
  - `cyberpunk_parses_with_its_exact_values`: `parse(CYBERPUNK)` (the spec's JSON) gives name `"Cyberpunk"`. The palette has `background == Some(Rgb(0x10,0x10,0x14))`, `text == Rgb(0x00,0xf0,0xff)`, `dim == Rgb(0xd0,0x00,0x6f)`, `border == Rgb(0x4d,0x4d,0x6c)`, `accent == Rgb(0xe6,0x00,0xff)`, `highlight_bg == Rgb(0x26,0x26,0x36)`, `select_fg == Rgb(0x10,0x10,0x14)`, `error == Rgb(0xff,0x00,0x55)` and `success == Rgb(0x00,0xff,0x66)`.
  - `missing_colour_rejects_the_file`: drop `"teal"` and the result is `Err`.
  - `bad_hex_rejects_the_file`: `"red": "red"` gives `Err`.
  - `missing_name_rejects_the_file`.
  - `every_builtin_parses_and_has_a_unique_name`: `builtins().len() == 10`. Names, case-folded, are unique and equal to `["catppuccin latte", "catppuccin frappé", "catppuccin macchiato", "catppuccin mocha", "nord", "gruvbox dark", "tokyo night", "dracula", "rosé pine", "cyberpunk"]`.
  - In `colors/mod.rs`, extend `default_palette_is_todays_look` with `background == None`, `text == Color::White`, `border == Color::DarkGray`, `error == Color::Red`, `success == Color::Green` and `bio == Color::Rgb(180,180,180)`.
- [ ] **Step 2: Run** `cargo test colors::` and expect a failure.
- [ ] **Step 3: Implement.**
  - Bring back an `Rgb` `#rrggbb` deserializer: a `FromStr` plus `Deserialize`. It's the same shape as the one removed with matugen, so reuse it.
  - Add a `#[derive(Deserialize)] struct CatppuccinColors` with all 22 fields and no `#[serde(default)]`.
  - Map it with the table in Global Constraints.
  - Transcribe Nord, Gruvbox Dark, Tokyo Night, Dracula and Rosé Pine from their projects' published palettes, putting each source colour on the nearest Catppuccin role. The four Catppuccin flavours use the official values exactly.
  - Add an entry to `THIRD_PARTY_NOTICES.md` for each adapted palette, giving the project, URL and licence, in the CLIamp entry's format.
- [ ] **Step 4: Run** `cargo test` and clippy. Expect a pass, because no view reads the new roles yet.
- [ ] **Checkpoint.**

### Task 2: Views draw every themed role, and fixed themes paint the background

**Files:**
- Modify: `src/ui/theme.rs` (`clear_area`; `row_style` and its siblings use `text`)
- Modify: `src/ui/mod.rs:58` (`draw` fills the frame)
- Modify: every `src/ui/*.rs` with `Color::White` (28 sites), `Color::Red` / `Color::Green` (`home.rs:164`, `overlays.rs:727,732,826`), `artist_detail.rs:132`'s `Rgb(180,180,180)`, every `border_style(...palette().dim...)`, and every `render_widget(Clear, …)` (12 sites)

**Interfaces:**
- Consumes: the new `Palette` fields from Task 1.
- Produces: `pub(super) fn clear_area(f: &mut Frame, area: Rect)`. It renders `Clear` and then, when `palette().background` is `Some(bg)`, fills `area` with `Style::default().bg(bg)`. Every `Clear` site calls this instead. Its doc comment says why: `Clear` resets cells to the terminal default.
- `draw` fills `f.area()` with the background right after `set_palette`, when there is one.

- [ ] **Step 1: Write the failing tests** in `src/ui/mod.rs` tests, using the existing `TestBackend` helpers:
  - `fixed_theme_paints_every_cell`: install Cyberpunk's palette with `set_palette`, render `draw` for a fresh test app at 80×24, and assert every cell's `bg == Color::Rgb(0x10,0x10,0x14)`, except where `highlight_bg` or `select_bg` is expected (assert those cells are `Rgb(0x26,0x26,0x36)` or `Rgb(0xe6,0x00,0xff)`). Because `draw` calls `set_palette(app.active_palette())`, this test has to run after Task 3. Until then, write it against a helper `draw_with(f, app, palette)`, which `draw` calls with `app.active_palette()`.
  - `fixed_theme_covers_overlays` (Review Focus 2): same setup with `app.command.active = true`. No cell has `bg == Color::Reset`.
  - `default_theme_paints_no_background`: draw with `Palette::DEFAULT`. Every cell outside the selection and select highlights has `bg == Color::Reset`.
- [ ] **Step 2: Run** `cargo test ui::tests::fixed_theme`. Expect a failure, with cells at `Reset`.
- [ ] **Step 3: Implement.**
  - Mechanical replacements: `Color::White` becomes `palette().text`, Red and Green become `palette().error` / `palette().success`, and the bio grey becomes `palette().bio`. Borders currently drawn with `palette().dim` become `palette().border`. Secondary text keeps `dim`.
  - Use `perl -pi` through `xargs`, because zsh doesn't split unquoted variables. Afterwards, check the `Modifier` and `Color` imports.
  - Add `clear_area` and the frame fill.
- [ ] **Step 4: Run** `cargo test`. Expect a pass, including every existing render test, which is the guarantee that Default is unchanged. Run `grep -rnE "Color::(White|Red|Green)|Rgb\(180, 180, 180\)|render_widget\(Clear" src/ui` and expect only `theme.rs` (the `clear_area` body) and test code.
- [ ] **Checkpoint.**

### Task 3: The catalogue, the name-based setting, and startup resolution

**Files:**
- Create: `src/colors/catalog.rs`
- Modify: `src/colors/mod.rs` (remove `ThemeSource`)
- Modify: `src/app/state/sort.rs` (`Preferences.theme: String`)
- Modify: `src/app/mod.rs` (fields, `active_palette`, `set_album_art`, `preferences()`, construction)
- Modify: `src/app/settings.rs` (row value, stepping, `set_theme`)
- Modify: `src/main.rs` (load the catalogue at startup; post the missing-theme status)

**Interfaces:**
- Produces, in `colors::catalog`:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub enum ThemeKind { Default, Album, Fixed(Palette) }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Origin { Builtin, User }
  #[derive(Debug, Clone)]
  pub struct ThemeEntry { pub name: String, pub kind: ThemeKind, pub origin: Origin }
  #[derive(Debug, Clone, Default)]
  pub struct ThemeCatalog { pub entries: Vec<ThemeEntry>, pub skipped: usize }
  impl ThemeCatalog {
      pub fn load(user_dir: &Path) -> Self;           // built-ins, then user files (alphabetical by filename)
      pub fn builtin_only() -> Self;                  // what tests construct
      pub fn find(&self, name: &str) -> Option<&ThemeEntry>;   // case-insensitive
      pub fn step(&self, name: &str, forward: bool) -> &str;   // wraps; unknown name steps from Default
  }
  pub fn themes_dir() -> PathBuf;                    // config_dir()/riptide/themes
  ```
  - Ordering: `Default`, `Album`, then built-ins in the spec's order with any user replacement kept in that slot, then the remaining user themes alphabetically by name.
  - Each skipped file gets one `warn!("theme: skipping {path}: {reason}")`.
- Produces on `App`: `theme: String`, `theme_catalog: ThemeCatalog`, `pub(crate) fn set_theme(&mut self, name: &str)` and `pub(crate) fn is_album_theme(&self) -> bool`.
  - `set_theme` stores the entry's canonical name, recomputes the album palette when the theme is Album (the logic from `set_theme_source`), and announces `"Theme: <name>"`.
  - `active_palette()` resolves `theme` through `find`. `Fixed(p)` gives `p`. `Album` follows today's rules. `Default` or an unknown name gives `Palette::DEFAULT`.
  - The Theme row shows the canonical name, or `"Default"` when the name is unknown.
  - `set_album_art` and `set_theme`'s Album branch use `is_album_theme()`.
- `main.rs`: after `App::new`, set `app.theme_catalog = ThemeCatalog::load(&themes_dir())`. If `config.prefs.theme` doesn't resolve, set the status `"Theme <name> not found — using Default"` (`StatusLevel::Info`) and leave `app.theme` as the saved string (Review Focus 4).

- [ ] **Step 1: Write the failing tests.**
  - In `catalog.rs`, using a temp dir under `std::env::temp_dir()` with the process id in its name, removed at the end:
    - `builtin_order_is_default_album_then_the_ten`: the names of `builtin_only()` are Default, Album, then the ten built-in names in order, twelve entries in all.
    - `find_ignores_case`: `find("CATPPUCCIN mocha")` gives the Mocha entry, and `find("album")` gives the Album entry.
    - `user_file_replaces_a_builtin_in_place`: a file named `"Nord"` with different colours sits at Nord's index with `Origin::User` and its own palette.
    - `user_file_named_default_is_skipped`: also covers `"album"`. `skipped == 2`, and `find("default")` is still `ThemeKind::Default`.
    - `duplicate_user_names_keep_the_first_filename`: `a.json` and `b.json` are both named "Mine". The entry's palette comes from `a.json`, and `skipped == 1`.
    - `invalid_user_file_is_counted_not_listed`: a truncated JSON file gives `skipped == 1` and isn't in the entries.
    - `missing_dir_is_builtins_only`.
    - `step_wraps_both_ways`: `step("Default", false)` is the last entry's name, and `step("Cyberpunk", true)` in `builtin_only()` is `"Default"`.
  - In `sort.rs`, replace the old test with `old_theme_values_still_load`: `{"theme":"album"}` gives `prefs.theme == "album"`, and `{}` gives `"default"`.
  - In `settings.rs`, port the five Album tests to `set_theme("Album")` and `t.app.theme = "Album".into()`. Add:
    - `fixed_theme_palette_is_the_files`: `set_theme("cyberpunk")` makes `active_palette().background == Some(Rgb(0x10,0x10,0x14))`, and `t.app.theme == "Cyberpunk"` (canonical).
    - `unknown_saved_theme_draws_default_and_keeps_the_name` (Review Focus 4): `t.app.theme = "gone".into()` makes `active_palette() == Palette::DEFAULT` and leaves `t.app.theme == "gone"`.
- [ ] **Step 2: Run** `cargo test catalog theme` and expect a failure.
- [ ] **Step 3: Implement.** `test_support` builds the app with `ThemeCatalog::builtin_only()`, so tests never read the real `~/.config`.
- [ ] **Step 4: Run** `cargo test` (three times, because of the shuffle-dependent tests) and clippy. Expect a pass.
- [ ] **Step 5: Run** the Task 2 render tests through `draw` with `set_theme("cyberpunk")` instead of `draw_with`, and delete `draw_with` if nothing else uses it.
- [ ] **Checkpoint.**

### Task 4: The theme picker

**Files:**
- Create: `src/app/state/theme_picker.rs` (re-exported from `state/mod.rs`)
- Modify: `src/app/settings.rs` (Enter on Theme opens the picker; `open_theme_picker`, `preview_theme`, `close_theme_picker`)
- Modify: `src/events/mod.rs` (route the picker before `vim_arrows`); `src/events/overlays.rs` (`handle_theme_picker_input`)
- Modify: `src/ui/overlays.rs` (`render_theme_picker`); `src/ui/mod.rs` (draw it after Settings; add `Overlays::THEME_PICKER`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Default)]
  pub struct ThemePicker { pub active: bool, pub query: String, pub selected: usize, pub original: String }
  impl ThemePicker {
      pub fn matches<'a>(&self, catalog: &'a ThemeCatalog) -> Vec<&'a ThemeEntry>; // case-insensitive substring on name, catalogue order
  }
  ```
- `App::open_theme_picker(&mut self)`:
  - Reloads `theme_catalog` from `themes_dir()`. A test build uses `builtin_only()`; route this through a `catalog_source` the test support can swap, or a `#[cfg(test)]` path, whichever reads cleaner.
  - Sets `original = theme.clone()`, clears the query, and puts `selected` on the current theme's row.
- Keys while the picker is open:
  - Printable characters are appended to `query` and `selected` resets to 0. Backspace pops a character.
  - Up and Down move within the matches.
  - Every change of the highlighted row calls `preview_theme(name)`, which sets `self.theme` without a status message.
  - Enter on a match calls `set_theme(name)` and closes. Enter with no matches does nothing (Review Focus 5).
  - Esc sets `theme = original` and closes. Settings stays open in both cases.
- Render:
  - Centred, `box_w = 40`, height clamped to the area, title `" theme "`.
  - An input line `› <query>`.
  - The group label `"built in"` (dim) above the built-in and Default/Album rows, and `"yours  ~/.config/riptide/themes"` above the `Origin::User` rows. A group with no matches gets no label.
  - The selected row in `select_bg` / `select_fg`.
  - When `catalog.skipped > 0`, a dim footer: `"{n} theme file skipped — see log"`, or `"files"` when n ≠ 1.
  - It scrolls to keep the selection visible.

- [ ] **Step 1: Write the failing tests.**
  - In `theme_picker.rs`: `matches_filter_by_substring_ignoring_case`. With query `"MOC"` against `builtin_only()`, the result is only Catppuccin Mocha.
  - In `src/events` tests, driving `handle_key` with `KeyEvent`s on a `test_app()` with `settings.active = true` and the Theme row selected:
    - `enter_on_theme_row_opens_the_picker`.
    - `typing_j_goes_into_the_filter` (Review Focus 1): open the picker and type `j`. `query == "j"`, and `selected` didn't move.
    - `moving_previews_and_esc_restores` (Review Focus 3): start on Default, open, press Down three times, and check `app.theme != "Default"`. Press Esc, and `app.theme == "Default"`, `theme_picker.active == false`, `settings.active == true`.
    - `enter_keeps_the_highlighted_theme`: type `cyber`, press Enter. `app.theme == "Cyberpunk"`, the picker is closed and Settings is open.
    - `enter_with_no_matches_does_nothing` (Review Focus 5): type `zzzz`, press Enter. The picker is still open and `app.theme` is unchanged.
  - In `ui/overlays.rs` tests: `picker_reports_skipped_files`. Set `theme_catalog.skipped = 2` and the rendered text contains `"2 theme files skipped — see log"`.
- [ ] **Step 2: Run** `cargo test picker` and expect a failure.
- [ ] **Step 3: Implement.** Keep the Settings rule: while the picker is open it owns every key, the same as `settings.active` does.
- [ ] **Step 4: Run** `cargo test` (three times) and clippy. Expect a pass.
- [ ] **Step 5: Manual check** in `cargo run`:
  - Step through the picker. Every theme paints the whole window, overlays included, and Esc puts back the original.
  - Drop a copy of the spec's Cyberpunk JSON, renamed to "Mine", into `~/.config/riptide/themes/` and reopen the picker. It appears under "yours".
- [ ] **Checkpoint.**

### Task 5: Docs

**Files:**
- Modify: `README.md` (Theme section and the prefs table: `theme` is a name)
- Modify: `CHANGELOG.md` (`Unreleased → Added`; extend the #83 entry or add one for the new issue)
- Modify: `AGENTS.md` (UI Patterns: `clear_area`, never a `Color::` literal, theme files are the Catppuccin role set, built-ins compiled in)
- Modify: `ROADMAP.md` (the in-progress entry covers fixed themes too)

- [ ] **Step 1: Write the README section.** It lists the twelve entries (Default, Album and the ten built-in themes) and the user-file format, using the spec's Cyberpunk JSON as the example. It also covers the folder, the case-insensitive name rules, Default and Album being reserved, and skipped files appearing in the log.
- [ ] **Step 2: Write the CHANGELOG** in user language, referencing the issue numbers.
- [ ] **Step 3: Run** `cargo test`, clippy and `cargo build --release`. Expect them all to pass.
- [ ] **Checkpoint: hand to the maintainer for testing and commit.**
