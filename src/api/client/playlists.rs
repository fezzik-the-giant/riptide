// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! User playlists and their tracks.

use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use super::parse::*;
use super::{ApiClient, OPENAPI_BASE, OpenApiPlaylistAttrs, OpenApiRelPage};
use crate::api::models::*;

pub(super) fn parse_v2_playlist_tracks(
    api_resp: &serde_json::Value,
) -> Result<(
    Vec<Track>,
    u32,
    Option<String>,
    Option<String>,
    Option<String>,
)> {
    // Extract description and cover from playlist attributes
    let description = api_resp
        .get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract cover art URL from the included array
    let cover = {
        let artwork_id = api_resp
            .get("data")
            .and_then(|v| v.get("relationships"))
            .and_then(|v| v.get("coverArt"))
            .and_then(|v| v.get("data"))
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str());

        if let Some(id) = artwork_id {
            // Find the artwork object in the included array
            api_resp
                .get("included")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter().find(|item| {
                        item.get("id").and_then(|v| v.as_str()) == Some(id)
                            && item.get("type").and_then(|v| v.as_str()) == Some("artworks")
                    })
                })
                .and_then(|artwork| artwork.get("attributes"))
                .and_then(|attrs| attrs.get("files"))
                .and_then(|files| files.as_array())
                .and_then(|arr| {
                    arr.iter().find(|f| {
                        f.get("meta")
                            .and_then(|m| m.get("width"))
                            .and_then(|w| w.as_u64())
                            == Some(320)
                    })
                })
                .and_then(|f| f.get("href"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    };

    // Get track IDs from the playlist's items relationship
    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp
        .get("data")
        .and_then(|v| v.get("relationships"))
        .and_then(|v| v.get("items"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
    {
        tracing::debug!(
            "Found items in data.relationships.items.data, count: {}",
            items_data.len()
        );
        for item_ref in items_data.iter() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                let item_id = item_ref["meta"]["itemId"].as_str().map(str::to_string);
                track_ids.push((track_id.to_string(), item_id));
            }
        }
    }

    // Build a map of track IDs to track details from the included array
    let mut track_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("tracks") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    track_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }

    let artist_map = build_artist_map(api_resp);

    let mut tracks = Vec::new();
    for (track_id, item_id) in track_ids {
        if let Some(track_obj) = track_map.get(&track_id) {
            if let Some(attrs) = track_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(title) = attrs.get("title").and_then(|v| v.as_str()) {
                    if let Ok(id) = track_id.parse::<u64>() {
                        let duration = parse_iso_duration(
                            attrs
                                .get("duration")
                                .and_then(|v| v.as_str())
                                .unwrap_or("PT0S"),
                        );

                        let artists = extract_artists_from_track(track_obj, &artist_map);

                        let album_title = attrs
                            .get("album")
                            .and_then(|v| v.as_object())
                            .and_then(|obj| obj.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");

                        let media_metadata =
                            attrs
                                .get("mediaTags")
                                .and_then(|v| v.as_array())
                                .map(|tags| {
                                    let tag_strs: Vec<String> = tags
                                        .iter()
                                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                        .collect();
                                    MediaMetadata { tags: tag_strs }
                                });

                        let album_id = extract_album_id_from_track(track_obj);
                        tracks.push(Track {
                            id,
                            title: title.to_string(),
                            duration,
                            artist: artists.first().cloned(),
                            artists,
                            album: Album {
                                id: album_id,
                                title: album_title.to_string(),
                                number_of_tracks: None,
                                release_date: None,
                                cover: None,
                                artist: None,
                                media_metadata: None,
                                added_at: None,
                                album_type: None,
                            },
                            media_metadata,
                            added_at: None,
                            playlist_item_id: item_id,
                        });
                    }
                }
            }
        }
    }

    // The playlist's real item count is data.attributes.numberOfItems. This used
    // to read meta.totalNumberOfItems — a v1 field name. v2 sends no top-level
    // `meta` on this endpoint at all, so the lookup never matched and the count
    // silently fell back to the page size (20) instead of the playlist total.
    let total = api_resp
        .get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("numberOfItems"))
        .and_then(|v| v.as_u64())
        .unwrap_or(tracks.len() as u64) as u32;

    // Get the next page URL from data.relationships.items.links.next
    let next_url = api_resp
        .get("data")
        .and_then(|v| v.get("relationships"))
        .and_then(|v| v.get("items"))
        .and_then(|v| v.get("links"))
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((tracks, total, next_url, description, cover))
}

pub(super) fn parse_playlist_relationship_items(
    api_resp: &serde_json::Value,
    total: u32,
) -> Result<(Vec<Track>, u32, Option<String>)> {
    // In relationship responses, items are directly in the data array
    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data").and_then(|v| v.as_array()) {
        for item_ref in items_data.iter() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                let item_id = item_ref["meta"]["itemId"].as_str().map(str::to_string);
                track_ids.push((track_id.to_string(), item_id));
            }
        }
    }

    // Build a map of track IDs to track details from the included array
    let mut track_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("tracks") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    track_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }

    let artist_map = build_artist_map(api_resp);

    let mut tracks = Vec::new();
    for (track_id, item_id) in track_ids {
        if let Some(track_obj) = track_map.get(&track_id) {
            if let Some(attrs) = track_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(title) = attrs.get("title").and_then(|v| v.as_str()) {
                    if let Ok(id) = track_id.parse::<u64>() {
                        let duration = parse_iso_duration(
                            attrs
                                .get("duration")
                                .and_then(|v| v.as_str())
                                .unwrap_or("PT0S"),
                        );

                        let artists = extract_artists_from_track(track_obj, &artist_map);

                        let album_title = attrs
                            .get("album")
                            .and_then(|v| v.as_object())
                            .and_then(|obj| obj.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");

                        let media_metadata =
                            attrs
                                .get("mediaTags")
                                .and_then(|v| v.as_array())
                                .map(|tags| {
                                    let tag_strs: Vec<String> = tags
                                        .iter()
                                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                        .collect();
                                    MediaMetadata { tags: tag_strs }
                                });

                        let album_id = extract_album_id_from_track(track_obj);
                        tracks.push(Track {
                            id,
                            title: title.to_string(),
                            duration,
                            artist: artists.first().cloned(),
                            artists,
                            album: Album {
                                id: album_id,
                                title: album_title.to_string(),
                                number_of_tracks: None,
                                release_date: None,
                                cover: None,
                                artist: None,
                                media_metadata: None,
                                added_at: None,
                                album_type: None,
                            },
                            media_metadata,
                            added_at: None,
                            playlist_item_id: item_id,
                        });
                    }
                }
            }
        }
    }

    // Get the next page URL from links.next (at top level for relationship responses)
    let next_url = api_resp
        .get("links")
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((tracks, total, next_url))
}

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

fn remove_item_body(track_id: u64, item_id: &str) -> serde_json::Value {
    serde_json::json!({"data": [{"id": track_id.to_string(), "type": "tracks",
        "meta": {"itemId": item_id}}]})
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

impl ApiClient {
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

    pub async fn remove_from_playlist(
        &self,
        uuid: &str,
        track_id: u64,
        item_id: &str,
    ) -> Result<()> {
        self.send_openapi_json(
            reqwest::Method::DELETE,
            &format!("/playlists/{uuid}/relationships/items"),
            &remove_item_body(track_id, item_id),
        )
        .await?;
        tracing::debug!("removed item {item_id} (track {track_id}) from playlist {uuid}");
        Ok(())
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

    /// Drains every page of the collection. Callers get the whole thing, so a
    /// forgotten cursor cannot silently truncate the library — which is exactly
    /// what capped this list at one page before.
    pub async fn get_user_collection_playlists(&self) -> Result<Vec<Playlist>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let (page, next) = self
                .user_collection_playlists_page(cursor.as_deref())
                .await?;
            all.extend(page);
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }

        tracing::info!("loaded {} playlists from the collection", all.len());
        Ok(all)
    }

    async fn user_collection_playlists_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<(Vec<Playlist>, Option<String>)> {
        let mut params = vec![("include", "items".to_string())];
        if let Some(c) = cursor {
            params.push(("page[cursor]", c.to_string()));
        }

        let page: OpenApiRelPage = self
            .get_openapi("/userCollectionPlaylists/me/relationships/items", &params)
            .await?;

        let attrs: std::collections::HashMap<String, OpenApiPlaylistAttrs> = page
            .included
            .into_iter()
            .filter_map(|r| r.attributes.map(|a| (r.id, a)))
            .collect();

        let playlists = page
            .data
            .into_iter()
            .filter_map(|r| {
                let attr = attrs.get(&r.id)?;
                let added_at = r.meta.and_then(|m| m.added_at);
                Some(Playlist {
                    uuid: r.id,
                    title: attr.name.clone(),
                    number_of_tracks: attr.number_of_items,
                    description: None,
                    cover: None,
                    added_at,
                    playlist_type: attr.playlist_type.clone(),
                })
            })
            .collect();

        let next_cursor = page.links.and_then(|l| l.meta).and_then(|m| m.next_cursor);
        Ok((playlists, next_cursor))
    }

    pub async fn save_playlist(&self, uuid: &str) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": uuid, "type": "playlists"}]});
        self.post_openapi_json("/userCollectionPlaylists/me/relationships/items", &body)
            .await
    }

    pub async fn remove_playlist(&self, uuid: &str) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": uuid, "type": "playlists"}]});
        self.delete_openapi_json("/userCollectionPlaylists/me/relationships/items", &body)
            .await
    }

    pub async fn get_playlist_tracks(
        &self,
        uuid: &str,
        next_url: Option<&str>,
    ) -> Result<(
        Vec<Track>,
        u32,
        Option<String>,
        Option<String>,
        Option<String>,
    )> {
        let token = self.access_token().await?;

        let url = if let Some(next) = next_url {
            // Use the next URL provided by the previous response
            // Ensure it includes the include=items.artists parameter
            let base_url = format!("{OPENAPI_BASE}{}", next);
            if base_url.contains("include=") {
                base_url
            } else {
                format!("{}&include=items.artists", base_url)
            }
        } else {
            // Initial request - build the first page URL
            format!(
                "{OPENAPI_BASE}/playlists/{uuid}?countryCode=US&include=items.artists&include=coverArt"
            )
        };

        tracing::debug!("Fetching playlist tracks from: {}", url);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /playlists/{}: {}", status, uuid, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, total, next_url, description, cover) = parse_v2_playlist_tracks(&api_resp)?;
        tracing::debug!(
            "playlist page: {} tracks of {total}, more pages: {}",
            tracks.len(),
            next_url.is_some()
        );
        Ok((tracks, total, next_url, description, cover))
    }

    pub async fn get_playlist_relationship_items(
        &self,
        next_url: &str,
        total: u32,
    ) -> Result<(Vec<Track>, u32, Option<String>)> {
        let token = self.access_token().await?;

        // Ensure the next URL includes the include=items.artists parameter
        let url = {
            let base_url = format!("{OPENAPI_BASE}{}", next_url);
            if base_url.contains("include=") {
                base_url
            } else {
                format!("{}&include=items.artists", base_url)
            }
        };
        tracing::debug!("Fetching playlist relationship items from: {}", url);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /relationships/items: {}", status, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, _, next_url) = parse_playlist_relationship_items(&api_resp, total)?;
        tracing::debug!(
            "playlist items page: {} tracks of {total}, more pages: {}",
            tracks.len(),
            next_url.is_some()
        );
        Ok((tracks, total, next_url))
    }
}

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

    /// Live `GET /playlists/{id}?include=items` shape (2026-10-19), trimmed, holding
    /// one track twice with the two `itemId`s from the 2026-10-15 duplicates probe.
    const PARENT_WITH_DUPLICATE: &str = r#"{"data":{"id":"00a62462-018f-4239-b6da-111ef4f50490","type":"playlists","attributes":{"name":"dopeshit","numberOfItems":2},"relationships":{"items":{"data":[{"id":"431291038","type":"tracks","meta":{"itemId":"e2df580f-28c7-49ed-950a-e19cb8d761bd","addedAt":"2026-10-15T18:20:28.091909Z"}},{"id":"431291038","type":"tracks","meta":{"itemId":"95eb7254-0e58-401a-82f5-163c2342821a","addedAt":"2026-10-15T18:20:28.607130Z"}}],"links":{"self":"/playlists/00a62462-018f-4239-b6da-111ef4f50490/relationships/items?countryCode=US"}}}},"included":[{"id":"431291038","type":"tracks","attributes":{"title":"In the Name of the Father","duration":"PT3M47S","mediaTags":["LOSSLESS"]}}]}"#;

    /// Live `GET /playlists/{id}/relationships/items?include=items` shape
    /// (2026-10-19), trimmed, with the same duplicate.
    const PAGE_WITH_DUPLICATE: &str = r#"{"data":[{"id":"431291038","type":"tracks","meta":{"itemId":"e2df580f-28c7-49ed-950a-e19cb8d761bd","addedAt":"2026-10-15T18:20:28.091909Z"}},{"id":"431291038","type":"tracks","meta":{"itemId":"95eb7254-0e58-401a-82f5-163c2342821a","addedAt":"2026-10-15T18:20:28.607130Z"}}],"links":{"self":"/playlists/00a62462-018f-4239-b6da-111ef4f50490/relationships/items?include=items&countryCode=US"},"included":[{"id":"431291038","type":"tracks","attributes":{"title":"In the Name of the Father","duration":"PT3M47S"}}]}"#;

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

    #[test]
    fn both_playlist_parsers_keep_each_entry_s_item_id() {
        let (first, ..) = parse_v2_playlist_tracks(&json(PARENT_WITH_DUPLICATE)).unwrap();
        let (cursor, ..) =
            parse_playlist_relationship_items(&json(PAGE_WITH_DUPLICATE), 2).unwrap();
        for tracks in [first, cursor] {
            assert_eq!(
                tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
                [431291038, 431291038]
            );
            assert_eq!(
                tracks
                    .iter()
                    .map(|t| t.playlist_item_id.as_deref())
                    .collect::<Vec<_>>(),
                [
                    Some("e2df580f-28c7-49ed-950a-e19cb8d761bd"),
                    Some("95eb7254-0e58-401a-82f5-163c2342821a")
                ]
            );
        }
    }

    /// Tidal rejects a remove without `meta.itemId` (`400 Must not be null`).
    #[test]
    fn a_remove_names_the_entry_by_item_id() {
        assert_eq!(
            remove_item_body(431291038, "e2df580f-28c7-49ed-950a-e19cb8d761bd"),
            serde_json::json!({"data": [{"id": "431291038", "type": "tracks",
                "meta": {"itemId": "e2df580f-28c7-49ed-950a-e19cb8d761bd"}}]})
        );
    }
}
