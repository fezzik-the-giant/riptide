// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Track and artist radio: server-generated mixes seeded from one item.

use anyhow::Result;
use std::collections::HashMap;

use super::parse::*;
use super::{ApiClient, OPENAPI_BASE};
use crate::api::models::*;

fn parse_radio_response(api_resp: &serde_json::Value) -> Result<Vec<Track>> {
    // Radio endpoint returns a playlist in data, with tracks in playlist.relationships.items.data
    let mut track_ids = Vec::new();

    // Find the playlist in the included array
    let playlist_obj = if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        included
            .iter()
            .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("playlists"))
    } else {
        None
    };

    if let Some(playlist) = playlist_obj {
        if let Some(track_refs) = playlist
            .get("relationships")
            .and_then(|v| v.get("items"))
            .and_then(|v| v.get("data"))
            .and_then(|v| v.as_array())
        {
            for track_ref in track_refs.iter() {
                if let Some(track_id) = track_ref.get("id").and_then(|v| v.as_str()) {
                    track_ids.push(track_id.to_string());
                }
            }
        }
    } else {
        tracing::debug!("No playlist found in included array");
    }

    tracing::debug!(
        "Extracted {} track IDs from radio playlist",
        track_ids.len()
    );

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
                        });
                    }
                }
            }
        }
    }
    Ok(tracks)
}

impl ApiClient {
    pub async fn get_track_radio(&self, track_id: u64) -> Result<Page<Track>> {
        tracing::debug!("Fetching radio for track {}", track_id);
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/tracks/{track_id}/relationships/radio?locale=en-US&include=radio.items.albums,radio.items.artists"
        );
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
            tracing::error!(
                "API error on /tracks/{}/relationships/radio: {}",
                track_id,
                body
            );
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let tracks = parse_radio_response(&api_resp)?;
        tracing::debug!("Track radio parsed {} tracks", tracks.len());
        let total = tracks.len() as u32;
        Ok(Page {
            items: tracks,
            total,
        })
    }

    pub async fn get_artist_radio(&self, artist_id: u64) -> Result<Page<Track>> {
        tracing::debug!("Fetching radio for artist {}", artist_id);
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/artists/{artist_id}/relationships/radio?locale=en-US&include=radio.items.albums,radio.items.artists"
        );
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
            tracing::error!(
                "API error on /artists/{}/relationships/radio: {}",
                artist_id,
                body
            );
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let tracks = parse_radio_response(&api_resp)?;
        tracing::debug!("Artist radio parsed {} tracks", tracks.len());
        let total = tracks.len() as u32;
        Ok(Page {
            items: tracks,
            total,
        })
    }
}
