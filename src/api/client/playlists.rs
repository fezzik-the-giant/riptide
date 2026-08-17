// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! User playlists and their tracks.

use anyhow::Result;
use std::collections::HashMap;

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
                track_ids.push(track_id.to_string());
            }
        }
    } else {
        tracing::debug!("No items found in data.relationships.items.data");
    }

    tracing::debug!("Extracted {} track IDs: {:?}", track_ids.len(), track_ids);

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
    for track_id in track_ids {
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

                        let artist_name = extract_artist_from_track(track_obj, &artist_map);

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
                            artist: Some(ArtistRef { name: artist_name }),
                            artists: Vec::new(),
                            album: Album {
                                id: album_id,
                                title: album_title.to_string(),
                                number_of_tracks: None,
                                release_date: None,
                                cover: None,
                                artist: None,
                                audio_quality: None,
                                media_metadata: None,
                                added_at: None,
                                album_type: None,
                            },
                            audio_quality: None,
                            media_metadata,
                            added_at: None,
                        });
                    }
                }
            }
        } else {
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

    match &next_url {
        Some(_) => tracing::debug!("Next page URL available"),
        None => tracing::debug!("No more pages"),
    }

    Ok((tracks, total, next_url, description, cover))
}

pub(super) fn parse_playlist_relationship_items(
    api_resp: &serde_json::Value,
    total: u32,
) -> Result<(Vec<Track>, u32, Option<String>)> {
    tracing::debug!("Parsing playlist relationship items response");

    // In relationship responses, items are directly in the data array
    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data").and_then(|v| v.as_array()) {
        tracing::debug!("Found items in data array, count: {}", items_data.len());
        for item_ref in items_data.iter() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                track_ids.push(track_id.to_string());
            }
        }
    } else {
        tracing::debug!("No items found in data array");
    }

    tracing::debug!("Extracted {} track IDs: {:?}", track_ids.len(), track_ids);

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
    for track_id in track_ids {
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

                        let artist_name = extract_artist_from_track(track_obj, &artist_map);

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
                            artist: Some(ArtistRef { name: artist_name }),
                            artists: Vec::new(),
                            album: Album {
                                id: album_id,
                                title: album_title.to_string(),
                                number_of_tracks: None,
                                release_date: None,
                                cover: None,
                                artist: None,
                                audio_quality: None,
                                media_metadata: None,
                                added_at: None,
                                album_type: None,
                            },
                            audio_quality: None,
                            media_metadata,
                            added_at: None,
                        });
                    }
                }
            }
        } else {
            tracing::debug!(
                "Track {} not found in track_map (map has {} entries)",
                track_id,
                track_map.len()
            );
        }
    }

    // Get the next page URL from links.next (at top level for relationship responses)
    let next_url = api_resp
        .get("links")
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match &next_url {
        Some(_) => tracing::debug!("Next page URL available"),
        None => tracing::debug!("No more pages"),
    }

    Ok((tracks, total, next_url))
}

pub(super) fn parse_v2_user_playlists(
    api_resp: &serde_json::Value,
) -> Result<(Vec<Playlist>, u32)> {
    tracing::debug!("Parsing user playlists from JSON:API response");

    let mut playlist_ids = Vec::new();
    let mut added_at_by_id: HashMap<String, String> = HashMap::new();
    if let Some(items_data) = api_resp
        .get("data")
        .and_then(|v| v.get("relationships"))
        .and_then(|v| v.get("items"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
    {
        tracing::debug!(
            "Extracting {} playlist references from relationships",
            items_data.len()
        );
        for item_ref in items_data {
            if let Some(id) = item_ref.get("id").and_then(|v| v.as_str()) {
                playlist_ids.push(id.to_string());
                // addedAt lives on the relationship entry, not on the playlist
                // object in `included`, so capture it here against the id.
                if let Some(added_at) = item_ref
                    .get("meta")
                    .and_then(|m| m.get("addedAt"))
                    .and_then(|v| v.as_str())
                {
                    added_at_by_id.insert(id.to_string(), added_at.to_string());
                }
            }
        }
    }

    let mut playlist_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        tracing::debug!(
            "Building playlist map from {} included items",
            included.len()
        );
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("playlists") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    playlist_map.insert(id.to_string(), item.clone());
                }
            }
        }
        tracing::debug!("Found {} playlists in included array", playlist_map.len());
    }

    let mut playlists = Vec::new();
    for playlist_id in playlist_ids {
        if let Some(playlist_obj) = playlist_map.get(&playlist_id) {
            if let Some(attrs) = playlist_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(name) = attrs.get("name").and_then(|v| v.as_str()) {
                    let number_of_items = attrs
                        .get("numberOfItems")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);

                    playlists.push(Playlist {
                        uuid: playlist_id.clone(),
                        title: name.to_string(),
                        number_of_tracks: number_of_items,
                        description: None,
                        cover: None,
                        added_at: added_at_by_id.get(&playlist_id).cloned(),
                    });

                    tracing::debug!(
                        "Parsed playlist: {} ({} items)",
                        name,
                        number_of_items.unwrap_or(0)
                    );
                }
            }
        }
    }

    let total = api_resp
        .get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|v| v.get("numberOfItems"))
        .and_then(|v| v.as_u64())
        .unwrap_or(playlists.len() as u64) as u32;

    tracing::debug!(
        "Playlist parsing complete: {} playlists parsed (total: {})",
        playlists.len(),
        total
    );

    Ok((playlists, total))
}

impl ApiClient {
    pub async fn get_user_collection_playlists(
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
                })
            })
            .collect();

        let next_cursor = page.links.and_then(|l| l.meta).and_then(|m| m.next_cursor);
        Ok((playlists, next_cursor))
    }

    pub async fn get_favorite_playlists(&self) -> Result<Page<FavoritePlaylistEntry>> {
        tracing::debug!("Fetching user's favorite playlists");

        let token = self.token.read().await.clone();
        let url =
            format!("{OPENAPI_BASE}/userCollectionPlaylists/me?locale=en-US&include=items.items");

        tracing::debug!("API request: GET /userCollectionPlaylists/me");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        tracing::debug!("API response: {} /userCollectionPlaylists/me", status);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!(
                "API error {} on /userCollectionPlaylists/me: {}",
                status,
                body
            );
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (playlists, total) = parse_v2_user_playlists(&api_resp)?;
        tracing::debug!(
            "Fetched {} favorite playlists (total: {})",
            playlists.len(),
            total
        );

        let entries = playlists
            .into_iter()
            .map(|p| FavoritePlaylistEntry {
                created: p.added_at.clone(),
                playlist: p,
            })
            .collect();

        Ok(Page {
            items: entries,
            total,
        })
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
        let token = self.token.read().await.clone();

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
        tracing::debug!("API response: {} /playlists/{uuid}", status);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /playlists/{}: {}", status, uuid, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, total, next_url, description, cover) = parse_v2_playlist_tracks(&api_resp)?;
        if let Some(first_track) = tracks.first() {
            if let Some(ref _url) = next_url {
                tracing::debug!(
                    "Loaded {} tracks, first: '{}', has next page",
                    tracks.len(),
                    first_track.title
                );
            } else {
                tracing::debug!(
                    "Loaded {} tracks, first: '{}', NO more pages",
                    tracks.len(),
                    first_track.title
                );
            }
        }
        Ok((tracks, total, next_url, description, cover))
    }

    pub async fn get_playlist_relationship_items(
        &self,
        next_url: &str,
        total: u32,
    ) -> Result<(Vec<Track>, u32, Option<String>)> {
        let token = self.token.read().await.clone();

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
        tracing::debug!("API response: {} /relationships/items", status);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /relationships/items: {}", status, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, _, next_url) = parse_playlist_relationship_items(&api_resp, total)?;
        if let Some(first_track) = tracks.first() {
            if next_url.is_some() {
                tracing::debug!(
                    "Loaded {} tracks, first: '{}', has next page",
                    tracks.len(),
                    first_track.title
                );
            } else {
                tracing::debug!(
                    "Loaded {} tracks, first: '{}', NO more pages",
                    tracks.len(),
                    first_track.title
                );
            }
        }
        Ok((tracks, total, next_url))
    }
}
