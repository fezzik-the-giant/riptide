# Playlist Editing — Design

**Issue:** [#57](https://github.com/fezzik-the-giant/riptide/issues/57) — create
playlists, and add, remove and reorder their tracks.

## Goal

A track can be favorited but not filed. After this work a user can, entirely from
the TUI:

- create a playlist, rename it, and delete it
- add any track to one of their own playlists
- remove a track from one of their own playlists
- reorder the tracks of one of their own playlists
- tell at a glance, in the Playlists tab, which playlists are theirs and which
  are Tidal-curated, other users', or mixes

Editing is limited to playlists the user owns. Changing a playlist's visibility
(public / unlisted) is out of scope.

## Verified API

Every request below was exercised against the live v2 API on a subscriber
account (2026-10-15 and 2026-10-16) with throwaway playlists that were deleted
afterwards. Nothing here is taken from the spec alone.

| Operation | Request | Result |
|---|---|---|
| Owned playlists | `GET /playlists?filter[owners.id]=me` | Exactly the user's own playlists. Saved editorial and other-user playlists are excluded. |
| Create | `POST /playlists` `{"data":{"type":"playlists","attributes":{"name","description","accessType"}}}` | `201`, body carries id, attributes, `playlistType: USER`. The new playlist is **added to the user's collection automatically**. |
| Rename | `PATCH /playlists/{id}` `{"data":{"id","type":"playlists","attributes":{"name"}}}` | `200` with the updated playlist. Omitted attributes are left unchanged. |
| Delete | `DELETE /playlists/{id}` | `200`. Afterwards `404`, and gone from the collection. |
| Add | `POST /playlists/{id}/relationships/items` `{"data":[{"id","type":"tracks"}, …]}` | `200`. Each added entry is echoed with `meta.itemId` and `meta.addedAt`. Batch adds work. |
| Add, rejected track | same | Still `200`; the track appears in `meta.skipped` with a `reason` (observed: `NOT_FOUND` for track 189681882 in country `US`). |
| Duplicates | add the same track twice | Two entries, distinct `itemId`s, `skipped` empty. |
| Remove | `DELETE /playlists/{id}/relationships/items` `{"data":[{"id","type":"tracks","meta":{"itemId"}}]}` | `200`, removes exactly that entry. **Without `meta.itemId`: `400 Must not be null` at `data/0/meta`.** |
| Move | `PATCH /playlists/{id}/relationships/items` `{"data":[{"id","type":"tracks","meta":{"itemId"}}],"meta":{"positionBefore":"<itemId>"}}` | `200`, entry now sits immediately before `positionBefore`. |
| Move to end | same, `meta.positionBefore` omitted | Entry moves to the last position. |

Facts that do **not** work and must not be relied on:

- A playlist's `owners` relationship is empty even with `include=owners`.
- `playlistType: USER` does not imply ownership — other users' playlists carry it too.

The collection request (`/userCollectionPlaylists/me/relationships/items?include=items`)
already returns `playlistType` and `accessType` for every playlist, so the badge
costs only the one extra owned-playlists request.

## Design

### API layer — `src/api/client/playlists.rs`

New `ApiClient` methods, one per verified request:

- `create_playlist(name) -> Playlist` — always `accessType: UNLISTED`, the safe
  default when there is no setting for it.
- `rename_playlist(id, name)`
- `delete_playlist(id)`
- `add_to_playlist(id, track_id) -> AddOutcome` — reads `meta.skipped`; returns
  `Added { item_id }` or `Skipped { reason }`. A `200` alone is never treated as
  success.
- `remove_from_playlist(id, item_id)`
- `move_in_playlist(id, item_id, before: Option<item_id>)` — `None` moves to the end.
- `get_owned_playlist_ids() -> HashSet<String>` — drains the owner-filter request.

`post_openapi_json` / `delete_openapi_json` in `mod.rs` discard the response body.
Body-returning variants are added beside them, plus `patch_openapi_json`. All of
them resolve the token through `access_token()` so renewal still applies.

Logging per AGENTS.md: one `debug!` per write with ids; `warn!` when tracks are
skipped, with the reason; `error!` on failure with status and body snippet.

### Models

- `Playlist` gains `playlist_type: Option<String>` (the raw `playlistType`) and a
  method `kind(&owned) -> PlaylistKind` — `Yours | Tidal | User | Mix | Other`.
  Ownership is checked first, then the type. It is computed at render time, not
  stored, because the playlists and the owned set arrive in separate responses
  in either order; a stored value would be stale whenever ownership lands second.
  Unknown types (e.g. `ARTIST`, not seen in the test library) map to `Other`.
- `Track` gains `playlist_item_id: Option<String>`, `None` except on rows loaded
  from a playlist. It lives on `Track` rather than in a parallel structure because
  playlist rows pass through filtering, Atmos narrowing, `remove_where` and queue
  copies; a field cannot drift out of step with its row.
- `parse_v2_playlist_tracks` and `parse_playlist_relationship_items` read
  `meta.itemId` into that field.

### App state and data flow

- New `ApiRequest`s: `CreatePlaylist { name, then_add: Option<Track> }`,
  `RenamePlaylist`, `DeletePlaylist`, `AddToPlaylist`, `RemoveFromPlaylist`,
  `MovePlaylistItem`, `LoadOwnedPlaylists`, each with a success `ApiResponse`.
  Failures use the existing `ApiResponse::Error(String)`, which already toasts;
  the message names the action and carries Tidal's `errors[0].detail`.
- `LoadOwnedPlaylists` is sent alongside `LoadPlaylists`; `App` keeps
  `owned_playlist_ids`, which `kind` reads. If it fails, nothing is treated
  as owned: edit keys show their "only your own playlists" toast, and an `error!`
  is logged.
- **Apply on confirm.** Create, rename, delete, add and remove change local state
  only when the success response arrives. These edits cannot be undone, so a
  wrong local state after a failed request is worse than ~200 ms of latency.
- **Move is the exception.** Move mode reorders locally while the user moves the
  row (no request), sends one `MovePlaylistItem` on drop, and restores the row to
  its original index if that request fails.
- Response effects:
  - create → insert into `playlists` and `owned_playlist_ids`; with `then_add`,
    send `AddToPlaylist` straight away.
  - add → if that playlist's detail view is open, append the row with its new
    `itemId`. Toast "Added to '…'", or "Couldn't add: <reason>" when skipped.
  - remove → `remove_where` on the detail list, matched by `itemId`.
  - rename → update the title in `playlists` and in any open detail view.
  - delete → remove from `playlists` and `owned_playlist_ids`; pop the detail view
    if it is the deleted playlist.
- The play queue is never modified by an edit: what is queued is what plays.
- The picker reads `playlists` filtered to `kind == Yours`; it makes no request.

### UI

- **Badge column.** `playlist_row` (`src/ui/lists.rs`) gains a `Fixed` column with
  the kind label: `Yours` / `Tidal` / `User` / `Mix`, blank for `Other`. `Yours` is
  drawn in the theme accent; the others are dimmed. The playlist detail title
  carries the same label.
- **Picker (`P`).** Modal modelled on `render_artist_selection_modal`. Title:
  *Add "<track>" to…*. Row 0 is `+ New playlist…`, then the user's playlists in the
  Playlists tab's sort order. `j`/`k`/`↑`/`↓` move, Enter picks, Esc closes. With
  no owned playlists only the `+ New playlist…` row is shown.
- **Name prompt (`N`, `R`, `+ New playlist…`).** Single-line input styled like the
  `:` command bar, titled "New playlist" or "Rename playlist" (prefilled with the
  current name). Input is trimmed; Enter on an empty name does nothing. No length
  validation — Tidal's limit is unverified, so a rejection surfaces as an error
  toast carrying the server's message.
- **Delete confirmation (`D`).** Modal: *Delete "<name>"? This can't be undone.*
  `y` deletes; `n` or Esc cancels; every other key is ignored.
- **Move mode (`m`).** The moving row is highlighted (reversed, `↕` marker); the
  footer reads `↑↓/jk move · Enter drop · Esc cancel`. All other keys are ignored
  until drop or cancel. Refused with a toast while the list is narrowed (filter
  query or hidden Atmos rows), because a drop next to an invisible row lands
  somewhere the user did not choose.
- **Feedback.** Every edit ends in a toast — the success message or the server's
  error.

### Keys

| Key | Context | Action |
|---|---|---|
| `P` | any track row (lists, detail views, search, queue) | Add to playlist… |
| `N` | Playlists tab | New playlist |
| `R` | own playlist (Playlists tab or its detail view) | Rename |
| `D` | own playlist (same) | Delete, with confirmation |
| `d` | track in own playlist detail | Remove from this playlist |
| `m` | track in own playlist detail | Move mode |

`d` on the Playlists tab keeps its existing meaning — remove from library,
undoable with `u` — for every playlist, owned or not. `R`, `D`, `d` (in detail)
and `m` on a playlist the user does not own show the toast "Only your own
playlists can be edited". All six go into a new "Playlists" group in the help
modal.

## Testing

- **Parsing**, using the probe response bodies as fixtures: create response;
  add with `meta.skipped` (track 189681882, `NOT_FOUND`); an items page holding
  one track twice with distinct `itemId`s. Assert `itemId` survives both playlist
  parsers and that a skipped track is never reported as added.
- **`PlaylistKind`**: one table-driven test over EDITORIAL / USER / MIX / unknown ×
  owned / not owned.
- **Move**: the drop-index → `positionBefore` mapping (middle → next row's
  `itemId`; last → `None`) and the revert on failure.
- **Manual TUI pass** before each PR on a throwaway playlist: create, add, add a
  duplicate, remove one copy, move to the middle and to the end, rename, delete.

## Delivery

Four short-lived PRs off latest master, each merged before the next branches.
#57 is the parent; each PR has its own sub-issue on the Riptide Development
board, linked to its branch, and closes it on merge.

1. **Ownership, create and add** — owned-playlists request, `PlaylistKind` and the
   badge column, `create_playlist` / `add_to_playlist`, the `P` picker, `N`,
   `+ New playlist…`. Adds the "Playlist Editing" section to `AGENTS.md` with the
   verified API facts above, since this is the first code that depends on them.
2. **Remove** — `playlist_item_id` on `Track`, both parsers, `d` in own playlist
   detail.
3. **Rename and delete** — `R`, `D` with confirmation.
4. **Reorder** — `m` move mode.

Each PR adds its `CHANGELOG.md` entry under `[Unreleased]`. When PR 4 merges, the
`ROADMAP.md` entry moves to `## Shipped` and #57 goes to `Done`.
