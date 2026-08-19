# Agents Context for Riptide

This file documents conventions, patterns, and preferences for working on the Riptide music player codebase.

## Project Overview

**Riptide** is a terminal-based music player for Tidal with a TUI interface built in Rust. It's undergoing an API modernization effort to migrate from Tidal v1 API to v2 API endpoints.

**Key Technologies:**
- Language: Rust
- TUI Framework: ratatui
- API: Tidal (v1 and v2)
- Player: mpv (via FFI)
- Build: cargo, flake.nix (Nix support), AUR

## Comments: why, not what
Comments must explain *why* a non-obvious choice exists, never *what* the code literally does. The code is the what; the comment is the otherwise undeducible rationale.

Do NOT write inline comments that paraphrase the next line (`// header`, `// increment`, `// build left spans`, `// clear query`). If the code is unclear, rename the variable/function or extract a helper instead.
- Do NOT add decorative section banners inside functions (`// ── Search bar ──`, `// ── Divider ──`). Function structure and names already convey sections.
- Do NOT narrate a change you just made (`// now uses v2 API`, `// added fallback`). Git history is the changelog.
- Prefer a well-named constant to a magic-number comment: `const HELP_QUERY_MAX: usize = 48` beats `// cap at 48 to avoid modal overflow`.
- Keep `///` doc comments only for public/exported API where the contract is non-obvious. Private helpers get docs only if the invariant cannot be expressed by the signature/name.
- Keep only comments that explain non-obvious rationale, invariants, constraints, workarounds, or `// SAFETY:` justifications. When in doubt, delete the comment, a redundant comment is worse than no comment because it drifts and misleads.
### Current Status

**The v1 → v2 migration is complete.** Every endpoint that can move has moved:

- Favorites (albums, tracks) → ✅ v2 API
- Follow/unfollow (artists) → ✅ v2 API
- Search → ✅ v2 API
- Playlists → ✅ v2 API
- Albums → ✅ v2 API
- Artists → ✅ v2 API (incl. top tracks, albums, EPs, singles, bio)
- Radio (track + artist) → ✅ v2 API
- Lyrics → ✅ v2 API
- **Stream URLs → ⛔ stays on v1 permanently (see below)**

### Stream URLs Stay on v1 — Do Not Re-Attempt

`get_stream_url` is the only remaining v1 caller and it is **not** technical
debt. v2 cannot serve playable audio to this client.

Verified against track 431291038 on a live subscriber token:

- **v1** `playbackinfopostpaywall` → BTS manifest with
  `{"codecs":"flac","encryptionType":"NONE","urls":[...]}` — a plain HTTPS URL
  to an unencrypted FLAC that mpv plays directly.
- **v2** `GET /trackManifests/{id}` → only accepts `manifestType` of `HLS` or
  `MPEG_DASH` (no BTS). Every combination of `formats` / `usage` / `adaptive`
  returns CENC-`cbcs` encrypted content: FairPlay (`skd://` initData) on HLS,
  Widevine + PlayReady PSSH boxes embedded in the `.mpd` on DASH. The
  `"initData": null` in the DASH JSON is a red herring — the real init data is
  inside the manifest body.

Decrypting that requires a CDM, which mpv/ffmpeg do not have. TIDAL's Player
SDK solves it by delegating to a browser's EME stack (`shaka-player`,
`fairplay-drm.ts`), so adopting it would mean embedding a browser engine and
dropping mpv entirely — and third-party apps on that path get 30-second
previews unless the client ID is entitled. Ours is not: `/trackFiles/{id}`
returns `403 CLIENT_NOT_ENTITLED`.

There is also nothing to gain: v1 already returns the loudness data v2
advertises (`albumReplayGain`, `trackReplayGain`, both peak amplitudes).

**Consequence:** `const BASE`, `dash_to_hls`, `build_flac_m3u8`, and the
localhost manifest server in `src/manifest.rs` are load-bearing. Do not remove
them as "dead v1 code." Full write-up is in the doc comment on
`get_stream_url` in `src/api/client.rs`.

### Hi-Res Playback Is Unreachable — Do Not Re-Attempt

A `MAX` badge means the release exists in hi-res in Tidal's catalogue. It does not
mean riptide can be served it. Three walls, each verified against the live API:

1. **The account is not the limit.** `/users/{id}/subscription` reports
   `highestSoundQuality: HI_RES` on a subscriber account, yet requesting
   `audioquality=HI_RES_LOSSLESS` for a `HIRES_LOSSLESS` track returns
   `audioQuality=LOSSLESS, bitDepth=16, sampleRate=44100`. The built-in client is
   capped at lossless.
2. **Developer-portal credentials cannot log in.** `device_authorization` answers
   `400 {"error_description":"Client is not a Limited Input Device client"}`. That
   grant is restricted to TV/console/automotive clients, which is why the default
   client id is an extracted Android Automotive one.
3. **Portal tokens cannot stream even so.** A user token obtained through the
   portal's authorization-code flow (as the Swagger console does) gets `401` from
   `/tracks/{id}/playbackinfopostpaywall`. Implementing that flow would buy a login
   and no audio.

`config.client_id` / `client_secret` remain useful: swapping in a *different
extracted Limited Input Device* client works, and is the only route if a hi-res
entitled one is ever found. They are not for portal credentials — `auth_error` in
`src/api/auth.rs` detects that case and says so.

The `QUALITIES` order in `streaming.rs` (`LOSSLESS` first, so `HI_RES_LOSSLESS` is
never reached) is therefore moot, not a bug: reaching it returns the same 16/44.1.

`now_playing.delivered` carries the `bitDepth`/`sampleRate` the server actually
returned, and the now-playing bar shows them. That is the honest counterpart to
the badge — do not infer bit depth from mpv, which reports the decoder's output
format (24-bit FLAC decodes to `s32`).

### Do NOT Attempt Large Refactors on Long-Lived Branches

**Why:** Previous attempt to migrate multiple endpoints on a feature branch that diverged from master resulted in cascading conflicts during rebase/merge attempts. Structural changes to shared types (StatefulList, Config) on different branches are nearly impossible to reconcile.

**Better Approach:**
1. Start from **latest master**
2. Migrate **one endpoint at a time** in self-contained commits
3. Test and merge back to master **immediately**
4. Repeat for next endpoint
5. This keeps branches short-lived and merges clean

### API v2 Migration Patterns

**POST to collection (add favorite/follow):**
```rust
let body = serde_json::json!({"data": [{"id": id.to_string(), "type": "type_name"}]});
self.post_openapi_json("/userCollection{Type}/me/relationships/items", &body).await
```

**DELETE from collection (remove favorite/unfollow):**
```rust
let body = serde_json::json!({"data": [{"id": id.to_string(), "type": "type_name"}]});
self.delete_openapi_json("/userCollection{Type}/me/relationships/items", &body).await
```

**Key Constants Required:**
- `pub const OPENAPI_BASE: &str = "https://openapi.tidal.com/v2";`

### Pagination

**CRITICAL: Tidal API v2 ignores `page[size]`, `limit` and `offset`.**

Verified live against `/playlists/{id}`: `offset=0`, `offset=20` and no offset
at all return byte-identical pages. The server picks the batch size; the only
way to advance is the cursor.

- Uses cursor-based pagination with `page[cursor]` parameter
- Response includes `nextCursor` in meta/links for subsequent pages
- Always include `include` parameters on pagination requests to get full objects
- Never add a `limit`/`offset` argument to a request type — it cannot do anything

### Everything Loads Upfront — There Is No Lazy Loading

Lists are fetched in full before they reach the UI. Two mechanisms do this:

1. **Client drains internally** — `while let Some(url) = next_url` inside the
   client method, returning the whole collection in one `ApiResponse`. Used by
   favourites, artists, playlists, artist detail (top tracks/albums/EPs/singles),
   album tracks. These requests carry no cursor, so **a second request refetches
   everything from the start** — the response handler must set
   `exhausted = true`.
2. **Handler re-fires the next page** — `ApiResponse` handlers for fav albums,
   playlist detail and search immediately request the next cursor page until
   exhausted, without waiting for the user to scroll.

**The v2 collection endpoints report no total.** A response carries `data`,
`included` and `links` only — verified live against `/userCollectionAlbums/me`
and `/userCollectionPlaylists/me`. So `links.next` is the *only* end-of-collection
signal, and `total` can only ever mean "what has arrived so far". Use
`StatefulList::append_page`, never `append` with a total synthesised from the page
length: `items.len() >= total` is then true immediately and paging stops after one
page. `links.next` is also a **path, not a URL** — resolve it with `absolute_url`.

Scroll-triggered loading was removed: `should_load_more()`, `check_load_more()`
and `StatefulList::next_offset` no longer exist. Do not reintroduce them.
`exhausted` means "the app must not request more", not "the API has no more".

## Git & Commit Preferences

### User Preference: No Auto-Commits
- **DO NOT create commits** unless explicitly asked
- Let user handle all versioning and commit messages
- Exception: Only commit if user says "commit this" or similar explicit request

### Commit Message Format
- Omit `Co-Authored-By: Claude` trailer (user preference)
- Use clear, verbose, imperative messages following project style — single-line title only, no body
- Reference issue numbers when applicable

### Branch Strategy for Changes
- For bug fixes / small changes: work on feature branch, test, then present for commit
- For large features: use incremental approach (see API Modernization section above)
- Always sync with master before major work via rebase or merge

### Staging a Release
One commit (`chore: release vX.Y.Z`) bumps `Cargo.toml`, `Cargo.lock` and adds the
`CHANGELOG.md` section. Then run **`./scripts/sync-spec.sh`**, which derives
`riptide.spec`'s `Version` and `%changelog` from those two files, and include it
in the same commit.

The spec must be correct *before* the tag: COPR builds it as committed in git, not
from the tag payload, so a spec updated during the release workflow would ship the
wrong version. CI fails the lint job if the spec and `Cargo.toml` disagree.

Do not tag or push — a `v*` tag fires the public release workflow, and that is the
maintainer's call.

### File a PR
Before filling, check whether a PR for this branch already exists. Review diff locally against 'origin/master' to make sure its contents mach the goal.

PR titles usually become commit messages, so follow the repository's title conventions. Look at recently merged PRs and Git history for examples.
Prefer a concise, human-readable title that explains why the change matters:

BAD
> ❌ perf(server): negotiate permessage-deflate on the websocket

GOOD
> ✅ perf(server): cut websocket frame size by 70%+ with gzipping

Open the description with a simple explanation of the problem based on the user's original prompt, then briefly explain the solution. Do not lead with an implementation inventory: 

BAD
> ❌ Removed implicit workspace carry-over from every "new thread" entry point (cmd+n / cmd+shift+o, sidebar v1/v2 buttons, command palette). New threads inherit only the project from context; branch, worktree, and env mode always come from the configured defaults. Deleted buildContextualThreadOptions, startNewThreadInProjectFromContext, and the v1 sidebar's seed-context machinery.

GOOD
> ✅ My "new worktree" default was ignored when starting new threads on existing worktrees. Super unintuitive. Now your preferences always apply.

## Code Style & Conventions

### File Organization
- API functions: `src/api/client.rs`
- Data models: `src/api/models.rs`
- State management: `src/app/state.rs`
- UI rendering: `src/ui.rs`
- Event handling: `src/app/responses.rs`

### Error Handling
- Use `Result<T>` with `anyhow::Context` for error propagation
- Add debug logs at API boundaries for troubleshooting

### Logging

The default level is `warn`; `RIPTIDE_LOG_LEVEL` (or `RUST_LOG`) raises it. A bare
level is scoped to the crate (`riptide=debug`) so dependencies stay quiet — hyper's
connection-pool chatter was a quarter of the lines in a real bug report. A full
directive (`riptide=debug,hyper=info`) is honoured as written.

Choose the level by who needs the line:

- `info!` — session lifecycle, readable on its own: startup, auth, what loaded and
  how many, and each track as it starts playing.
- `debug!` — one line per API request or parsed page, carrying counts and ids.
- `warn!` — anomalies that would explain a bug report: duplicate entries,
  references missing from a response body, mpv disagreeing with the queue.
- `error!` — failed requests, with status and a body snippet.

**Never log inside a per-item loop.** `Included object type: {}` fired once per
JSON object and was 36% of one user's log. Aggregate into a single summary line,
and `warn!` with a count when items were dropped. Do not pair "sending request"
with "got response", and never dump a whole `Vec` of ids — one line stating the
outcome and its counts replaces all of it.

The point is a log someone can actually read: issue #43 went undiagnosed for days
because the evidence (one track fetched 88 times) was buried in noise and no
message named the anomaly.

### Pagination in Lists
- Use `StatefulList<T>` with `pagination_cursor: Option<String>` field
- Let the server choose the batch size; never pass a count
- Chain the next page from the response handler, not from a scroll event

### UI Patterns
- Use `ListViewport` for scroll management (interior mutability with Cell)
- Render functions receive `&Frame` for double-buffering
- Use ratatui's Layout/Constraint system for responsive design

## Testing Approach

- Focus on API parsing (serialize/deserialize JSON responses)
- Test pagination cursor flow
- Manual testing in terminal is primary validation for TUI features
- No mocking of database/API for critical integration tests

## Things to Avoid

### Don't:
1. **Create large feature branches** - They diverge from master and cause merge hell
2. **Assume page[size] works** - Tidal v2 only supports cursor-based pagination
3. **Omit include parameters on pagination** - Subsequent pages will return IDs only
4. **Add unnecessary error handling** - Trust framework/API guarantees at internal boundaries
5. **Remove unused code speculatively** - Delete only when certain it's unused
6. **Add feature flags for backwards compatibility** - Just change the code
7. **Mock external APIs in critical tests** - Use real API responses
8. **Add half-finished implementations** - Complete the feature or don't commit
9. **Create duplicated tests. Tests are good! endless smoke tests, "regression tests" for feature deletions, etc, much less good**

### Do:
1. **Start fresh from master** for each new API migration
2. **Test in the TUI** before considering work done
3. **Add debug logs** at API boundaries
4. **Keep pagination cursor in responses** for subsequent page requests
5. **Use JSON:API format** for v2 API payloads (`{"data": [...]}`)
6. **Verify build succeeds** before submitting work
7. **Apply the YAGNI concept from the ExtremeProgramming book**
8. **Try to reduce complexity when solving problems**
9. **Tests should be focused, not slop (see Things to Avoid > Don't > 9.)**
10. **Keep comments up to date! When making changes, it's important to keep things in sync**

## Troubleshooting Common Issues

**"Pagination not working / only 20 items load"**
- Check if `page[size]` parameter is being used (remove it)
- Verify `page[cursor]` is being used instead
- Ensure cursor value is being passed to next request

**"Newly favorited item doesn't show cover art"**
- Check if track object includes full album data
- Verify `include=albums,artists` parameters are present in requests
- Track objects from favorites may need album data refresh

**"Merge conflicts during rebase"**
- **Stop and use incremental approach instead**
- Rebase large branches only if unavoidable
- Better: merge and resolve conflicts in one go, then fix any issues

## Architecture Notes

### Interior Mutability Patterns
- `RwLock<String>` for token management (async-safe)
- `Cell<usize>` for scroll offset tracking in ListViewport (single-threaded)

### State Management
- StatefulList manages both UI selection and API pagination state
- `pagination_cursor` carries the API cursor; there is no offset counterpart
- Views on stack (ArtistDetail, PlaylistDetail, AlbumDetail) maintain independent state

### API Response Handling
- Parse JSON:API format into domain models
- Extract relationships and included objects manually
- Build maps for efficient lookups during transformation

### mpv's Playlist Is Not the Queue

The app owns `now_playing.queue`; mpv holds only the current track plus one
prefetched next (`--prefetch-playlist=yes` gives gapless playback). Two mpv
behaviours make this harder than it looks, both verified against a live mpv:

1. **The playlist is append-only.** Finished entries are never removed — only
   `playlist-pos` advances. After `k` self-advances it holds `k + 2` entries with
   `playlist-pos == k`, so any *absolute* `playlist-remove` index computed as if
   the current track were at 0 will hit the wrong entry — and removing the playing
   entry makes mpv jump to the next one while reporting `end-file` with
   `reason: "stop"`, which the read loop ignores (only `"eof"` advances the queue).
2. **A file appended to an exhausted playlist is *played*, not queued.** If mpv
   has run off the end, `loadfile … append` starts it immediately.

The design that satisfies both:

- **`PlayerCmd::SetNext`** is the only way to queue a track. It sends
  `playlist-clear` (keeps just the playing entry, so it cannot touch what is
  playing and needs no index arithmetic) followed by `loadfile … append`, as one
  burst so the playlist is never left empty behind the current track. It also
  keeps the playlist from growing without bound.
- **Never clear mpv's queued entry before the replacement URL is in hand.**
  `App::replace_prefetched_next()` only *requests* the URL; the swap happens in the
  `StreamUrl` handler. Clearing eagerly leaves the playlist empty for a whole
  round-trip, and a track ending in that window turns the late prefetch into
  hijacked playback (bug 2 above).
- **`now_playing.mpv_exhausted`** gates every `SetNext`. Nothing may be appended
  once mpv has run out of playlist.
- **`now_playing.next_prefetched`** records what mpv actually holds.
  `PlayerEvent::TrackEnded` compares it against `queue[queue_index + 1]` and
  replays the track instead of advancing blindly when they disagree.
- **`PlayerCmd::Play`** (`loadfile replace`) is the resync point — it resets the
  playlist to one entry at position 0. Explicit actions (next/prev/play-from-queue)
  all route through it, which is why they never desync, and why bugs here only
  reproduce while tracks advance on their own.

`src/app/playback.rs` tests carry a `FakeMpv` modelling all of the above; the
`assert_in_sync` invariant (what mpv plays is what Now Playing shows) is the thing
to preserve. Some of those tests depend on shuffle order — run the suite repeatedly
when changing this area.

### Filtering Library Lists

`StatefulList::selected` indexes the **visible** rows, not `items`. With a filter
active they differ, and `matches` holds positions into `items`, so **anything that
mutates `items` directly must call `refilter()`** or the indices go stale. Use
`remove_where` for removals — it handles `total`, the refilter and the clamp.
An empty filter is a fast path that reads `items` directly, so unfiltered lists
(every detail view) cost nothing.

Reach rows through `selected_item()` / `get_visible()` / `visible_window()`, never
`items[selected]` — that pairing is what makes a filtered list act on the wrong row.

## Remaining V1 API Usage & Refactoring Opportunities

### Complete V1 API Usage Inventory

**Exactly one v1 caller remains, and it is intentional:**

| Function | Endpoint | File | Status |
|----------|----------|------|--------|
| get_stream_url | `/tracks/{id}/playbackinfopostpaywall` | client.rs:3022 | **v1 permanently** — v2 is DRM-encrypted, see "Stream URLs Stay on v1" above |

Everything else now targets openapi.tidal.com/v2. The v1 helpers that used to
back the migrated endpoints (`get`, `post_form`, `delete`, `uid`) have been
deleted; `const BASE` survives solely for `get_stream_url`.

### Pagination Refactoring — Done

The duplicated scroll-triggered loading this section used to describe is gone.
`should_load_more()`, `check_load_more()`, `next_offset` and the per-list
`load_more_*` helpers were deleted once every list moved to upfront loading;
`ApiRequest` variants no longer carry `offset`. See "Everything Loads Upfront"
above before adding anything back here.

### Code Duplication & Standardization Opportunities

**1. Query Parameter Building (5+ instances)**
- Files: `client.rs` in `get()` `:1242`, `get_openapi()` `:1298`, `post_form()` `:1385`, `delete()` `:2271`, `search_*` `:1917` etc.
- Pattern: Build `all_params` vec, add countryCode, optionally sessionId, extend with params
- **Opportunity:** Extract to `build_api_params()` helper

**2. Error Response Parsing (2 instances)**
- Files: `client.rs` `get()` `:1282-1290` (300 char snippet), `get_openapi()` `:1308-1314` (400 char snippet)
- Pattern: Log error with body snippet, return formatted error
- **Opportunity:** Extract to `parse_error_response()` helper

**3. Favorite/Collection Management — Done**
- All four removal sites now call `StatefulList::remove_where`, which owns the
  `.retain()`, the `total` decrement, the refilter and the selection clamp.
  Removing an item from a library list any other way will desync a filtered list.

**4. Duplicate Deduplication (2 instances)**
- Files: `responses.rs` `:20-28` (fav_albums dedup via HashSet), `:66-79` (favorites/playlists dedup)
- Pattern: Filter out items already in collection using HashSet
- **Opportunity:** Extract to `deduplicate_by_id()` helper

**5. UI List Item Rendering (75+ instances)**
- Files: `src/ui.rs` in `render_home_section`, `render_artist_list`, `render_fav_albums_list`, `render_artist_tracks_full`, `render_artist_albums` etc.
- Anchor: `grep -rn "ListItem::new" src/ui.rs` or `grep -n "fn render_" src/ui.rs`
- Pattern: Calculate visibility (`visible_items`, `ListViewport`), build `ListItem::new(Line::from(...))` with prefix + title + badge, apply styles
- **Opportunity:** Extract to `create_list_item()` or styling component helper

**6. Data Filtering & Transformation (8 instances)**
- Files: `src/app/responses.rs` near `ApiResponse::FavAlbumsPage` / `ApiResponse::Playlists` dedup
- Anchor: `grep -n "HashSet\|\.retain\|\.iter()\.map" src/app/responses.rs`
- Pattern: `.iter().map()`, `.filter()`, `.collect()` with custom predicates
- **Opportunity:** Extract filter predicates to reusable functions

**7. Queue Extension (2 instances)**
- Files: `src/app/responses.rs` (`source_playlist_*` handling), `src/app/playback.rs` (`toggle_shuffle`, `queue.shuffle`)
- Anchor: `grep -n "shuffle\|source_playlist" src/app/playback.rs src/app/responses.rs`
- Pattern: Extend queue with shuffle handling, track source/offset
- **Opportunity:** Extract to `extend_queue_with_shuffle()` helper

### Refactoring Priority Matrix

| Helper | Impact | Effort | Files Affected | Lines Saved | Priority |
|--------|--------|--------|-----------------|------------|----------|
| Query Param Builder | High | Low | client.rs (4 places) | 60+ | **P0** |
| Error Parser | Medium | Low | client.rs (2 places) | 20+ | P1 |
| ~~Collection Manager~~ | — | — | done: `StatefulList::remove_where` | 30+ | — |
| Deduplication | Medium | Low | responses.rs (2 places) | 15+ | P1 |
| UI List Item | High | High | ui.rs (75+ places) | 200+ | P2 |
| Queue Extension | Low | Medium | responses.rs, playback.rs | 25+ | P2 |

### Recommended Refactoring Sequence

1. **Phase 1 (Quick Wins):** Query param builder + Error parser (P0 items)
   - Low effort, high code clarity improvement
   - Estimated: 2-3 hours

2. **Phase 2 (Core Stability):** Collection manager + deduplication
   - Medium effort, eliminates major duplication
   - Estimated: 4-5 hours

3. **Phase 3 (Polish):** UI rendering helper (if time permits)
   - High effort, significant code reduction
   - Estimated: 6-8 hours

## Related Documentation

See project memory for detailed notes:
- `api_refactor_lessons.md` - Lessons from v1→v2 migration attempts
- `api_v2_patterns.md` - Working patterns for v2 API
- `pagination_strategy.md` - Consistent pagination approach
- `tidal_api_pagination.md` - Tidal API specifics
- `logging_strategy.md` - Debug logging guidelines
