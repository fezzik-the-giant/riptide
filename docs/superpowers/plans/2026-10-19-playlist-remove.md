# Playlist Remove Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `d` on a track in one of your own playlists' detail view removes that one entry from the playlist on Tidal (#72, PR 2 of 4 for #57).

**Architecture:** Every track row loaded from a playlist carries its entry's `meta.itemId` in a new `Track::playlist_item_id`. `d` sends `RemoveFromPlaylist` with that id. The row disappears only when the success response arrives ("apply on confirm"), through `StatefulList::remove_where` matched by `itemId`. Rows that PR 1 appends after an add now store the `itemId` the add returned.

**Tech Stack:** Rust, ratatui/crossterm, reqwest, tokio worker (`ApiRequest`/`ApiResponse`).

**Spec:** `docs/superpowers/specs/2026-10-16-playlist-editing-design.md`

## Global Constraints

- Remove is `DELETE /playlists/{id}/relationships/items` with body `{"data":[{"id":"<trackId>","type":"tracks","meta":{"itemId":"<itemId>"}}]}`. Without `meta.itemId` Tidal answers `400 Must not be null`.
- Verified live 2026-10-19: both read endpoints carry `meta.itemId` on every item ref. `GET /playlists/{id}?include=items` has them at `data.relationships.items.data[].meta.itemId`; `GET /playlists/{id}/relationships/items` has them at `data[].meta.itemId`.
- Apply on confirm: local state changes only in the success response handler. The play queue is never modified by an edit.
- `d` on a playlist the user does not own shows exactly `Only your own playlists can be edited`.
- `d` on the Playlists tab itself keeps its existing meaning (remove from library, undoable with `u`).
- Failures use the existing `ApiResponse::Error` via `write_failed`, so the toast carries Tidal's `errors[0].detail`.
- Logging: one `debug!` per write with ids. `send_openapi_json` already logs `error!` on failure.
- Commits: none per task (user rule). One commit after the user's TUI test, single-line conventional message, no trailer.

## Review Focus

1. **Duplicate added from the playlist's own view.** `P` on a row of playlist X, adding to X itself: the appended row must carry the *new* `itemId`, not the source row's. Otherwise `d` on the new row deletes the original entry. Test: Task 3 `an_added_row_carries_its_own_item_id_not_the_source_rows`.
2. **Removing one copy of a duplicated track.** Only the entry with the matching `itemId` goes; the other copy stays. Test: Task 3 `removing_one_copy_of_a_duplicate_keeps_the_other`.
3. **Narrowed list.** With a filter query or hidden Atmos rows, `d` must act on the highlighted visible row (`selected_item()`), never `items[selected]`. Test: Task 3 `d_on_a_narrowed_list_removes_the_highlighted_entry`.
4. **Not owned, or ownership failed to load.** No request, and the exact toast. Test: Task 3 `d_in_someone_elses_playlist_sends_nothing_and_says_why`.
5. **Response lands after the view was closed.** The Playlists tab count still drops by one and nothing panics. Test: Task 3 `a_removal_confirmed_after_the_view_closed_still_fixes_the_count`.

---

### Task 1: `itemId` on every playlist row

**Files:**
- Modify: `src/api/models.rs` (`Track`)
- Modify: `src/api/client/playlists.rs` (`parse_v2_playlist_tracks`, `parse_playlist_relationship_items`, tests)
- Modify: every `Track { … }` literal (`grep -rn "Track {$" src`, 26 sites incl. `src/app/test_support.rs::track`) — add `playlist_item_id: None`

**Interfaces:**
- Produces: `Track::playlist_item_id: Option<String>`, declared `#[serde(default, skip_deserializing)]` like `added_at`. It is `None` except on rows parsed from a playlist.

- [ ] **Step 1: Write the failing tests** in `playlists.rs`'s test module. Build two fixtures from the live responses in `/tmp/claude-1000/-home-radahn-git-riptide/40922c8f-caee-4933-97ac-60b806554c69/scratchpad/parent.json` and `page.json`, trimmed the way the existing fixtures are. Each fixture holds track `431291038` twice, with the two `itemId`s from the 2026-10-15 duplicates probe: `e2df580f-28c7-49ed-950a-e19cb8d761bd` and `95eb7254-0e58-401a-82f5-163c2342821a`. The track is included once.

```rust
#[test]
fn both_playlist_parsers_keep_each_entry_s_item_id() {
    let (first, ..) = parse_v2_playlist_tracks(&json(PARENT_WITH_DUPLICATE)).unwrap();
    let (cursor, ..) = parse_playlist_relationship_items(&json(PAGE_WITH_DUPLICATE), 2).unwrap();
    for tracks in [first, cursor] {
        assert_eq!(tracks.iter().map(|t| t.id).collect::<Vec<_>>(), [431291038, 431291038]);
        assert_eq!(
            tracks.iter().map(|t| t.playlist_item_id.as_deref()).collect::<Vec<_>>(),
            [Some("e2df580f-28c7-49ed-950a-e19cb8d761bd"), Some("95eb7254-0e58-401a-82f5-163c2342821a")]
        );
    }
}
```

- [ ] **Step 2: Run it.** `cargo test both_playlist_parsers` → Expected: compile error, no field `playlist_item_id`.
- [ ] **Step 3: Implement.** Add the field and update all literals. In both parsers, collect `(track_id, item_id)` pairs from the item refs instead of bare ids. `item_id` is `item_ref["meta"]["itemId"].as_str()`. Set it on the pushed `Track`.
- [ ] **Step 4: Run it.** `cargo test both_playlist_parsers` → PASS. Then `cargo test` → all pass.

### Task 2: `remove_from_playlist` through the worker

**Files:**
- Modify: `src/api/client/playlists.rs`, `src/api/messages.rs`, `src/api/worker.rs`

**Interfaces:**
- Produces:
  - `fn remove_item_body(track_id: u64, item_id: &str) -> serde_json::Value`, private, in `playlists.rs`.
  - `ApiClient::remove_from_playlist(&self, uuid: &str, track_id: u64, item_id: &str) -> Result<()>`.
  - `ApiRequest::RemoveFromPlaylist { playlist: Playlist, track: Track, item_id: String }`.
  - `ApiResponse::RemovedFromPlaylist { playlist: Playlist, track: Track, item_id: String }`.

- [ ] **Step 1: Write the failing test** in `playlists.rs`:

```rust
/// Tidal rejects a remove without `meta.itemId` (`400 Must not be null`).
#[test]
fn a_remove_names_the_entry_by_item_id() {
    assert_eq!(
        remove_item_body(431291038, "e2df580f-28c7-49ed-950a-e19cb8d761bd"),
        serde_json::json!({"data": [{"id": "431291038", "type": "tracks",
            "meta": {"itemId": "e2df580f-28c7-49ed-950a-e19cb8d761bd"}}]})
    );
}
```

- [ ] **Step 2: Run it.** `cargo test a_remove_names_the_entry` → Expected: compile error, `remove_item_body` not found.
- [ ] **Step 3: Implement.** Add `remove_item_body`. `remove_from_playlist` sends it with `send_openapi_json(Method::DELETE, "/playlists/{uuid}/relationships/items", …)` and logs `debug!("removed item {item_id} (track {track_id}) from playlist {uuid}")`. Add the request/response variants. The worker arm answers `RemovedFromPlaylist` on success, or `write_failed(&format!("Couldn't remove from '{}'", playlist.title), &e)`.
- [ ] **Step 4: Run it.** `cargo test a_remove_names_the_entry` → PASS. The build will fail on the unhandled `RemovedFromPlaylist` in `responses.rs` until Task 3, so route it to a stub there that Task 3 fills in.

### Task 3: `d` in your own playlist, and the add fix

**Files:**
- Modify: `src/app/playlist_edit.rs` (logic and tests)
- Modify: `src/app/responses.rs` (route `RemovedFromPlaylist`)
- Modify: `src/events/navigation.rs` (`View::PlaylistDetail` arm, `Action`)
- Modify: `src/app/state/keybinds.rs` (Playlists group), `README.md` (Playlists table), `CHANGELOG.md` (`[Unreleased]` Added)

**Interfaces:**
- Consumes: Task 1's `playlist_item_id`, Task 2's request and response.
- Produces:
  - `App::remove_selected_from_playlist(&mut self)`: acts on the top view if it is a `PlaylistDetail` with Tracks focus.
  - `App::on_removed_from_playlist(&mut self, playlist: Playlist, track: Track, item_id: String)`.

- [ ] **Step 1: Write the failing tests** in `playlist_edit.rs`. Use the existing `own()`, `test_app()` and `track()` helpers. A row with an id is `Track { playlist_item_id: Some("a".into()), ..track(1) }`.
  - `an_added_row_carries_its_own_item_id_not_the_source_rows`: open own playlist `p` (exhausted) holding a row with item id `"a"`. Call `on_added_to_playlist(own("p"), <that row cloned>, Added { item_id: "b" })`. The appended row's `playlist_item_id` is `Some("b")`.
  - `d_removes_the_entry_only_once_tidal_confirms`: owned `p` open, rows `a`, `b`, cursor on `b`. Call `remove_selected_from_playlist()`. Then:
    - The requests are exactly `[RemoveFromPlaylist { item_id == "b", playlist.uuid == "p", .. }]`.
    - Both rows are still present.
    - After `on_removed_from_playlist(.., "b")`, only `a` remains, `playlists` entry `p` has `number_of_tracks` one lower, and the status contains `Removed`.
  - `removing_one_copy_of_a_duplicate_keeps_the_other`: two rows with the same `id` and item ids `a` and `b`. After `on_removed_from_playlist(.., "a")`, the single remaining row has item id `b`.
  - `d_on_a_narrowed_list_removes_the_highlighted_entry`: rows `a` and `b` with different titles, and a filter query that matches only `b`, selected at 0. The request's `item_id == "b"`.
  - `d_in_someone_elses_playlist_sends_nothing_and_says_why`: `p` is not in `owned_playlist_ids`. No requests, and the status is exactly `Only your own playlists can be edited`.
  - `a_removal_confirmed_after_the_view_closed_still_fixes_the_count`: no view open, `playlists` holds `p` with `Some(2)`. After `on_removed_from_playlist`, it reads `Some(1)`.
- [ ] **Step 2: Run them.** `cargo test playlist_edit` → Expected: compile errors for the two missing methods. The add test fails on `Some("a")` vs `Some("b")`.
- [ ] **Step 3: Implement.**
  - `on_added_to_playlist`: the pushed row gets `playlist_item_id = Some(item_id)` from the `Added` outcome.
  - `remove_selected_from_playlist`: do nothing unless the top view is a `PlaylistDetail` with Tracks focus. If not owned, show the exact toast above. Otherwise take `selected_item()`. If its `playlist_item_id` is `None`, show the Error toast `Tidal sent no entry id for '<title>', so it can't be removed` and send nothing. Otherwise send the request.
  - `on_removed_from_playlist`:
    - `remove_where(|t| t.playlist_item_id.as_deref() != Some(&item_id))` on every `PlaylistDetail` in the view stack for that uuid, then set its `playlist.number_of_tracks = Some(tracks.total)`.
    - Decrement `number_of_tracks` (saturating) on the `playlists` entry.
    - Toast `Removed '<track>' from '<playlist>'`.
  - Route the response in `responses.rs`.
  - Add `Action::RemoveFromPlaylist` and `KeyCode::Char('d')` to the `View::PlaylistDetail` arm, which dispatches to `app.remove_selected_from_playlist()`.
  - Help `Keybind { key: "d", action: "Remove track (own playlist view)" }`; matching README row.
  - CHANGELOG bullet under Added: `Press d on a track in one of your own playlists to remove that entry from it; with a duplicated track only the copy under the cursor goes (#72).`
- [ ] **Step 4: Run them.** `cargo test playlist_edit` → PASS. Then `cargo test`, `cargo fmt --check` and `cargo clippy --all-targets`. All tests should pass, fmt should be clean, and clippy's warnings should match master.

### Task 4: Hand-off

- [ ] **Step 1:** Stop and ask the user for a TUI pass on a throwaway playlist:
  1. Create it with `N`.
  2. Add a track, and add the same track again from the playlist's own view with `P`, to get two copies.
  3. Press `d` on the second copy. Only it should go, and the count should drop.
  4. Reopen the playlist. The remaining copy should still be there, and `d` should remove it too.
  5. Press `d` in a Tidal-curated playlist. It should toast `Only your own playlists can be edited`.
  6. Press `d` on the Playlists tab. It should still remove the playlist from the library, and `u` should still undo that.
  7. Press `d` in one of your own playlists that is longer than one page. The header's track count should drop by one, not reset to 0.
  8. Delete the throwaway playlist in the official app.

  Do not commit, or ask to commit, until they confirm.
