// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Album detail, album tracks, and the saved-albums collection.

use anyhow::{Context, Result};
use std::collections::HashMap;

use super::parse::*;
use super::{ApiClient, OPENAPI_BASE, absolute_url};
use crate::api::models::*;

fn parse_album_tracks(
    api_resp: &serde_json::Value,
    album_id: u64,
) -> Result<(Vec<Track>, Option<String>)> {
    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data").and_then(|v| v.as_array()) {
        for item_ref in items_data.iter() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                track_ids.push(track_id.to_string());
            }
        }
    }

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
                        });
                    }
                }
            }
        }
    }

    let next_url = api_resp
        .get("links")
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((tracks, next_url))
}

impl ApiClient {
    pub async fn get_favorite_albums(
        &self,
        next_url: Option<String>,
    ) -> Result<(Vec<Album>, Option<String>)> {
        let token = self.token.read().await.clone();
        // links.next comes back as a path rather than an absolute URL, matching
        // how the other paginated endpoints here are followed.
        let url = match next_url {
            Some(u) if u.starts_with("http") => u,
            Some(u) => format!("{OPENAPI_BASE}{u}"),
            None => format!(
                "{OPENAPI_BASE}/userCollectionAlbums/me/relationships/items?locale=en-US&sort=-addedAt&include=items.artists,items.coverArt"
            ),
        };
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
            tracing::error!("API error fetching favorite albums: {}", body);
            anyhow::bail!("HTTP {} fetching favorite albums", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let mut albums = Vec::new();
        let mut next_cursor = None;

        // Build album map from included
        let mut album_map = HashMap::new();
        if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
            for item in included {
                if item.get("type").and_then(|v| v.as_str()) == Some("albums") {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        album_map.insert(id.to_string(), item.clone());
                    }
                }
            }
        }

        let artist_map = build_artist_map(&api_resp);

        // Extract album IDs from data array and build Album objects
        if let Some(data_array) = api_resp.get("data").and_then(|v| v.as_array()) {
            for item in data_array {
                if let Some(album_id_str) = item.get("id").and_then(|v| v.as_str()) {
                    if let Some(album_obj) = album_map.get(album_id_str) {
                        if let Some(attrs) = album_obj.get("attributes").and_then(|v| v.as_object())
                        {
                            if let Ok(album_id) = album_id_str.parse::<u64>() {
                                if let Some(title) = attrs.get("title").and_then(|v| v.as_str()) {
                                    let artist_name =
                                        extract_artist_from_album(album_obj, &artist_map);
                                    let cover = extract_cover_from_album(album_obj);
                                    let media_metadata = extract_media_metadata(attrs);

                                    albums.push(Album {
                                        id: album_id,
                                        title: title.to_string(),
                                        number_of_tracks: attrs
                                            .get("numberOfTracks")
                                            .and_then(|v| v.as_u64())
                                            .map(|n| n as u32),
                                        release_date: attrs
                                            .get("releaseDate")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        cover,
                                        artist: artist_name.map(|name| ArtistRef { name }),
                                        media_metadata,
                                        added_at: item
                                            .get("meta")
                                            .and_then(|v| v.get("addedAt"))
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        album_type: attrs
                                            .get("albumType")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Extract next cursor from links
        if let Some(links) = api_resp.get("links") {
            if let Some(next_link) = links.get("next").and_then(|v| v.as_str()) {
                next_cursor = Some(next_link.to_string());
            }
        }

        Ok((albums, next_cursor))
    }

    pub async fn add_favorite_album(&self, album_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": album_id.to_string(), "type": "albums"}]});
        self.post_openapi_json("/userCollectionAlbums/me/relationships/items", &body)
            .await
    }

    pub async fn remove_favorite_album(&self, album_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": album_id.to_string(), "type": "albums"}]});
        self.delete_openapi_json("/userCollectionAlbums/me/relationships/items", &body)
            .await
    }

    pub async fn get_album(&self, album_id: u64) -> Result<(Album, Option<String>)> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}/albums/{album_id}?include=coverArt");

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
            tracing::error!("API error {} fetching album {album_id}: {}", status, body);
            anyhow::bail!("HTTP {} fetching album", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let album_data = api_resp.get("data").context("missing data field")?;
        let album_id = album_data
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .context("missing or invalid album id")?;

        let attrs = album_data.get("attributes").context("missing attributes")?;
        let title = attrs
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let release_date = attrs
            .get("releaseDate")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let number_of_tracks = attrs
            .get("numberOfTracks")
            .or_else(|| attrs.get("numberOfItems"))
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);

        // Extract cover art from the included array
        let mut cover_url: Option<String> = None;
        if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
            for item in included {
                if item.get("type").and_then(|v| v.as_str()) == Some("artworks") {
                    if let Some(files) = item
                        .get("attributes")
                        .and_then(|v| v.get("files"))
                        .and_then(|v| v.as_array())
                    {
                        if let Some(file) = files.iter().find(|f| {
                            f.get("meta")
                                .and_then(|m| m.get("width"))
                                .and_then(|w| w.as_u64())
                                .map(|w| w == 320)
                                .unwrap_or(false)
                        }) {
                            cover_url = file
                                .get("href")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            break;
                        }
                    }
                }
            }
        }
        let album = Album {
            id: album_id,
            title,
            number_of_tracks,
            release_date,
            cover: cover_url.clone(),
            artist: None,
            media_metadata: None,
            added_at: None,
            album_type: None,
        };

        Ok((album, cover_url))
    }

    pub async fn get_album_tracks(&self, album_id: u64) -> Result<Page<Track>> {
        tracing::debug!("Fetching album tracks for album {}", album_id);
        let token = self.token.read().await.clone();
        let mut tracks = Vec::new();
        let mut next_url = Some(format!(
            "{OPENAPI_BASE}/albums/{album_id}/relationships/items?locale=en-US&include=items.albums,items.artists"
        ));

        while let Some(url) = next_url {
            let full_url = absolute_url(&url);
            let resp = self
                .http
                .get(&full_url)
                .bearer_auth(&token)
                .header("Accept", "application/vnd.api+json")
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await?;
                tracing::error!("API error fetching album tracks: {}", body);
                anyhow::bail!("HTTP {} fetching album tracks", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;
            let (page_tracks, next_url_from_response) = parse_album_tracks(&api_resp, album_id)?;

            tracks.extend(page_tracks);

            next_url = next_url_from_response;
        }

        let total = tracks.len() as u32;
        tracing::debug!("Album tracks fully loaded: {} total", total);
        Ok(Page {
            items: tracks,
            total,
        })
    }
}
