# Playlist Rename and Delete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `R` renames and `D` deletes (after a y/n confirmation) one of your own playlists, from the Playlists tab or from the playlist's own view (#73, PR 3 of 4 for #57).

**Architecture:** Two new writes go through the existing worker. Rename reuses PR 1's name prompt: the prompt gets a target, either a new playlist or a rename of a given one. Delete adds a confirmation overlay; it needs a ninth overlay flag, so `Overlays` widens from `u8` to `u16`. Both apply on confirm.

**Tech Stack:** Rust, ratatui/crossterm, reqwest, tokio worker (`ApiRequest`/`ApiResponse`).

**Spec:** `docs/superpowers/specs/2026-10-16-playlist-editing-design.md`

## Global Constraints

- Rename is `PATCH /playlists/{id}` with body `{"data":{"id":"<id>","type":"playlists","attributes":{"name":"<name>"}}}`. Tidal answers `200`, and the attributes left out stay unchanged.
- Delete is `DELETE /playlists/{id}` with **no body**, the form verified live. Tidal answers `200`; afterwards the playlist is `404` and gone from the collection.
- Name prompt: titled "rename playlist" when renaming, prefilled with the current name. Input is trimmed, and Enter on an empty name does nothing. There's no length check, so a rejection arrives as an error toast.
- Delete confirmation reads `Delete "<name>"? This can't be undone.` `y` deletes, `n` or Esc cancels, and every other key is ignored.
- `R` and `D` on a playlist you don't own show exactly `Only your own playlists can be edited`.
- Apply on confirm: local state changes only in the success handlers. Failures go through `write_failed`.
- Delete removes the playlist from `playlists`, `favorite_playlist_ids` and `owned_playlist_ids`, and closes every view of it.
- Commits: none per task (user rule). One commit after the user's TUI test, single-line conventional message, no trailer.

## Review Focus

1. **The terminal spells Shift+r / Shift+d as lowercase plus SHIFT.** That must not start radio (`r`) or remove from the library (`d`). Test: Task 3 `shift_r_and_shift_d_never_fall_through_to_radio_or_library_removal`.
2. **A playlist you don't own:** a Tidal-curated one on the Playlists tab, or anything when ownership failed to load. No prompt or confirmation opens, and the exact toast shows. Tests: Task 2 `r_on_someone_elses_playlist_opens_nothing_and_says_why` and Task 3 `d_on_someone_elses_playlist_asks_nothing`.
3. **Deleting a playlist whose view is open, including under another pushed view** (playlist → artist): every view of it closes and other views stay. Test: Task 3 `deleting_a_playlist_closes_every_view_of_it_and_nothing_else`.
4. **Renaming to the same name, or to a blank one,** sends no request. Test: Task 2 `renaming_to_the_same_or_a_blank_name_sends_nothing`.
5. **Stray keys in the confirmation.** `q` (quit), `d` and `j` are ignored; the confirmation stays open and nothing is sent. Test: Task 3 `the_delete_confirmation_ignores_every_key_but_y_n_and_esc`.

---

### Task 1: Rename and delete through the worker

**Files:**
- Modify: `src/api/client/mod.rs` (`send_openapi_json`), `src/api/client/playlists.rs`, `src/api/messages.rs`, `src/api/worker.rs`

**Interfaces:**
- Produces:
  - `send_openapi_json(method, path, body: Option<&serde_json::Value>)`. Every existing caller passes `Some(&body)`; `None` sends no body.
  - `fn rename_body(uuid: &str, name: &str) -> serde_json::Value`, private, in `playlists.rs`.
  - `ApiClient::rename_playlist(&self, uuid: &str, name: &str) -> Result<()>` and `ApiClient::delete_playlist(&self, uuid: &str) -> Result<()>`. Each logs one `debug!` with the uuid.
  - `ApiRequest::RenamePlaylist { playlist: Playlist, name: String }` and `ApiRequest::DeletePlaylist { playlist: Playlist }`.
  - `ApiResponse::PlaylistRenamed { playlist: Playlist, name: String }` and `ApiResponse::PlaylistDeleted { playlist: Playlist }`. Failure toasts: `Couldn't rename '<title>'` and `Couldn't delete '<title>'`.

- [ ] **Step 1: Write the failing test** in `playlists.rs`:

```rust
#[test]
fn a_rename_sends_only_the_name() {
    assert_eq!(
        rename_body("7c56e2bc-d5c3-44ef-adeb-57b5b1327d28", "Road trip"),
        serde_json::json!({"data": {"id": "7c56e2bc-d5c3-44ef-adeb-57b5b1327d28",
            "type": "playlists", "attributes": {"name": "Road trip"}}})
    );
}
```

- [ ] **Step 2: Run it.** `cargo test a_rename_sends_only` → Expected: compile error, `rename_body` not found.
- [ ] **Step 3: Implement** the interfaces above. Make `.json(body)` conditional on `Some`. Until Tasks 2 and 3 fill in the handlers, route the two responses in `responses.rs` to stub methods `on_playlist_renamed` and `on_playlist_deleted` in `src/app/playlist_edit.rs`.
- [ ] **Step 4: Run it.** `cargo test a_rename_sends_only` → PASS. Then `cargo test` → all pass.

### Task 2: `R` renames

**Files:**
- Modify: `src/app/state/playlist_edit.rs` (`NamePrompt`), `src/app/playlist_edit.rs`, `src/ui/overlays.rs` (`render_name_prompt` title), `src/events/global.rs`, `src/events/mod.rs` (tests), `src/app/state/keybinds.rs`, `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1's request and response.
- Produces:
  - `pub enum NameTarget { New { then_add: Option<Track> }, Rename(Playlist) }`, with `Default` = `New { then_add: None }`.
  - `NamePrompt.target: NameTarget` replaces `then_add`. `open_name_prompt(then_add)` keeps its signature.
  - `App::playlist_under_cursor(&self) -> Option<Playlist>`: the top view's playlist if the top view is a `PlaylistDetail`; with no view open on the Playlists tab, the selected row; otherwise `None`.
  - `App::own_playlist_under_cursor(&mut self) -> Option<Playlist>`: like the above, but shows the "Only your own…" toast and returns `None` when the playlist is not owned. Task 3 reuses it.
  - `App::open_rename_prompt(&mut self)` and `App::on_playlist_renamed(&mut self, playlist: Playlist, name: String)`.

- [ ] **Step 1: Write the failing tests** in `playlist_edit.rs`:
  - `r_opens_the_prompt_prefilled_with_the_current_name`: on the Playlists tab, owned `p` titled "Mine" is selected. After `open_rename_prompt()`, the prompt is active, `input == "Mine"`, and the target is `Rename(p)`.
  - `renaming_sends_the_new_name_and_applies_it_on_confirm`: from that state, set the input to `"  Road trip "` and call `submit_name_prompt()`. The requests are exactly `[RenamePlaylist { playlist.uuid == "p", name == "Road trip" }]`, and the library still says "Mine". After `on_playlist_renamed(p, "Road trip")`, both the library row and an open `PlaylistDetail` of `p` say "Road trip".
  - `renaming_to_the_same_or_a_blank_name_sends_nothing`: input `" Mine "` closes the prompt and sends nothing; input `"   "` sends nothing and leaves the prompt open.
  - `r_on_someone_elses_playlist_opens_nothing_and_says_why`: `p` is not owned. The prompt stays closed, and the status is exactly `Only your own playlists can be edited`.
- [ ] **Step 2: Run them.** `cargo test playlist_edit` → Expected: compile errors for the missing methods and fields.
- [ ] **Step 3: Implement.**
  - `submit_name_prompt` branches on the target. Rename sends nothing and closes the prompt if the trimmed name equals the current title.
  - `on_playlist_renamed` sets the title on the library row (then calls `sort_playlists`) and on every open `PlaylistDetail` of that uuid, then toasts `Renamed to '<name>'`.
  - `render_name_prompt` titles itself ` rename playlist ` for a rename and ` new playlist ` otherwise.
  - In `global.rs`, add `KeyCode::Char('R') | KeyCode::Char('r') if SHIFT => app.open_rename_prompt()` before the bare-letter arms. The arm has no context guard, so Shift+r never reaches the bare `r` (radio); `open_rename_prompt` does nothing where there is no playlist under the cursor.
  - Help: `Keybind { key: "R", action: "Rename playlist (your own)" }`, plus a README row and a CHANGELOG bullet.
- [ ] **Step 4: Run them.** `cargo test playlist_edit` → PASS. Then `cargo test` → all pass.

### Task 3: `D` deletes, with confirmation

**Files:**
- Modify: `src/ui/image.rs` (`Overlays` to `u16`, plus `DELETE_CONFIRM = 1 << 8`), `src/ui/mod.rs`, `src/ui/overlays.rs` (`render_delete_confirm`, constant `DELETE_CONFIRM_W`), `src/app/state/playlist_edit.rs`, `src/app/mod.rs` (field), `src/app/playlist_edit.rs`, `src/events/overlays.rs`, `src/events/mod.rs` (routing and tests), `src/events/global.rs`, `src/app/state/keybinds.rs`, `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1's request and response, and Task 2's `own_playlist_under_cursor`.
- Produces:
  - `App.delete_confirm: Option<Playlist>`, which also serves as the overlay's open state.
  - `App::open_delete_confirm(&mut self)`, `App::confirm_delete(&mut self)` and `App::on_playlist_deleted(&mut self, playlist: Playlist)`.
  - `handle_delete_confirm_input(app, key)` in `src/events/overlays.rs`.

- [ ] **Step 1: Write the failing tests.** In `playlist_edit.rs`:
  - `d_asks_first_and_deletes_only_on_y`: owned `p` is selected on the Playlists tab. `open_delete_confirm()` sets `delete_confirm` to `p` and sends nothing. `confirm_delete()` sends exactly `[DeletePlaylist { playlist.uuid == "p" }]` and clears `delete_confirm`. The library still holds `p` until `on_playlist_deleted(p)`. After that, `p` is gone from `playlists`, `favorite_playlist_ids` and `owned_playlist_ids`, and the status contains `Deleted`.
  - `deleting_a_playlist_closes_every_view_of_it_and_nothing_else`: the view stack is `[PlaylistDetail(p), ArtistDetail]`, plus a `PlaylistDetail(q)` at the bottom. After `on_playlist_deleted(p)`, the stack is `[PlaylistDetail(q), ArtistDetail]`.
  - `d_on_someone_elses_playlist_asks_nothing`: not owned, so `delete_confirm` stays `None` and the exact toast shows.

  In `events/mod.rs`:
  - `the_delete_confirmation_ignores_every_key_but_y_n_and_esc`: with the confirmation open, pressing `q`, `d` and `j` sends nothing, leaves `should_quit` false and keeps it open. `n` closes it. Reopened, Esc closes it. Reopened, `y` sends `DeletePlaylist`.
  - `shift_r_and_shift_d_never_fall_through_to_radio_or_library_removal`: on the Playlists tab with owned `p` selected, `KeyEvent::new(KeyCode::Char('d'), SHIFT)` opens the confirmation and sends no `RemovePlaylist`. On the Favorites tab, Shift+r spelled as `r`+SHIFT sends no radio request.
- [ ] **Step 2: Run them.** `cargo test playlist_edit` and `cargo test events` → Expected: compile errors.
- [ ] **Step 3: Implement.**
  - `on_playlist_deleted`: `remove_where` on `playlists`, remove the uuid from both id sets, `view_stack.retain` every view that is not a `PlaylistDetail` of that uuid, then toast `Deleted '<title>'`.
  - The confirmation modal sits in the middle of the screen, about 3 lines tall, and shows the exact copy above with a dimmed `y delete · n cancel` line.
  - Route `handle_delete_confirm_input` in `handle_key` right after the playlist picker: after `vim_arrows`, ahead of fullscreen art and queue focus.
  - In `global.rs`, add `KeyCode::Char('D') | KeyCode::Char('d') if SHIFT => app.open_delete_confirm()` beside the `R` arm, also with no context guard.
  - Widen `Overlays` to `u16`, add the flag, and render the modal.
  - Help: `Keybind { key: "D", action: "Delete playlist (your own)" }`, plus a README row and a CHANGELOG bullet merged with the rename one.
- [ ] **Step 4: Run them.** `cargo test` → all pass. Then `cargo fmt --check` should be clean, and `cargo clippy --all-targets` warnings should match master.

### Task 4: Hand-off

- [ ] **Step 1:** Stop and ask the user for a TUI pass:
  1. Create a playlist with `N`.
  2. `R` on it in the tab: the prompt should be prefilled. Rename it, and the tab should update and re-sort.
  3. Open it and press `R` again: the header should update.
  4. `R`, then Enter without changes: nothing should happen.
  5. `D` in its view: the confirmation should show. `q` and `j` should do nothing, and `n` should cancel.
  6. `D` again, then `y`: the view should close and the playlist should disappear from the tab.
  7. `R` and `D` on a Tidal-curated playlist should show the toast.
  8. Confirm in the official app that the playlist is gone.

  Do not commit, or ask to commit, until they confirm.
