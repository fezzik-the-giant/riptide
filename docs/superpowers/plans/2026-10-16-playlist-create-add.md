# Playlist Editing PR 1 — Ownership, Create and Add — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show which playlists are the user's own, let them create a playlist, and let them add any track to one of their own playlists (issue #71, part 1 of #57).

**Architecture:** Three new v2 calls (owned-playlist list, create, add) in `src/api/client/playlists.rs` behind one body-returning write helper in `mod.rs`. They travel through the existing `ApiRequest`/`ApiResponse` worker. App-side logic lives in a new `src/app/playlist_edit.rs`, and its two overlay states (picker and name prompt) in a new `src/app/state/playlist_edit.rs`. Ownership is a `HashSet<String>` on `App`, and `Playlist::kind(&owned)` is evaluated at render time, so the order the two responses arrive in doesn't matter.

**Tech Stack:** Rust, ratatui, crossterm, reqwest, serde_json, tokio.

**Spec:** `docs/superpowers/specs/2026-10-16-playlist-editing-design.md` (read it first: it holds the verified API table this plan relies on).

## Global Constraints

- **No commits until the user has tested in the TUI and confirmed.** Tasks end at "tests pass", not at "commit". The final task hands the branch to the user. (User rule; overrides the usual per-task commit.)
- Commit message format when the user asks: conventional prefix, single line, **no `Co-Authored-By` or any agent trailer.**
- Work on branch `feature/playlist-create-add` (already exists, linked to #71).
- New playlists are created with `accessType: "UNLISTED"`.
- An add is successful only when the response has no `meta.skipped` entry; a `200` alone is not success.
- Owned playlists come only from `GET /playlists?filter[owners.id]=me`. Never infer ownership from `playlistType` or the `owners` relationship.
- Comments explain *why*, never *what* (AGENTS.md "Comments: why, not what"). No decorative banners inside functions.
- Logging: one `debug!` per write with ids, `warn!` when a track is skipped (with the reason), `error!` on a failed request with status and body snippet. Never log inside a per-item loop.
- Truncate display text by display columns (`ellipsize` / `ellipsize_head`), never by `chars()`.
- Verify with `cargo build`, `cargo test`, `cargo clippy --all-targets`.

## Review Focus

1. **More owned playlists than fit in the picker.** The selected row must stay visible while scrolling with `j`/`k`. Pinned by `picker_scroll` tests in Task 4.
2. **Adding to a playlist whose detail view is open but still paging.** The row must not be appended locally, because the server-side page that arrives next already contains it. Pinned in Task 3.
3. **`P` with no track under the cursor** (Playlists tab, Home, an album row). It shows a toast and opens no picker. Pinned in Task 4.
4. **Typing a name containing `j`, `k`, `q`, `P`, or a space.** These are letters in the prompt, not commands, so the prompt must route ahead of `vim_arrows` and the global keys. Pinned in Task 4.
5. **Terminals that send `p` + SHIFT instead of `P`.** That must open the picker, not jump to the previous track. Pinned in Task 4.

---

### Task 1: `PlaylistKind` and `playlist_type` on `Playlist`

**Files:**
- Modify: `src/api/models.rs` (the `Playlist` struct ~line 231, its `impl`, tests module ~line 412)
- Modify: `src/api/client/mod.rs:61-65` (`OpenApiPlaylistAttrs`)
- Modify: `src/api/client/playlists.rs:338` (collection page literal)
- Modify: `src/api/client/search.rs:384` (search playlist literal)
- Modify: `src/api/client/mixes.rs:21` (`mix_from_playlist`)
- Modify test literals: `src/ui/home.rs:207`, `src/app/navigation.rs:388`, `src/ui/lists.rs:489`, `src/app/playback.rs:1942`

**Interfaces:**
- Produces: `pub enum PlaylistKind { Yours, Tidal, User, Mix, Other }` with `pub fn label(self) -> &'static str`. `Playlist.playlist_type: Option<String>`. `Playlist::kind(&self, owned: &HashSet<String>) -> PlaylistKind`.

- [ ] **Step 1: Write the failing test** in the `tests` module of `src/api/models.rs`. Extend its `use super::{…}` line with `Playlist, PlaylistKind`, then add:

```rust
#[test]
fn ownership_outranks_playlist_type() {
    let owned: std::collections::HashSet<String> = ["mine".to_string()].into();
    let p = |uuid: &str, ty: Option<&str>| Playlist {
        uuid: uuid.to_string(),
        title: String::new(),
        number_of_tracks: None,
        description: None,
        cover: None,
        added_at: None,
        playlist_type: ty.map(String::from),
    };
    let cases = [
        (p("mine", Some("USER")), PlaylistKind::Yours),
        (p("mine", None), PlaylistKind::Yours),
        (p("other", Some("EDITORIAL")), PlaylistKind::Tidal),
        (p("other", Some("USER")), PlaylistKind::User),
        (p("other", Some("MIX")), PlaylistKind::Mix),
        (p("other", Some("ARTIST")), PlaylistKind::Other),
        (p("other", None), PlaylistKind::Other),
    ];
    for (playlist, want) in cases {
        assert_eq!(
            playlist.kind(&owned),
            want,
            "{} / {:?}",
            playlist.uuid,
            playlist.playlist_type
        );
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test ownership_outranks_playlist_type`
Expected: compile error, no field `playlist_type` / no type `PlaylistKind`.

- [ ] **Step 3: Implement.** In `src/api/models.rs` add the field as the last field of `Playlist`:

```rust
    #[serde(rename = "playlistType", default)]
    pub playlist_type: Option<String>,
```

Add, below the `Playlist` struct:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistKind {
    Yours,
    Tidal,
    User,
    Mix,
    Other,
}

impl PlaylistKind {
    pub fn label(self) -> &'static str {
        match self {
            PlaylistKind::Yours => "Yours",
            PlaylistKind::Tidal => "Tidal",
            PlaylistKind::User => "User",
            PlaylistKind::Mix => "Mix",
            PlaylistKind::Other => "",
        }
    }
}
```

In `impl Playlist` add:

```rust
    /// Ownership is passed in rather than stored: it arrives in its own response,
    /// in either order relative to the playlists, so a stored value would be
    /// stale whenever it lands second. `playlistType: USER` alone does not mean
    /// "yours" — other people's playlists carry it too.
    pub fn kind(&self, owned: &std::collections::HashSet<String>) -> PlaylistKind {
        if owned.contains(&self.uuid) {
            return PlaylistKind::Yours;
        }
        match self.playlist_type.as_deref() {
            Some("EDITORIAL") => PlaylistKind::Tidal,
            Some("USER") => PlaylistKind::User,
            Some("MIX") => PlaylistKind::Mix,
            _ => PlaylistKind::Other,
        }
    }
```

In `src/api/client/mod.rs`, add to `OpenApiPlaylistAttrs`:

```rust
    #[serde(rename = "playlistType")]
    playlist_type: Option<String>,
```

In `src/api/client/playlists.rs` (`user_collection_playlists_page`), add `playlist_type: attr.playlist_type.clone(),` to the `Playlist { … }` literal.

In `src/api/client/search.rs:384`, add `playlist_type: item["attributes"]["playlistType"].as_str().map(String::from),`.

In `src/api/client/mixes.rs:21`, add:

```rust
        playlist_type: attrs
            .get("playlistType")
            .and_then(|v| v.as_str())
            .map(String::from),
```

In the four test-only literals (`src/ui/home.rs:207`, `src/app/navigation.rs:388`, `src/ui/lists.rs:489`, `src/app/playback.rs:1942`), add `playlist_type: None,`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all pass, including `ownership_outranks_playlist_type`.

---

### Task 2: Client: write helper, parsers, `create_playlist`, `add_to_playlist`, `get_owned_playlist_ids`

**Files:**
- Modify: `src/api/models.rs` (add `AddOutcome`)
- Modify: `src/api/client/mod.rs` (add `send_openapi_json`, `error_detail`, a tests module)
- Modify: `src/api/client/playlists.rs` (parsers, three methods, tests module)

**Interfaces:**
- Consumes: `Playlist.playlist_type` (Task 1).
- Produces:
  - `pub enum AddOutcome { Added { item_id: String }, Skipped { reason: String } }` in `models.rs` (derive `Debug, Clone, PartialEq`)
  - `ApiClient::create_playlist(&self, name: &str) -> Result<Playlist>`
  - `ApiClient::add_to_playlist(&self, uuid: &str, track_id: u64) -> Result<AddOutcome>`
  - `ApiClient::get_owned_playlist_ids(&self) -> Result<HashSet<String>>`
  - `ApiClient::send_openapi_json(&self, method: reqwest::Method, path: &str, body: &serde_json::Value) -> Result<serde_json::Value>` (private to the client)

- [ ] **Step 1: Write the failing tests.** Append to `src/api/client/playlists.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Live `POST /playlists` response (2026-10-15), externalLinks trimmed.
    const CREATED: &str = r#"{"data":{"id":"7c56e2bc-d5c3-44ef-adeb-57b5b1327d28","type":"playlists","attributes":{"name":"riptide-probe","description":"temporary","bounded":false,"createdAt":"2026-10-15T18:20:19.824299Z","lastModifiedAt":"2026-10-15T18:20:19.822501Z","accessType":"UNLISTED","playlistType":"USER","numberOfFollowers":0}},"links":{"self":"/playlists/7c56e2bc-d5c3-44ef-adeb-57b5b1327d28?countryCode=US"}}"#;

    /// Live single-track add (2026-10-15).
    const ADDED: &str = r#"{"data":[{"id":"431291038","type":"tracks","meta":{"itemId":"e2df580f-28c7-49ed-950a-e19cb8d761bd","addedAt":"2026-10-15T18:20:28.091909Z"}}],"links":{"self":"/playlists/7c56e2bc-d5c3-44ef-adeb-57b5b1327d28/relationships/items?countryCode=US"},"meta":{"skipped":[]}}"#;

    /// Live add (2026-10-16) where Tidal refused one track but still answered 200.
    const SKIPPED: &str = r#"{"data":[{"id":"431291038","type":"tracks","meta":{"itemId":"d868dbbb-e6fa-4615-8061-6ebb0ee6e892","addedAt":"2026-10-16T15:05:37.127112Z"}}],"links":{"self":"/playlists/33545e0e-0216-40d9-9f6f-2eb6abde0e11/relationships/items?countryCode=US"},"meta":{"skipped":[{"id":"189681882","type":"tracks","reason":"NOT_FOUND"}]}}"#;

    /// Live `GET /playlists?filter[owners.id]=me` (2026-10-15), attributes trimmed.
    const OWNED: &str = r#"{"data":[{"id":"8c67ccd0-bd1c-4eae-91d0-c18afa9fa47c","type":"playlists","attributes":{"name":"🔥","playlistType":"USER"}},{"id":"e4658ab2-ac28-41f8-a76b-6fd0b96e1168","type":"playlists","attributes":{"name":"Reya's Jams","playlistType":"USER"}}],"links":{"self":"/playlists?filter%5Bowners.id%5D=me&countryCode=US"}}"#;

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn a_created_playlist_is_parsed_as_an_empty_user_playlist() {
        let p = parse_created_playlist(&json(CREATED)).unwrap();
        assert_eq!(p.uuid, "7c56e2bc-d5c3-44ef-adeb-57b5b1327d28");
        assert_eq!(p.title, "riptide-probe");
        assert_eq!(p.number_of_tracks, Some(0));
        assert_eq!(p.playlist_type.as_deref(), Some("USER"));
    }

    #[test]
    fn an_add_yields_the_new_entry_item_id() {
        assert_eq!(
            parse_add_outcome(&json(ADDED)).unwrap(),
            AddOutcome::Added {
                item_id: "e2df580f-28c7-49ed-950a-e19cb8d761bd".to_string()
            }
        );
    }

    /// Tidal answers 200 even when it refuses the track, so trusting the status
    /// would tell the user a track was filed when it was not.
    #[test]
    fn a_skipped_track_is_never_reported_as_added() {
        assert_eq!(
            parse_add_outcome(&json(SKIPPED)).unwrap(),
            AddOutcome::Skipped {
                reason: "NOT_FOUND".to_string()
            }
        );
    }

    #[test]
    fn the_owned_page_yields_ids_and_ends_without_a_next_link() {
        let (ids, next) = parse_owned_page(&json(OWNED));
        assert_eq!(
            ids,
            vec![
                "8c67ccd0-bd1c-4eae-91d0-c18afa9fa47c".to_string(),
                "e4658ab2-ac28-41f8-a76b-6fd0b96e1168".to_string()
            ]
        );
        assert_eq!(next, None);
    }
}
```

Append to `src/api/client/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_error_surfaces_tidal_s_own_detail() {
        let body = br#"{"errors":[{"code":"INVALID_REQUEST_BODY","detail":"Must not be null","source":{"pointer":"data/0/meta"}}]}"#;
        assert_eq!(error_detail(body).as_deref(), Some("Must not be null"));
        assert_eq!(error_detail(b"<html>bad gateway</html>"), None);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test client::` (covers both new test modules under `src/api/client/`)
Expected: compile errors (`parse_created_playlist`, `parse_add_outcome`, `parse_owned_page`, `error_detail`, `AddOutcome` not found).

- [ ] **Step 3: Implement.** In `src/api/models.rs`, next to `PlaylistKind`:

```rust
/// What Tidal did with one track sent to a playlist.
#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    Added { item_id: String },
    /// The request still answered 200; `reason` is Tidal's, e.g. `NOT_FOUND`.
    Skipped { reason: String },
}
```

In `src/api/client/mod.rs`, below `delete_openapi_json`:

```rust
    /// Writes whose response the caller needs. Unlike `post_openapi_json`, a
    /// failure carries Tidal's own `errors[0].detail`, which is what the toast
    /// shows — `error_for_status` would reduce it to a bare status code.
    async fn send_openapi_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let token = self.access_token().await?;
        let url = format!("{OPENAPI_BASE}{path}");
        let resp = self
            .http
            .request(method, &url)
            .bearer_auth(&token)
            .query(&[("countryCode", &self.config.country_code)])
            .json(body)
            .send()
            .await
            .context("openapi write failed")?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(200).collect();
            tracing::error!("openapi write {path} returned {status}: {snippet}");
            anyhow::bail!(
                "{}",
                error_detail(&bytes).unwrap_or_else(|| format!("HTTP {status}"))
            );
        }
        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
```

and as a free function near `absolute_url`:

```rust
fn error_detail(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v["errors"][0]["detail"].as_str().map(String::from)
}
```

In `src/api/client/playlists.rs`, change the import line to `use anyhow::{Context, Result};`, add `use std::collections::HashSet;`, and add the parsers above `impl ApiClient`:

```rust
pub(super) fn parse_created_playlist(resp: &serde_json::Value) -> Result<Playlist> {
    let data = &resp["data"];
    let uuid = data["id"]
        .as_str()
        .context("create response carries no data.id")?;
    let attrs = &data["attributes"];
    Ok(Playlist {
        uuid: uuid.to_string(),
        title: attrs["name"].as_str().unwrap_or_default().to_string(),
        // The create response has no numberOfItems; a playlist made a moment
        // ago is empty, and saying so beats rendering no count at all.
        number_of_tracks: Some(0),
        description: attrs["description"].as_str().map(String::from),
        cover: None,
        // Stands in for the collection's addedAt so "last added" sorts it first.
        added_at: attrs["createdAt"].as_str().map(String::from),
        playlist_type: attrs["playlistType"].as_str().map(String::from),
    })
}

/// Reads the outcome for a single-track add. `skipped` is checked first because
/// Tidal answers 200 whether or not it took the track.
pub(super) fn parse_add_outcome(resp: &serde_json::Value) -> Result<AddOutcome> {
    if let Some(skipped) = resp["meta"]["skipped"].as_array().and_then(|a| a.first()) {
        let reason = skipped["reason"].as_str().unwrap_or("UNKNOWN").to_string();
        return Ok(AddOutcome::Skipped { reason });
    }
    let item_id = resp["data"][0]["meta"]["itemId"]
        .as_str()
        .context("add response carries neither an itemId nor a skipped entry")?;
    Ok(AddOutcome::Added {
        item_id: item_id.to_string(),
    })
}

pub(super) fn parse_owned_page(resp: &serde_json::Value) -> (Vec<String>, Option<String>) {
    let ids = resp["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let next = resp["links"]["next"].as_str().map(String::from);
    (ids, next)
}
```

Add to `impl ApiClient` in `playlists.rs`:

```rust
    /// The body matches the shape verified live, `description` included.
    pub async fn create_playlist(&self, name: &str) -> Result<Playlist> {
        let body = serde_json::json!({"data": {"type": "playlists", "attributes": {
            "name": name, "description": "", "accessType": "UNLISTED"
        }}});
        let resp = self
            .send_openapi_json(reqwest::Method::POST, "/playlists", &body)
            .await?;
        let playlist = parse_created_playlist(&resp)?;
        tracing::debug!("created playlist {}", playlist.uuid);
        Ok(playlist)
    }

    pub async fn add_to_playlist(&self, uuid: &str, track_id: u64) -> Result<AddOutcome> {
        let body = serde_json::json!({"data": [{"id": track_id.to_string(), "type": "tracks"}]});
        let resp = self
            .send_openapi_json(
                reqwest::Method::POST,
                &format!("/playlists/{uuid}/relationships/items"),
                &body,
            )
            .await?;
        let outcome = parse_add_outcome(&resp)?;
        match &outcome {
            AddOutcome::Added { item_id } => {
                tracing::debug!("added track {track_id} to playlist {uuid} as item {item_id}")
            }
            AddOutcome::Skipped { reason } => {
                tracing::warn!("Tidal skipped track {track_id} for playlist {uuid}: {reason}")
            }
        }
        Ok(outcome)
    }

    /// The only reliable ownership signal: a playlist's `owners` relationship
    /// comes back empty even with `include=owners`, and `playlistType: USER`
    /// covers other people's playlists too.
    pub async fn get_owned_playlist_ids(&self) -> Result<HashSet<String>> {
        let token = self.access_token().await?;
        let mut owned = HashSet::new();
        let mut next = Some(format!(
            "/playlists?filter%5Bowners.id%5D=me&countryCode={}",
            self.config.country_code
        ));
        while let Some(path) = next {
            let resp: serde_json::Value = self
                .http
                .get(super::absolute_url(&path))
                .bearer_auth(&token)
                .send()
                .await
                .context("owned playlists request failed")?
                .error_for_status()?
                .json()
                .await
                .context("parse owned playlists")?;
            let (ids, more) = parse_owned_page(&resp);
            owned.extend(ids);
            next = more;
        }
        tracing::info!("you own {} playlists", owned.len());
        Ok(owned)
    }
```

Check `absolute_url` is visible from `playlists.rs` (it is `pub(super)` in `mod.rs`, and `playlists` is a child module, so `super::absolute_url` resolves).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: all pass. `cargo clippy --all-targets` is clean.

---

### Task 3: Messages, worker and response handling

**Files:**
- Modify: `src/api/messages.rs`
- Modify: `src/api/worker.rs`
- Create: `src/app/playlist_edit.rs`
- Modify: `src/app/mod.rs` (add `mod playlist_edit;`, `owned_playlist_ids` field + init)
- Modify: `src/app/loading.rs:79` (send `LoadOwnedPlaylists`)
- Modify: `src/app/responses.rs` (route the three new responses)

**Interfaces:**
- Consumes: `create_playlist`, `add_to_playlist`, `get_owned_playlist_ids`, `AddOutcome` (Task 2).
- Produces:
  - `ApiRequest::LoadOwnedPlaylists`, `ApiRequest::CreatePlaylist { name: String, then_add: Option<Track> }`, `ApiRequest::AddToPlaylist { playlist: Playlist, track: Track }`
  - `ApiResponse::OwnedPlaylists(HashSet<String>)`, `ApiResponse::PlaylistCreated { playlist: Playlist, then_add: Option<Track> }`, `ApiResponse::AddedToPlaylist { playlist: Playlist, track: Track, outcome: AddOutcome }`
  - `App.owned_playlist_ids: HashSet<String>`
  - `App::on_playlist_created(&mut self, playlist: Playlist, then_add: Option<Track>)`, `App::on_added_to_playlist(&mut self, playlist: Playlist, track: Track, outcome: AddOutcome)`

- [ ] **Step 1: Write the failing tests.** Create `src/app/playlist_edit.rs` with only the test module for now:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Creating playlists and filing tracks into the user's own.

#[cfg(test)]
mod tests {
    use crate::api::ApiRequest;
    use crate::api::models::{AddOutcome, Playlist};
    use crate::app::test_support::{test_app, track};
    use crate::app::{StatusLevel, View};

    pub(super) fn own(uuid: &str, title: &str) -> Playlist {
        Playlist {
            uuid: uuid.to_string(),
            title: title.to_string(),
            number_of_tracks: Some(2),
            description: None,
            cover: None,
            added_at: None,
            playlist_type: Some("USER".to_string()),
        }
    }

    #[test]
    fn a_created_playlist_joins_the_library_as_yours_then_receives_its_track() {
        let mut t = test_app();
        t.drain_api();
        t.app.on_playlist_created(own("new", "Road trip"), Some(track(7)));

        assert!(t.app.owned_playlist_ids.contains("new"));
        assert!(t.app.favorite_playlist_ids.contains("new"));
        assert!(t.app.playlists.items.iter().any(|p| p.uuid == "new"));
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::AddToPlaylist { playlist, track }]
                if playlist.uuid == "new" && track.id == 7
        ));
    }

    #[test]
    fn a_skipped_add_is_reported_as_a_failure_and_changes_nothing() {
        let mut t = test_app();
        t.app.playlists.append_page(vec![own("p", "Mine")], None);
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(7),
            AddOutcome::Skipped { reason: "NOT_FOUND".to_string() },
        );
        assert_eq!(t.app.playlists.items[0].number_of_tracks, Some(2));
        let (msg, level, _) = t.app.status.clone().unwrap();
        assert_eq!(level, StatusLevel::Error);
        assert!(msg.contains("NOT_FOUND"), "{msg}");
    }

    #[test]
    fn an_add_appends_to_the_open_detail_once_it_has_finished_loading() {
        let mut t = test_app();
        t.app.open_playlist(own("p", "Mine"));
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks.append_page(vec![track(1)], None);
        }
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(7),
            AddOutcome::Added { item_id: "i".to_string() },
        );
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
            panic!("detail closed")
        };
        assert_eq!(d.tracks.items.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 7]);
    }

    /// The next server page already holds the new entry, so appending it here
    /// would show it twice.
    #[test]
    fn an_add_leaves_a_still_paging_detail_to_the_server() {
        let mut t = test_app();
        t.app.open_playlist(own("p", "Mine"));
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks.append_page(vec![track(1)], Some("cursor".to_string()));
        }
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(7),
            AddOutcome::Added { item_id: "i".to_string() },
        );
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
            panic!("detail closed")
        };
        assert_eq!(d.tracks.items.len(), 1);
    }
}
```

Add `mod playlist_edit;` to the module list in `src/app/mod.rs`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test playlist_edit`
Expected: compile errors (`on_playlist_created`, `on_added_to_playlist`, `owned_playlist_ids`, `ApiRequest::AddToPlaylist` not found).

- [ ] **Step 3: Implement.** `src/api/messages.rs`: add `use std::collections::HashSet;` and, in `ApiRequest`:

```rust
    LoadOwnedPlaylists,
    CreatePlaylist {
        name: String,
        /// Set when the playlist is being made from the "add to playlist" picker.
        then_add: Option<Track>,
    },
    AddToPlaylist {
        playlist: Playlist,
        track: Track,
    },
```

in `ApiResponse`:

```rust
    OwnedPlaylists(HashSet<String>),
    PlaylistCreated {
        playlist: Playlist,
        then_add: Option<Track>,
    },
    AddedToPlaylist {
        playlist: Playlist,
        track: Track,
        outcome: AddOutcome,
    },
```

`src/api/worker.rs`, beside the `RemovePlaylist` arm:

```rust
        ApiRequest::LoadOwnedPlaylists => match client.get_owned_playlist_ids().await {
            Ok(ids) => ApiResponse::OwnedPlaylists(ids),
            Err(e) => ApiResponse::Error(format!("Couldn't load your playlists: {e}")),
        },

        ApiRequest::CreatePlaylist { name, then_add } => {
            match client.create_playlist(&name).await {
                Ok(playlist) => ApiResponse::PlaylistCreated { playlist, then_add },
                Err(e) => ApiResponse::Error(format!("Couldn't create '{name}': {e}")),
            }
        }

        ApiRequest::AddToPlaylist { playlist, track } => {
            match client.add_to_playlist(&playlist.uuid, track.id).await {
                Ok(outcome) => ApiResponse::AddedToPlaylist { playlist, track, outcome },
                Err(e) => {
                    ApiResponse::Error(format!("Couldn't add to '{}': {e}", playlist.title))
                }
            }
        }
```

`src/app/mod.rs`: add the field after `favorite_playlist_ids`:

```rust
    /// Playlists the user can edit. Empty until its request answers, and left
    /// empty if that fails, so edit keys refuse rather than send doomed writes.
    pub owned_playlist_ids: HashSet<String>,
```

and `owned_playlist_ids: HashSet::new(),` in `App::new` next to `favorite_playlist_ids: HashSet::new(),`.

`src/app/loading.rs:79`: after `let _ = self.api_tx.send(ApiRequest::LoadPlaylists);` add `let _ = self.api_tx.send(ApiRequest::LoadOwnedPlaylists);`.

`src/app/playlist_edit.rs`, above the test module:

```rust
use crate::api::ApiRequest;
use crate::api::models::{AddOutcome, Playlist, Track};

use super::{App, StatusLevel, View};

impl App {
    /// Tidal adds a new playlist to the collection itself, so it joins the
    /// library here as well as the owned set.
    pub(crate) fn on_playlist_created(&mut self, playlist: Playlist, then_add: Option<Track>) {
        self.owned_playlist_ids.insert(playlist.uuid.clone());
        if self.favorite_playlist_ids.insert(playlist.uuid.clone()) {
            self.playlists.items.push(playlist.clone());
            self.playlists.total = self.playlists.total.saturating_add(1);
            self.sort_playlists();
        }
        match then_add {
            Some(track) => {
                let _ = self.api_tx.send(ApiRequest::AddToPlaylist { playlist, track });
            }
            None => self.set_status(format!("Created '{}'", playlist.title), StatusLevel::Info),
        }
    }

    pub(crate) fn on_added_to_playlist(
        &mut self,
        playlist: Playlist,
        track: Track,
        outcome: AddOutcome,
    ) {
        if let AddOutcome::Skipped { reason } = outcome {
            self.set_status(
                format!("Tidal didn't add '{}' to '{}' ({reason})", track.title, playlist.title),
                StatusLevel::Error,
            );
            return;
        }
        if let Some(p) = self.playlists.items.iter_mut().find(|p| p.uuid == playlist.uuid) {
            p.number_of_tracks = Some(p.number_of_tracks.unwrap_or(0) + 1);
        }
        for view in &mut self.view_stack {
            if let View::PlaylistDetail(d) = view {
                // Still paging means a later page will already contain the entry.
                if d.playlist.uuid == playlist.uuid && d.tracks.exhausted {
                    d.tracks.items.push(track.clone());
                    d.tracks.total = d.tracks.total.saturating_add(1);
                    d.tracks.refilter();
                    d.playlist.number_of_tracks = Some(d.tracks.total);
                }
            }
        }
        self.set_status(
            format!("Added '{}' to '{}'", track.title, playlist.title),
            StatusLevel::Info,
        );
    }
}
```

`src/app/responses.rs`, next to the `PlaylistRemoved` arm:

```rust
            ApiResponse::OwnedPlaylists(ids) => self.owned_playlist_ids = ids,

            ApiResponse::PlaylistCreated { playlist, then_add } => {
                self.on_playlist_created(playlist, then_add)
            }

            ApiResponse::AddedToPlaylist { playlist, track, outcome } => {
                self.on_added_to_playlist(playlist, track, outcome)
            }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: all pass, including the four `playlist_edit` tests.

---

### Task 4: The `P` picker, the name prompt, and `N`

**Files:**
- Create: `src/app/state/playlist_edit.rs`
- Modify: `src/app/state/mod.rs` (`mod playlist_edit; pub use playlist_edit::*;`)
- Modify: `src/app/mod.rs` (fields `playlist_picker`, `name_prompt` + init)
- Modify: `src/app/playlist_edit.rs` (picker/prompt methods + tests)
- Modify: `src/events/mod.rs` (routing in `handle_key`, key tests)
- Modify: `src/events/global.rs` (`P`, `N`)
- Modify: `src/events/overlays.rs` (two input handlers)
- Modify: `src/ui/image.rs:36-41` (two `Overlays` flags)
- Modify: `src/ui/mod.rs` (flags in `draw`, calls in `render_overlays`)
- Modify: `src/ui/overlays.rs` (two render functions, `picker_scroll`, tests)

**Interfaces:**
- Consumes: `ApiRequest::CreatePlaylist`, `ApiRequest::AddToPlaylist`, `App.owned_playlist_ids` (Task 3).
- Produces:
  - `pub struct PlaylistPicker { pub active: bool, pub track: Option<Track>, pub targets: Vec<Playlist>, pub selected: usize }` with `fn rows(&self) -> usize`
  - `pub struct NamePrompt { pub active: bool, pub input: String, pub then_add: Option<Track> }`
  - `App::track_under_cursor(&self) -> Option<Track>`, `App::open_playlist_picker(&mut self)`, `App::pick_playlist(&mut self)`, `App::open_name_prompt(&mut self, then_add: Option<Track>)`, `App::submit_name_prompt(&mut self)`
  - `fn picker_scroll(selected: usize, visible: usize) -> usize` in `src/ui/overlays.rs`

- [ ] **Step 1: Write the failing tests.** Add to the test module in `src/app/playlist_edit.rs`:

```rust
    use crate::app::Tab;

    fn on_favorites_with_owned_playlists() -> crate::app::test_support::TestApp {
        let mut t = test_app();
        t.app.current_tab = Tab::Favorites;
        t.app.favorites.append_page(vec![track(7)], None);
        t.app.playlists.append_page(
            vec![own("mine", "Mine"), own("theirs", "Theirs")],
            None,
        );
        t.app.owned_playlist_ids.insert("mine".to_string());
        t.drain_api();
        t
    }

    #[test]
    fn the_picker_offers_new_playlist_then_only_your_own() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.open_playlist_picker();
        let picker = &t.app.playlist_picker;
        assert!(picker.active);
        assert_eq!(picker.track.as_ref().map(|t| t.id), Some(7));
        assert_eq!(
            picker.targets.iter().map(|p| p.uuid.as_str()).collect::<Vec<_>>(),
            ["mine"]
        );
        assert_eq!(picker.rows(), 2);
    }

    #[test]
    fn picking_a_playlist_sends_the_add_and_closes_the_picker() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.open_playlist_picker();
        t.app.playlist_picker.selected = 1;
        t.app.pick_playlist();
        assert!(!t.app.playlist_picker.active);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::AddToPlaylist { playlist, track }]
                if playlist.uuid == "mine" && track.id == 7
        ));
    }

    #[test]
    fn new_playlist_from_the_picker_creates_it_with_the_track_attached() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.open_playlist_picker();
        t.app.pick_playlist();
        assert!(t.app.name_prompt.active);
        t.app.name_prompt.input = "  Road trip ".to_string();
        t.app.submit_name_prompt();
        assert!(!t.app.name_prompt.active);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::CreatePlaylist { name, then_add: Some(track) }]
                if name == "Road trip" && track.id == 7
        ));
    }

    #[test]
    fn a_blank_name_creates_nothing_and_keeps_the_prompt_open() {
        let mut t = test_app();
        t.drain_api();
        t.app.open_name_prompt(None);
        t.app.name_prompt.input = "   ".to_string();
        t.app.submit_name_prompt();
        assert!(t.app.name_prompt.active);
        assert!(t.api_requests().is_empty());
    }

    #[test]
    fn with_no_track_under_the_cursor_the_picker_stays_shut() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.current_tab = Tab::Playlists;
        t.app.open_playlist_picker();
        assert!(!t.app.playlist_picker.active);
        assert!(t.app.status.is_some());
    }
```

Add to the `tests` module in `src/events/mod.rs`:

```rust
    fn favorites_with_one_owned_playlist() -> TestApp {
        let mut t = test_app();
        t.app.current_tab = Tab::Favorites;
        t.app.favorites.append_page(vec![crate::app::test_support::track(7)], None);
        t.app.owned_playlist_ids.insert("mine".to_string());
        t
    }

    /// Some terminals report Shift+p as a lowercase `p` carrying SHIFT, which
    /// the bare `p` arm would read as "previous track".
    #[test]
    fn shift_p_opens_the_picker_however_the_terminal_spells_it() {
        for code in [KeyCode::Char('P'), KeyCode::Char('p')] {
            let mut t = favorites_with_one_owned_playlist();
            handle_key(&mut t.app, KeyEvent::new(code, KeyModifiers::SHIFT));
            assert!(t.app.playlist_picker.active, "{code:?}");
        }
    }

    #[test]
    fn letters_typed_into_the_name_prompt_stay_letters() {
        let mut t = favorites_with_one_owned_playlist();
        t.app.open_name_prompt(None);
        for c in "jk qP".chars() {
            handle_key(&mut t.app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(t.app.name_prompt.input, "jk qP");
        assert!(!t.app.should_quit);
        assert!(!t.app.playlist_picker.active);
    }

    #[test]
    fn shift_n_opens_the_name_prompt_only_on_the_playlists_tab() {
        let mut t = favorites_with_one_owned_playlist();
        handle_key(&mut t.app, KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert!(!t.app.name_prompt.active);
        t.app.current_tab = Tab::Playlists;
        handle_key(&mut t.app, KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert!(t.app.name_prompt.active);
    }
```

Add a test module at the bottom of `src/ui/overlays.rs` (or extend its existing one, ~line 860):

```rust
    #[test]
    fn the_picker_scrolls_just_enough_to_keep_the_selection_visible() {
        assert_eq!(super::picker_scroll(0, 5), 0);
        assert_eq!(super::picker_scroll(4, 5), 0);
        assert_eq!(super::picker_scroll(5, 5), 1);
        assert_eq!(super::picker_scroll(20, 5), 16);
        assert_eq!(super::picker_scroll(3, 0), 0);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test`
Expected: compile errors for the missing picker/prompt types, methods and `picker_scroll`.

- [ ] **Step 3: Implement the state.** Create `src/app/state/playlist_edit.rs`:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Overlay state for filing a track into a playlist.

use crate::api::models::{Playlist, Track};

/// Row 0 is always "+ New playlist…"; row `i + 1` is `targets[i]`. The targets
/// are snapshotted on open so a library refresh cannot shift the row under the
/// cursor.
#[derive(Default)]
pub struct PlaylistPicker {
    pub active: bool,
    pub track: Option<Track>,
    pub targets: Vec<Playlist>,
    pub selected: usize,
}

impl PlaylistPicker {
    pub fn rows(&self) -> usize {
        self.targets.len() + 1
    }
}

#[derive(Default)]
pub struct NamePrompt {
    pub active: bool,
    pub input: String,
    /// Set when the prompt was opened from the picker, so the new playlist
    /// receives the track it was made for.
    pub then_add: Option<Track>,
}
```

In `src/app/state/mod.rs` add `mod playlist_edit;` and `pub use playlist_edit::*;`. In `src/app/mod.rs` add fields `pub playlist_picker: PlaylistPicker,` and `pub name_prompt: NamePrompt,` beside `artist_selection`, initialised with `PlaylistPicker::default()` and `NamePrompt::default()`.

- [ ] **Step 4: Implement the app methods.** In `src/app/playlist_edit.rs`, extend the imports with `use crate::search::SearchPane;` and `use super::{ArtistDetailFocus, NamePrompt, PlaylistDetailFocus, PlaylistPicker, Tab};`, then add to `impl App`:

```rust
    /// The track a row-level command acts on in the current context, or `None`
    /// where the cursor rests on something that is not a track.
    pub(crate) fn track_under_cursor(&self) -> Option<Track> {
        if self.queue_focused {
            return self.now_playing.queue.get(self.queue_cursor).cloned();
        }
        match self.view_stack.last() {
            Some(View::ArtistDetail(d)) if d.focus == ArtistDetailFocus::Tracks => {
                d.tracks.selected_item().cloned()
            }
            Some(View::PlaylistDetail(d)) if d.focus == PlaylistDetailFocus::Tracks => {
                d.tracks.selected_item().cloned()
            }
            Some(View::AlbumDetail(d)) => d.tracks.selected_item().cloned(),
            Some(_) => None,
            None => match self.current_tab {
                Tab::Favorites => self.favorites.selected_item().cloned(),
                Tab::Search if self.search.pane == SearchPane::Tracks => {
                    self.search.tracks.get(self.search.track_sel).cloned()
                }
                _ => None,
            },
        }
    }

    pub(crate) fn open_playlist_picker(&mut self) {
        let Some(track) = self.track_under_cursor() else {
            self.set_status(
                "Select a track to add it to a playlist".to_string(),
                StatusLevel::Info,
            );
            return;
        };
        let targets = self
            .playlists
            .items
            .iter()
            .filter(|p| self.owned_playlist_ids.contains(&p.uuid))
            .cloned()
            .collect();
        self.playlist_picker = PlaylistPicker {
            active: true,
            track: Some(track),
            targets,
            selected: 0,
        };
    }

    pub(crate) fn pick_playlist(&mut self) {
        let picker = std::mem::take(&mut self.playlist_picker);
        let Some(track) = picker.track else { return };
        match picker.selected.checked_sub(1).and_then(|i| picker.targets.get(i)) {
            Some(playlist) => {
                let _ = self.api_tx.send(ApiRequest::AddToPlaylist {
                    playlist: playlist.clone(),
                    track,
                });
            }
            None => self.open_name_prompt(Some(track)),
        }
    }

    pub(crate) fn open_name_prompt(&mut self, then_add: Option<Track>) {
        self.name_prompt = NamePrompt {
            active: true,
            input: String::new(),
            then_add,
        };
    }

    pub(crate) fn submit_name_prompt(&mut self) {
        let name = self.name_prompt.input.trim().to_string();
        if name.is_empty() {
            return;
        }
        let prompt = std::mem::take(&mut self.name_prompt);
        let _ = self.api_tx.send(ApiRequest::CreatePlaylist {
            name,
            then_add: prompt.then_add,
        });
    }
```

- [ ] **Step 5: Implement key handling.** In `src/events/overlays.rs`:

```rust
pub(super) fn handle_playlist_picker_input(app: &mut App, key: KeyEvent) {
    let picker = &mut app.playlist_picker;
    match key.code {
        KeyCode::Esc => *picker = Default::default(),
        KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
        KeyCode::Down => {
            if picker.selected + 1 < picker.rows() {
                picker.selected += 1;
            }
        }
        KeyCode::Enter => app.pick_playlist(),
        _ => {}
    }
}

pub(super) fn handle_name_prompt_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.name_prompt = Default::default(),
        KeyCode::Enter => app.submit_name_prompt(),
        KeyCode::Backspace => {
            app.name_prompt.input.pop();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.name_prompt.input.push(c)
        }
        _ => {}
    }
}
```

In `src/events/mod.rs` `handle_key`, directly after the `if app.command.active { … }` block:

```rust
    if app.name_prompt.active {
        handle_name_prompt_input(app, key);
        return;
    }
```

and directly after the `if app.settings.active { … }` block (after `vim_arrows`, so `j`/`k` reach the picker as Up/Down, and before queue focus, so a picker opened from the queue keeps its keys):

```rust
    if app.playlist_picker.active {
        handle_playlist_picker_input(app, key);
        return;
    }
```

In `src/events/global.rs` `handle_global_key`, above the `KeyCode::Char('n')` arm:

```rust
        // Before the bare `n`/`p` arms: some terminals spell Shift+p as `p` with
        // SHIFT, which would otherwise skip tracks.
        KeyCode::Char('P') | KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.open_playlist_picker();
        }
        KeyCode::Char('N') | KeyCode::Char('n')
            if key.modifiers.contains(KeyModifiers::SHIFT)
                && app.current_tab == crate::app::Tab::Playlists
                && app.view_stack.is_empty() =>
        {
            app.open_name_prompt(None);
        }
```

Off the Playlists tab, an uppercase `N` matches no arm and does nothing. A terminal that spells it as `n` + SHIFT still reaches the bare `n` arm (next track), which is exactly what Shift+n does there today. That's unchanged behaviour, so it needs no extra arm.

- [ ] **Step 6: Implement rendering.** `src/ui/image.rs`, extend `Overlays`:

```rust
    pub(super) const PLAYLIST_PICKER: Self = Self(1 << 6);
    pub(super) const NAME_PROMPT: Self = Self(1 << 7);
```

`src/ui/mod.rs` `draw`: add `.with(Overlays::PLAYLIST_PICKER, app.playlist_picker.active)` and `.with(Overlays::NAME_PROMPT, app.name_prompt.active)` to the chain. In `render_overlays`, after the artist-selection block:

```rust
    if app.playlist_picker.active {
        render_playlist_picker(f, app, area);
    }

    if app.name_prompt.active {
        render_name_prompt(f, app, area);
    }
```

`src/ui/overlays.rs` (it already imports `ellipsize_head`; also bring in `ellipsize` from `row` if not in scope):

```rust
/// First row to draw so `selected` stays inside a window of `visible` rows.
fn picker_scroll(selected: usize, visible: usize) -> usize {
    if visible == 0 {
        return 0;
    }
    selected.saturating_sub(visible - 1)
}

const PLAYLIST_PICKER_W: u16 = 48;
const NEW_PLAYLIST_ROW: &str = "+ New playlist…";

pub(super) fn render_playlist_picker(f: &mut Frame, app: &App, area: Rect) {
    let picker = &app.playlist_picker;
    let box_w = PLAYLIST_PICKER_W.min(area.width.saturating_sub(4));
    let box_h = (2 + picker.rows() as u16).min(area.height.saturating_sub(6)).max(3);
    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(x, y, box_w.min(area.width), box_h.min(area.height));

    f.render_widget(Clear, overlay);
    let track = picker.track.as_ref().map(|t| t.title.as_str()).unwrap_or_default();
    let title = format!(
        " add \"{}\" to ",
        ellipsize(track, box_w.saturating_sub(12))
    );
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let visible = inner.height as usize;
    let first = picker_scroll(picker.selected, visible);
    let labels = std::iter::once(NEW_PLAYLIST_ROW).chain(picker.targets.iter().map(|p| p.title.as_str()));
    for (row, (i, label)) in labels.enumerate().skip(first).take(visible).enumerate() {
        let selected = i == picker.selected;
        let style = if selected {
            Style::default().bg(SELECT_BG).fg(SELECT_FG).add_modifier(Modifier::BOLD)
        } else if i == 0 {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if selected { " ► " } else { "   " };
        let text = ellipsize(label, inner.width.saturating_sub(3));
        f.render_widget(
            Paragraph::new(format!("{prefix}{text}")).style(style),
            Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
        );
    }
}

pub(super) fn render_name_prompt(f: &mut Frame, app: &App, area: Rect) {
    let box_w: u16 = 44u16.min(area.width.saturating_sub(4));
    let box_h: u16 = 3;
    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(x, y, box_w.min(area.width), box_h.min(area.height));

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(
            " new playlist ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let cursor = cursor_char(app.tick);
    let shown = ellipsize_head(&app.name_prompt.input, inner.width.saturating_sub(2) as usize);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(shown, Style::default().fg(Color::White)),
            Span::styled(cursor, Style::default().fg(ACCENT)),
        ])),
        Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(1), 1),
    );
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: all pass, no warnings.

---

### Task 5: Ownership badge on playlist rows and the detail header

**Files:**
- Modify: `src/ui/lists.rs:331-357` (`playlist_row`), tests module ~line 481
- Modify: `src/ui/playlist_detail.rs:89-96` (track-count line)

**Interfaces:**
- Consumes: `Playlist::kind`, `PlaylistKind::label` (Task 1), `App.owned_playlist_ids` (Task 3).
- Produces: nothing new for later tasks.

- [ ] **Step 1: Write the failing test** in the `tests` module of `src/ui/lists.rs`. Update its `playlist` helper literal with `playlist_type: Some("EDITORIAL".to_string()),`, then add:

```rust
    /// Ownership and the list load are separate responses; the badge has to be
    /// right whichever lands second.
    #[test]
    fn the_badge_follows_ownership_learned_after_the_list_loaded() {
        let mut t = test_app();
        std::mem::forget(t.api_rx);
        let p = playlist("Metal Classics", Some(12));
        let text = |app: &crate::app::App| {
            playlist_row(app, &p, 70, false, Style::default())
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        };
        assert!(text(&t.app).contains("Tidal"));
        t.app.owned_playlist_ids.insert("Metal Classics".to_string());
        assert!(text(&t.app).contains("Yours"));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test the_badge_follows_ownership`
Expected: FAIL, the row contains no "Tidal".

- [ ] **Step 3: Implement.** In `src/ui/lists.rs` add a constant beside the other row constants (or above `playlist_row`):

```rust
/// Wide enough for the longest label, "Yours"/"Tidal", plus a gap.
const KIND_BADGE_W: u16 = 6;
```

In `playlist_row`, add a `Cell` between the title and the track count:

```rust
            {
                let kind = playlist.kind(&app.owned_playlist_ids);
                let badge_style = if kind == crate::api::models::PlaylistKind::Yours {
                    row_accent_style(is_selected)
                } else {
                    dim
                };
                Cell::fixed(kind.label(), KIND_BADGE_W, badge_style)
            },
```

In `src/ui/playlist_detail.rs`, replace the `if let Some(count) = …` block with:

```rust
    let kind = detail.playlist.kind(&app.owned_playlist_ids).label();
    let header = match (kind, detail.playlist.track_count_label()) {
        ("", None) => None,
        ("", Some(count)) => Some(count),
        (kind, None) => Some(kind.to_string()),
        (kind, Some(count)) => Some(format!("{kind} · {count}")),
    };
    if let Some(header) = header {
        f.render_widget(
            Paragraph::new(header)
                .style(Style::default().fg(Color::White))
                .alignment(Alignment::Center),
            sections[1],
        );
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: all pass. If an existing `lists.rs` test asserts an exact row string, update its expectation to include the badge column.

---

### Task 6: Help entries, docs, changelog, and the hand-off

**Files:**
- Modify: `src/app/state/keybinds.rs` (new "Playlists" group)
- Modify: `AGENTS.md` (new "Playlist Editing" section)
- Modify: `CHANGELOG.md` (`[Unreleased]` → Added)

- [ ] **Step 1: Help group.** In `src/app/state/keybinds.rs` add `Self::playlists(),` to `all()` after `Self::queue(),` and:

```rust
    pub fn playlists() -> Self {
        KeybindGroup {
            title: "Playlists",
            binds: &[
                Keybind {
                    key: "P",
                    action: "Add track to a playlist",
                },
                Keybind {
                    key: "N",
                    action: "New playlist (Playlists tab)",
                },
            ],
        }
    }
```

Run `cargo test`. If a test pins `total_bind_count()` to a number, raise it by 2.

- [ ] **Step 2: AGENTS.md.** Add this section after "Hiding Atmos Rows":

```markdown
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
```

- [ ] **Step 3: CHANGELOG.** Under `## [Unreleased]` → `### Added` (create the headings if absent, keeping the Added/Changed/Fixed/Internal order):

```markdown
- Playlists can now be created and filled from riptide. Press `P` on any track
  — in your library, search, an album, an artist or the queue — to add it to one
  of your own playlists, or pick "+ New playlist…" to make one on the spot.
  `N` on the Playlists tab creates an empty one. (#57, #71)
- The Playlists tab now labels every playlist as Yours, Tidal (curated), User
  (someone else's) or Mix, so the ones you can edit stand out. (#71)
```

- [ ] **Step 4: Full verification**

Run: `cargo build && cargo test && cargo clippy --all-targets`
Expected: builds, all tests pass, no warnings.

- [ ] **Step 5: Manual TUI pass** (`cargo run`), on a throwaway playlist:
  1. Playlists tab: your own playlists show **Yours** in the accent colour, editorial ones **Tidal**, saved user playlists **User**, mixes **Mix**.
  2. `N` → type `riptide-test` (include a `j`, `k`, `q` and a space) → Enter. The toast says "Created", and the playlist appears with the Yours badge and 0 tracks.
  3. Favorites tab → `P` on a track → the picker lists "+ New playlist…" then only your playlists → choose `riptide-test`. The toast says "Added", and the count reads 1.
  4. Open `riptide-test`: the track is there. Back out, go to Search, `P` on a track result → add it. Then from an album detail, an artist's top tracks and the queue.
  5. `P` → "+ New playlist…" → `riptide-test-2` → the new playlist exists and contains the track.
  6. `P` on the Playlists tab (no track) → toast, no picker.
  7. Esc closes the picker and the prompt without sending anything.
  8. Delete both test playlists in the official app (riptide can't delete until #73).

- [ ] **Step 6: Hand off.** Stop and ask the user to run their own TUI test. Do not commit, and do not ask to commit, until they confirm it works. When they ask, commit with a single-line conventional message, no trailer, e.g. `feat(playlists): create playlists and file any track into your own with P (#71)`.
