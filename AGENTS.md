# Agents Context for Riptide

This file documents conventions, patterns, and preferences for working on the Riptide music player codebase.

## Project Overview

**Riptide** is a terminal-based music player for Tidal with a TUI interface built in Rust.
The Tidal v1 → v2 API migration is complete; only `get_stream_url` remains on v1, permanently.

**Key Technologies:**
- Language: Rust
- TUI Framework: ratatui
- API: Tidal (v1 and v2)
- Player: mpv (via FFI)
- Build: cargo, flake.nix (Nix support), AUR

## Roadmap

`ROADMAP.md` holds the candidate features nobody has started yet, each with the
file reference that proves the gap and the questions still open on it. Read it
before proposing new work — an idea that looks obvious from the outside has
usually been weighed there already.

**Mark an entry the moment its feature lands.** Move it under `## Shipped` with
the version it went out in, or under `## Considered and Rejected` with the
reason it will not be built. An entry left sitting under `## Candidates` after
it ships gets proposed again a month later, and nothing in the file tells the
next session that the work is already done.

Every entry is also an issue on the **Riptide Development** board
(<https://github.com/users/fezzik-the-giant/projects/2>), and its status there
moves as the work does — `Ready` when a design is approved, `In progress` on
branching, `In review` on opening the PR, `Done` on merge. `ROADMAP.md` holds
the status table and the rest of the convention.

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
`get_stream_url` in `src/api/client/streaming.rs`.

### Access Tokens Live Four Hours — the Client Renews Them

Verified live against `auth.tidal.com`: the device flow and the refresh grant
both return `expires_in: 14400`. A session left open longer than that outlives
its credentials, which is what issue "No stream available for …" turned out to
be — the token was read once at startup and never renewed.

Two facts the renewal path depends on:

1. **The refresh grant does not rotate the refresh token.** The response carries
   `access_token`, `expires_in`, `scope`, `token_type` and `user` — no
   `refresh_token`. The one from the device flow stays valid, so renewal needs
   no config write, and `main`'s exit-time `save_config` cannot clobber it.
2. **The stored `session_id` survives renewal.** A new access token paired with
   the old `sessionId` resolves streams normally, so there is no need to re-fetch
   `/v1/sessions` on refresh.

`ApiClient::access_token` is the single choke point: every request resolves its
bearer token through it, and it renews under the write lock when within
`REFRESH_MARGIN` of expiry. The worker spawns each request, so a batch can cross
the boundary together — the write lock is what stops them all refreshing at once.
Do not reintroduce a bare `self.token.read()`; it would silently opt that
endpoint out of renewal.

**401 is not a quality tier falling through.** `get_stream_url`'s `QUALITIES`
chain treats `403` as "not entitled to this tier" and moves on, but a `401` is
the session, and every remaining tier fails identically. It bails immediately.
Falling through on it spent three requests to reach "no stream URL available for
track", which named the track and hid the only fact that mattered.

### Dolby Atmos Is Reachable — via `immersiveaudio`

Unlike hi-res, Atmos is not walled off. `playbackinfopostpaywall` takes an
`immersiveaudio` parameter, verified live against track 189681882
(`DOLBY_ATMOS`) and 431291038 (stereo):

| `immersiveaudio` | Atmos track                          | stereo track |
|------------------|--------------------------------------|--------------|
| `true`           | `audioMode=DOLBY_ATMOS`, BTS `eac3`  | unchanged    |
| `false`          | `audioMode=STEREO`, BTS `flac` 16/44 | unchanged    |
| omitted          | same as `true` — Atmos is the default | unchanged   |

Three consequences the code depends on:

1. **Absence is not `false`.** Omitting the parameter gives Atmos, so
   `get_stream_url` always sends it. Only the literal `true` enables Atmos —
   `False`, `0` and junk all read as off.
2. **It is inert on tracks with no Atmos mix**, so it needs no per-track check.
3. **The Atmos stream is `encryptionType: NONE`** — plain E-AC-3 that mpv plays
   directly. ffprobe reports `Dolby Digital Plus + Dolby Atmos`, 5.1, 48 kHz:
   ffmpeg decodes the core and ignores the JOC objects, so the setting buys real
   surround on a surround setup and a lossy downmix of an otherwise lossless
   release on two speakers. That trade is the user's to make, which is why
   `prefs.atmos` exists and defaults to off.

`BtsManifest::is_eac3()` accepts it on the tier it arrives on. Without that
branch it fails `is_flac()` and falls through the whole `QUALITIES` chain until
`HIGH` accepts the identical stream — three requests for one track.

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

**The first page and its cursor pages have different shapes.** A collection
fetched through its parent (`/playlists/{id}`) nests the rows under
`data.relationships.items.data`, with the next link at
`data.relationships.items.links.next`. A cursor page fetched from
`/playlists/{id}/relationships/items` returns them flat in `data`, with the next
link at top-level `links.next`. One parser cannot read both, which is why
`parse_v2_playlist_tracks` and `parse_playlist_relationship_items` in
`src/api/client/playlists.rs` are separate functions.

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
- Branch off latest master, named `feature/<short-description>` (e.g. `feature/stream-url-v2`)
- For bug fixes / small changes: work on feature branch, test, then present for commit
- For large features: migrate one endpoint at a time (see "Do NOT Attempt Large Refactors" above)
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

### Changelog Entries
`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)
and SemVer. Newest release first under `## [X.Y.Z] - YYYY-MM-DD`; unreleased work
accumulates under `## [Unreleased]`.

Four subsections in this order — **Added**, **Changed**, **Fixed**, **Internal** —
omitting any that are empty. `Internal` is for changes with no user-visible effect.

Entries are written for users, not lifted from the commit log: full sentences
saying what changed and what it means for them, and for a fix, what the wrong
behaviour actually was. Reference issue numbers (`(#54)`) and credit contributors
by handle where they apply.

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
- API client: `src/api/client/` — one module per domain (`albums`, `artists`,
  `playlists`, `search`, `streaming`, `tracks`); shared helpers in `mod.rs`
- Data models: `src/api/models.rs`
- State: `src/app/state/` — `list.rs` holds `StatefulList`, `now_playing.rs` the
  queue and mpv's view of it; also `detail.rs`, `keybinds.rs`, `settings.rs`,
  `sort.rs`, `palette.rs`
- UI rendering: `src/ui/` — `lists.rs` and `row.rs` own row shapes, one module per view
- API responses: `src/app/responses.rs`
- Key handling: `src/events/`

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

### UI Patterns
- The artist, search and Home views share one tab strip: `render_carousel` in
  `src/ui/carousel.rs` draws the bordered block whose title is the strip and
  returns the area for the active tab's content. It owns the strip's padding —
  labels passed in carry no leading or trailing spaces. `carousel_width` says
  what the strip needs, which is how Home decides whether it can afford its art
  column.
- List rows go through `src/ui/row.rs`: `layout_row` splits a row into `Fixed`
  columns (metadata that must stay visible) and `Flex` ones (text that can be
  ellipsized). A `Flex` column whose share falls below its `min` is dropped
  rightmost-first, which is how the artist gives way before the title. Truncation
  is measured in **display columns** via `unicode-width`, not chars — a library
  holds CJK titles and emoji, and counting chars misaligns every column on those
  rows. `track_row`, `album_row`, `playlist_row` and `simple_row` in
  `src/ui/lists.rs` are the only row shapes; do not hand-build a `ListItem`.
- The marquee scrolls only the selected row's overflowing text, from
  `App::marquee_phase()`. `handle_key` resets `marquee_epoch` on every keystroke,
  which is why a row the cursor just landed on starts from its beginning — that
  one line replaces per-list animation state.
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

**"A list stops after one page"**
- `links.next` is a path, not a URL — resolve it with `absolute_url`
- Check `append_page` was used, not `append` with a synthesised total
- Confirm `include` parameters are repeated on the cursor request

**"Newly favorited item doesn't show cover art"**
- Check if track object includes full album data
- Verify `include=albums,artists` parameters are present in requests
- Track objects from favorites may need album data refresh

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
  `PlayerEvent::TrackEnded` compares it against the entry `next_index()` names and
  replays the track instead of advancing blindly when they disagree.
- **`NowPlaying::next_index()` is the only answer to "what follows".** It applies
  the repeat mode — the first entry after the last under repeat queue, the current
  one under repeat track — so prefetching and advancing cannot disagree about it.
  Never compute `queue_index + 1` for that; `n`/`p` use `skip_index()`, because
  repeat track governs a track that ends, not a skip.
- **A stream request says whether it is the prefetch**, and `StreamUrl` echoes
  it. Under repeat track the prefetch names the very track whose `Play` is still
  loading, and mpv's `file-loaded` can land after that response; matching on the
  id alone took it for the current track and restarted the song.
- **`PlayerCmd::Play`** (`loadfile replace`) is the resync point — it resets the
  playlist to one entry at position 0. Explicit actions (next/prev/play-from-queue)
  all route through it, which is why they never desync, and why bugs here only
  reproduce while tracks advance on their own.

`src/app/playback.rs` tests carry a `FakeMpv` modelling all of the above; the
`assert_in_sync` invariant (what mpv plays is what Now Playing shows) is the thing
to preserve. Some of those tests depend on shuffle order — run the suite repeatedly
when changing this area.

### Narrowing Lists — Every List, Not Just the Library Tabs

`StatefulList::selected` indexes the **visible** rows, not `items`. Two things
narrow a list and both populate `matches` with positions into `items`:

1. the filter query, on the library tabs
2. `hide_atmos`, on every list that can hold a mix — see "Hiding Atmos Rows"

**Anything that mutates `items` directly must call `refilter()`** or the indices
go stale. Use `remove_where` for removals — it owns `total`, the refilter and the
clamp. `narrowed()` is the fast-path test: with no query and Atmos showing, the
accessors read `items` directly and cost nothing.

Reach rows through `selected_item()` / `get_visible()` / `visible_window()` /
`visible_items()`, never `items[selected]` or a hand-rolled
`items.iter().skip(offset)`. That pairing makes a narrowed list act on — or draw
— the wrong row.

This used to be a library-tab concern only, because detail views could not be
filtered. It is not any more: `hide_atmos` narrows artist, album and playlist
detail lists too. Fixing that meant rewriting 20 `items.get(selected)` sites in
`src/events/navigation.rs`, the three "queue the whole list" clones beside them,
and the four hand-rolled `skip(offset).take(height)` loops in
`src/ui/artist_detail.rs`, which drew hidden rows while the title counted the
visible ones. If a new list-shaped view appears, it goes through those accessors
from the start.

### Hiding Atmos Rows

With Atmos off, an entry Tidal holds *only* as an Atmos mix plays as stereo FLAC
while advertising ATMOS, so it is hidden — matching the official clients, which
show the catalogue's separate stereo release in its place. `models::atmos_only`
is the predicate: `DOLBY_ATMOS` in `mediaTags` with no `LOSSLESS` or
`HIRES_LOSSLESS` beside it. Entries carrying both mixes keep playing lossless
stereo when Atmos is off, so they stay — only their badge changes.

`mediaTags` is the only signal available: v2 exposes no `audioModes`. Verified
against 81 Atmos-bearing search results over 20 queries, the predicate picks out
exactly the entries whose v1 `audioModes` is `["DOLBY_ATMOS"]` alone.

Two mechanisms, because the two structures give different guarantees:

- **`StatefulList` narrows in place.** `App::apply_atmos_visibility` mirrors the
  setting into every list, the view stack included, and `App::push_view` calls it
  so a view opened later cannot miss it. Toggling is instant and refetches
  nothing.
- **The search panes drop rows on arrival.** They are plain `Vec`s that
  `track_sel` indexes directly, so a row present but undrawn would make Enter act
  on the wrong track. `App::refresh_search` re-runs the query on toggle instead.

The queue is deliberately left alone: what is queued is what plays, and hiding a
queued row would desync it from mpv. Its badge still drops to the truth, which is
why `quality_badge` takes the setting.

### Playlist Editing — Verified API

Every playlist write was exercised live (2026-10-15/16) with throwaway
playlists; the full table is in
`docs/superpowers/specs/2026-10-16-playlist-editing-design.md`. The facts the
code depends on:

- **Ownership comes only from `GET /playlists?filter[owners.id]=me`.** A
  playlist's `owners` relationship is empty even with `include=owners`, and
  `playlistType: USER` covers other people's playlists too. `Playlist::kind`
  takes the owned set as an argument because it arrives in its own response,
  in either order relative to the library.
- **A new playlist joins the collection by itself.** `on_playlist_created`
  inserts it into the library locally for the same reason; no save call follows.
- **An add answers `200` even when Tidal refuses the track.** The refusal is
  in `meta.skipped` with a `reason` (seen: `NOT_FOUND`). `parse_add_outcome`
  checks it first; trusting the status reports tracks as filed when they are not.
- **Duplicates are allowed, and each entry has its own `meta.itemId`.** Removing
  and moving an entry both require that `itemId` — a remove without it is
  `400 Must not be null`.
- **Write errors carry `errors[0].detail`.** `send_openapi_json` surfaces it,
  which is what the toast shows.
- **A move names the entry it lands before.** `PATCH …/relationships/items`
  with the entry's `meta.itemId` and a top-level `meta.positionBefore`; to move
  to the end, the whole top-level `meta` is left out (verified that way).
- **A rename sends only `name`; a delete sends no body.** `PATCH /playlists/{id}`
  leaves the attributes it is not given unchanged, and `DELETE /playlists/{id}`
  was verified bodiless, which is why `send_openapi_json` takes an `Option`.
