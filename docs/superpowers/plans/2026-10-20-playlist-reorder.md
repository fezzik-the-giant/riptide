# Playlist Reorder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `m` picks up a track in one of your own playlists. `j`/`k`/`↑`/`↓` move it, Enter drops it with one request, and Esc puts it back (#74, PR 4 of 4 for #57).

**Architecture:** Move mode reorders the open detail list locally while the row moves, with no requests. On drop it sends one `MovePlaylistItem`. If that request fails, the row returns to where it started. The mode lives in `App.playlist_move` and captures keys like a modal. The moving row is found by its `itemId`, not by an index, so a remove or add response that lands mid-move can't make the mode lose it.

**Tech Stack:** Rust, ratatui/crossterm, reqwest, tokio worker (`ApiRequest`/`ApiResponse`).

**Spec:** `docs/superpowers/specs/2026-10-16-playlist-editing-design.md`

## Global Constraints

- Move is `PATCH /playlists/{id}/relationships/items` with body `{"data":[{"id":"<trackId>","type":"tracks","meta":{"itemId":"<itemId>"}}],"meta":{"positionBefore":"<itemId>"}}`. The entry lands immediately before `positionBefore`. Move-to-end sends the same body **with no top-level `meta` at all**. That is the form verified live on 2026-10-16, and Tidal answers `200`.
- Drop mapping: the row's new index `j` in the list. If `j + 1` exists, `positionBefore` is that row's `itemId`; if `j` is the last row, there is no `meta` (move to end). Dropping at the starting index sends nothing.
- **"Narrowed" means rows are actually hidden, not that Atmos is off.** The spec refuses move mode while the list is narrowed. Taken literally (`hide_atmos` set), that would refuse it for every user with Atmos off, which is the default. The check is `tracks.visible_len() != tracks.items.len()`, so only a playlist that really hides Atmos-only rows is refused. Detail views have no filter query, so Atmos is the only thing that can hide rows there.
- Also refused while the list is still paging (`!tracks.exhausted`). A drop on the last loaded row would send "move to end" and land past rows that haven't arrived yet.
- Refusal toasts: not owned → `Only your own playlists can be edited`; rows hidden → `Show Atmos (Shift+S) to reorder: some tracks here are hidden`; still paging → `Wait for the playlist to finish loading`; the row has no `itemId` → error toast, as `d` does.
- While moving, only `↑`/`↓` (and `j`/`k`, which `vim_arrows` already turns into arrows), Enter and Esc do anything. Every other key is ignored, `q` included.
- The moving row is drawn reversed, with `↕` beside its number. The footer reads `↑↓/jk move · Enter drop · Esc cancel`.
- Success toasts `Moved '<title>'`. Failure puts the row back at its starting index and toasts `Couldn't move '<title>': <detail>`.
- Commits: none per task (user rule). One commit after the user's TUI test, single-line conventional message, no trailer.

## Review Focus

1. **Atmos off on a playlist with no Atmos-only rows** must allow move mode; one with hidden rows must refuse. Test: Task 2 `move_mode_is_refused_only_when_rows_are_really_hidden`.
2. **The drop mapping at both ends.** A drop in the middle names the next row; a drop on the last row sends no `positionBefore`; a drop where it started sends nothing. Tests: Task 1 `a_move_names_the_row_it_lands_before_or_none_at_the_end` and Task 2 `dropping_sends_one_move_naming_the_next_row`.
3. **Revert on failure** puts the row back at its starting index, found by `itemId` among duplicates. Test: Task 2 `a_failed_move_puts_the_row_back`.
4. **Stray keys while moving.** `q`, `d` and `m` do nothing, and Esc restores the original order without a request. Test: Task 2 `move_mode_ignores_every_key_but_arrows_enter_and_esc`.
5. **Moving can't run off either end of the list,** and the cursor follows the row as it moves.

---

### Task 1: Move through the worker

**Files:**
- Modify: `src/api/client/playlists.rs`, `src/api/messages.rs`, `src/api/worker.rs`, `src/app/responses.rs`, `src/app/playlist_edit.rs` (stubs)

**Interfaces:**
- Produces:
  - `fn move_body(track_id: u64, item_id: &str, before: Option<&str>) -> serde_json::Value`, private.
  - `ApiClient::move_in_playlist(&self, uuid: &str, track_id: u64, item_id: &str, before: Option<&str>) -> Result<()>`, with one `debug!`.
  - `ApiRequest::MovePlaylistItem { playlist: Playlist, track: Track, item_id: String, before: Option<String>, origin: usize }`.
  - `ApiResponse::PlaylistItemMoved { playlist: Playlist, track: Track }` and `ApiResponse::PlaylistItemMoveFailed { playlist: Playlist, track: Track, item_id: String, origin: usize, error: String }`. The worker sends the second on failure instead of `write_failed`, because the revert needs `origin` and `item_id`. It also logs `error!` the way `write_failed` callers do.

- [ ] **Step 1: Write the failing test** in `playlists.rs`, `a_move_names_the_row_it_lands_before_or_none_at_the_end`. `move_body(7, "a", Some("b"))` should equal `{"data":[{"id":"7","type":"tracks","meta":{"itemId":"a"}}],"meta":{"positionBefore":"b"}}`, and `move_body(7, "a", None)` should equal `{"data":[{"id":"7","type":"tracks","meta":{"itemId":"a"}}]}`, with no `meta` key.
- [ ] **Step 2: Run it.** `cargo test a_move_names` → Expected: compile error, `move_body` not found.
- [ ] **Step 3: Implement** the interfaces. Route the two responses to stub methods `on_playlist_item_moved` and `on_playlist_item_move_failed` in `src/app/playlist_edit.rs`.
- [ ] **Step 4: Run it.** `cargo test a_move_names` → PASS. Then `cargo test` → all pass.

### Task 2: `m` move mode

**Files:**
- Modify: `src/app/state/playlist_edit.rs` (`PlaylistMove`), `src/app/mod.rs` (field), `src/app/playlist_edit.rs`, `src/events/navigation.rs` (`m` in the PlaylistDetail arm), `src/events/overlays.rs` (`handle_move_input`), `src/events/mod.rs` (routing and tests), `src/ui/lists.rs` (`render_track_list` moving row), `src/ui/playlist_detail.rs`, `src/ui/footer.rs`, plus the hint and help entries

**Interfaces:**
- Consumes: Task 1's request and responses.
- Produces:
  - `pub struct PlaylistMove { pub item_id: String, pub origin: usize }` and `App.playlist_move: Option<PlaylistMove>`. `Some` means the mode is on.
  - `App::start_move(&mut self)`, `App::step_move(&mut self, down: bool)`, `App::drop_move(&mut self)`, `App::cancel_move(&mut self)`.
  - `App::on_playlist_item_moved(playlist, track)` and `App::on_playlist_item_move_failed(playlist, track, item_id, origin, error)`.
  - `render_track_list` gains `moving: bool`. Every other caller passes `false`.

- [ ] **Step 1: Write the failing tests.** In `playlist_edit.rs`, start from an own playlist `p` open with rows `a`, `b`, `c` (item ids `a`, `b`, `c`), fully loaded:
  - `dropping_sends_one_move_naming_the_next_row`: cursor on `a`. `start_move`, then `step_move(true)`, so the order is `b,a,c` with the cursor on `a`. No request has been sent yet. `drop_move` sends exactly `[MovePlaylistItem { item_id == "a", before == Some("c"), origin == 0 }]` and ends the mode. Starting again on `a` and stepping down once more gives `b,c,a`, and the drop sends `before == None`.
  - `dropping_where_it_started_sends_nothing`: `start_move`, step down, step up, drop → no request, mode off.
  - `a_failed_move_puts_the_row_back`: rows `a`, `b`, `a2`, where `a2` is a duplicate of track `a`'s id with item id `a2`. Move `a` to the end, then `on_playlist_item_move_failed(..., item_id "a", origin 0, ...)`. The order is back to `a,b,a2` by item id, and the status is an Error containing `Couldn't move`.
  - `move_mode_is_refused_only_when_rows_are_really_hidden`: with `hide_atmos` set and no Atmos-only rows, `start_move` turns the mode on. After adding one Atmos-only row (`track_tagged(9, &["DOLBY_ATMOS"])`, refiltered), `start_move` is refused and the status is the hidden-rows toast.
  - `moving_stops_at_both_ends`: on `a`, `step_move(false)` leaves the order unchanged. On `c`, `step_move(true)` leaves it unchanged.
  - `m_on_someone_elses_playlist_or_a_loading_one_refuses`: not owned → the exact ownership toast. Owned but not exhausted → `Wait for the playlist to finish loading`. The mode stays off in both cases.

  In `events/mod.rs`:
  - `move_mode_ignores_every_key_but_arrows_enter_and_esc`: own playlist open, then `m`. `q`, `d` and `m` send nothing, leave `should_quit` false and keep the mode on. `j` moves the row down. Esc restores the original order, ends the mode and sends nothing.
- [ ] **Step 2: Run them.** `cargo test playlist_edit` and `cargo test events` → Expected: compile errors.
- [ ] **Step 3: Implement.**
  - `start_move` requires the top view to be a `PlaylistDetail` with Tracks focus. It checks ownership (exact toast), then loaded, then hidden rows, then the row's `itemId`, and records `origin` as the row's index in `items`. Since the list isn't narrowed, the visible index and the `items` index are the same.
  - `step_move` finds the row by `itemId`, swaps it with its neighbour, and sets `selected` to follow it.
  - `drop_move` finds the row's index `j`. If `j == origin`, it just ends the mode. Otherwise it reads the next row's `itemId`; if that row exists but has no `itemId`, it cancels and shows the error toast, so it never sends a silent move-to-end. Then it sends the request and ends the mode.
  - `cancel_move` moves the row back to `origin` and ends the mode.
  - The failure handler does the same restore on every open detail of that playlist, then toasts. The success handler toasts.
  - Routing: in `handle_key`, `if app.playlist_move.is_some() { handle_move_input(app, key); return; }` goes right after the delete confirmation. `handle_move_input` maps Up to `step_move(false)`, Down to `step_move(true)`, Enter to `drop_move` and Esc to `cancel_move`, and ignores everything else.
  - `KeyCode::Char('m') => Action::StartMove` in the PlaylistDetail arm.
  - Rendering: `render_playlist_detail` passes `moving = app.playlist_move.is_some()`. In `render_track_list`, the selected row when moving gets `Modifier::REVERSED` and the ordinal `format!("{:>3}↕ ", i + 1)`. `get_context_hint` returns the move footer while the mode is on.
- [ ] **Step 4: Run them.** `cargo test` → all pass. Then `cargo fmt --check` should be clean, and `cargo clippy --all-targets` warnings should match master.

### Task 3: Docs and closing out #57

**Files:** `src/app/state/keybinds.rs`, `README.md`, `CHANGELOG.md`, `AGENTS.md`, `ROADMAP.md`

- [ ] Help: `Keybind { key: "m", action: "Move track (own playlist view)" }`, plus a README row.
- [ ] CHANGELOG: an `Added` bullet for #74.
- [ ] AGENTS.md "Playlist Editing — Verified API": add the move body, including that move-to-end drops the whole top-level `meta`.
- [ ] ROADMAP.md: move "Playlist editing" to `## Shipped`, with the `Done` status and `next release (under [Unreleased])`, and a short line on what landed across #71–#74.
- [ ] `cargo test` → all pass.

### Task 4: Hand-off

- [ ] **Step 1:** Stop and ask the user for a TUI pass, on a throwaway playlist with 4 or more tracks:
  1. `m` on the second track. It should be highlighted with `↕`, and the footer should change.
  2. `j` twice, then Enter. It should land two rows down and the toast should say "Moved". Reopen the playlist, or check the official app, to confirm Tidal kept the order.
  3. `m` on a track, `j` until the last row, then Enter. It should stay last after a reopen.
  4. `m`, move it, then Esc. The original order should come back.
  5. While moving, `q` and `d` should do nothing.
  6. With Atmos off, `m` in a playlist with hidden Atmos tracks (Reya's Jams) should show the hidden-rows toast. Show Atmos, and it should work.
  7. `m` in a Tidal playlist should show the ownership toast.

  Do not commit, or ask to commit, until they confirm. After merge, set #74 and #57 to Done.
