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

**CRITICAL: Tidal API v2 does NOT support `page[size]` parameters.**

- Fixed result count per request (typically 40 for initial, 20 for pagination)
- Uses cursor-based pagination with `page[cursor]` parameter
- Response includes `nextCursor` in meta/links for subsequent pages
- Always include `include` parameters on pagination requests to get full objects

**Pattern:**
```rust
let min_items = if next_link.is_none() { 40 } else { 20 };
// Use cursor-based pagination, not offset-based
```

See memory: `pagination_strategy.md` and `tidal_api_pagination.md` for full details.

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
- Always add debug logs for:
  - API parsing (especially JSON:API responses)
  - Pagination cursor changes
  - Data extraction from responses
- Use `tracing::debug!()` macro

### Pagination in Lists
- Use `StatefulList<T>` with `pagination_cursor: Option<String>` field
- Load initial: 40 items, subsequent pages: 20 items
- Implement `should_load_more()` pattern for scroll-based loading

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
- Separate pagination_cursor for API pagination vs next_offset for UI display
- Views on stack (ArtistDetail, PlaylistDetail, AlbumDetail) maintain independent state

### API Response Handling
- Parse JSON:API format into domain models
- Extract relationships and included objects manually
- Build maps for efficient lookups during transformation

## Remaining V1 API Usage & Refactoring Opportunities

### Complete V1 API Usage Inventory

**Still using v1 API endpoints:**

| Function | Endpoint | File | Priority |
|----------|----------|------|----------|
| get_favorite_artists | `/users/{uid}/favorites/artists` | client.rs:1410 | Medium |
| get_favorite_tracks | `/users/{uid}/favorites/tracks` | client.rs:1890 | Medium |
| get_track_lyrics | `/tracks/{id}/lyrics` | client.rs:2249 | Low |
| get_stream_url | `/tracks/{id}/playbackinfopostpaywall` | client.rs:2317 | Critical |
| add_favorite_album | `POST /users/{uid}/favorites/albums` | client.rs:1877 | Low |
| remove_favorite_album | `DELETE /users/{uid}/favorites/albums/{id}` | client.rs:1885 | Low |
| add_favorite_track | `POST /users/{uid}/favorites/tracks` | client.rs:2291 | Medium |
| follow_artist | `POST /users/{uid}/favorites/artists` | client.rs:2299 | Low |
| remove_favorite_track | `DELETE /users/{uid}/favorites/tracks/{id}` | client.rs:2307 | Medium |
| unfollow_artist | `DELETE /users/{uid}/favorites/artists/{id}` | client.rs:2312 | Low |

**Note:** `get_artist_bio` migrated to v2 (`client.rs:1602` → `OPENAPI_BASE/relationships/biography`). Previous v1 empty-include limitation no longer applies; v2 completeness tracked in `artist_biography_v2_incomplete.md`.

### Pagination Refactoring Opportunities

**Problem:** Pagination logic repeated 4+ times across `loading.rs` and `responses.rs` with inconsistent batch sizes.

**Current Implementation Pattern (Duplicated):**
```rust
// In loading.rs - repeated 4+ times
if self.list.loading || self.list.exhausted { return; }
self.list.loading = true;
let _ = self.api_tx.send(ApiRequest::LoadXxx { offset: self.list.next_offset });

// In responses.rs - repeated 4+ times  
ApiResponse::Xxx(items, total) => {
    self.xxx.append(items, total);
    if self.xxx_sort.is_none() {
        self.xxx.items.sort_by(...);
    }
}
```

**Batch Size Inconsistencies (verified on `create-agent-directives`, `src/api/mod.rs` / `client.rs`):**
- `get_favorite_artists`: 50 items offset-based (`mod.rs:142` → `get_favorite_artists(offset, 50)`)
- `get_favorite_tracks`: 50 items offset-based (`mod.rs:191` → `get_favorite_tracks(offset, 50)`)
- `get_favorite_albums`: cursor-based v2 (`mod.rs:184` → `LoadFavAlbums { next_url }` → `client.rs:1787`, API default ~40 initial / 20 paginated via `pagination_cursor`)
- `get_favorite_playlists` + `get_user_collection_playlists`: cursor-based v2 merged in `mod.rs:154-184` (no fixed limit)
- Artist catalog (`get_artist_top_tracks` `:1422`, `get_artist_albums` `:1466`, `get_artist_eps` `:1514`, `get_artist_singles` `:1558`): internal `while next_url` full fetch, no caller-side limit
- `get_album_tracks` `:2135`, `get_playlist_tracks` etc.: cursor-based via `pagination_cursor`

**Refactoring Opportunity:** Create centralized pagination helper with configurable limits (offset vs cursor)

### Code Duplication & Standardization Opportunities

**1. Query Parameter Building (5+ instances)**
- Files: `client.rs` in `get()` `:1242`, `get_openapi()` `:1298`, `post_form()` `:1385`, `delete()` `:2271`, `search_*` `:1917` etc.
- Pattern: Build `all_params` vec, add countryCode, optionally sessionId, extend with params
- **Opportunity:** Extract to `build_api_params()` helper

**2. Error Response Parsing (2 instances)**
- Files: `client.rs` `get()` `:1282-1290` (300 char snippet), `get_openapi()` `:1308-1314` (400 char snippet)
- Pattern: Log error with body snippet, return formatted error
- **Opportunity:** Extract to `parse_error_response()` helper

**3. Favorite/Collection Management (4 instances)**
- Files: `responses.rs` `:49-52`, `:59-62`, `:480-483`, `:488-491`
- Pattern: `.retain()`, `total.saturating_sub()`, adjust selection index
- **Opportunity:** Extract to `remove_item_from_list()` helper

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
| Pagination | High | Medium | loading.rs, responses.rs | 40+ | **P0** |
| Query Param Builder | High | Low | client.rs (4 places) | 60+ | **P0** |
| Error Parser | Medium | Low | client.rs (2 places) | 20+ | P1 |
| Collection Manager | Medium | Low | responses.rs (4 places) | 30+ | P1 |
| Deduplication | Medium | Low | responses.rs (2 places) | 15+ | P1 |
| UI List Item | High | High | ui.rs (75+ places) | 200+ | P2 |
| Queue Extension | Low | Medium | responses.rs, playback.rs | 25+ | P2 |

### Recommended Refactoring Sequence

1. **Phase 1 (Quick Wins):** Query param builder + Error parser (P0 items)
   - Low effort, high code clarity improvement
   - Estimated: 2-3 hours

2. **Phase 2 (Core Stability):** Pagination helper + Collection manager
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
