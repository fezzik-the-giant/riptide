// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Track detail, lyrics, and the favourite-tracks collection.

use anyhow::{Context, Result};
use std::collections::HashMap;

use super::parse::*;
use super::{ApiClient, OPENAPI_BASE};
use crate::api::models::*;

fn parse_v2_user_collection_tracks(
    api_resp: &serde_json::Value,
) -> Result<(Vec<Track>, u32, Option<String>)> {
    tracing::debug!("Parsing favorite tracks from JSON:API response");

    let mut track_ids = Vec::new();
    let mut added_at_map = HashMap::new();

    // Handle both response structures:
    // 1. Collection endpoint (/userCollectionTracks/me): data.relationships.items.data
    // 2. Relationship endpoint (/userCollectionTracks/{id}/relationships/items): data directly
    let items_data = api_resp.get("data").and_then(|v| {
        // First try collection endpoint structure
        v.get("relationships")
            .and_then(|r| r.get("items"))
            .and_then(|i| i.get("data"))
            .and_then(|d| d.as_array())
            .or_else(|| {
                // Fall back to relationship endpoint structure
                v.as_array()
            })
    });

    if let Some(items) = items_data {
        tracing::debug!("Extracting {} track references", items.len());
        for item_ref in items {
            if let Some(id) = item_ref.get("id").and_then(|v| v.as_str()) {
                track_ids.push(id.to_string());
                if let Some(added_at) = item_ref
                    .get("meta")
                    .and_then(|m| m.get("addedAt"))
                    .and_then(|v| v.as_str())
                {
                    added_at_map.insert(id.to_string(), added_at.to_string());
                }
            }
        }
    }

    // Build maps from included objects
    let mut track_map = HashMap::new();
    let mut artist_map = HashMap::new();

    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        tracing::debug!("Parsing {} included objects", included.len());
        for item in included {
            if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
                tracing::debug!("Included object type: {}", item_type);
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if item_type == "tracks" {
                        track_map.insert(id.to_string(), item.clone());
                    } else if item_type == "artists" {
                        artist_map.insert(id.to_string(), item.clone());
                    }
                }
            }
        }
        tracing::debug!(
            "Built maps: {} tracks, {} artists",
            track_map.len(),
            artist_map.len()
        );
    } else {
        tracing::debug!("No included array in response");
    }

    // Build track objects from IDs and included data
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
                        if artist_name == "Unknown" {
                            tracing::debug!(
                                "Track '{}' - Artist relationships: {:?}, Artist map size: {}",
                                title,
                                track_obj
                                    .get("relationships")
                                    .and_then(|r| r.get("artists"))
                                    .is_some(),
                                artist_map.len()
                            );
                        }

                        let album_title = attrs
                            .get("album")
                            .and_then(|v| v.as_object())
                            .and_then(|obj| obj.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let album = Album {
                            id: 0,
                            title: album_title,
                            number_of_tracks: None,
                            release_date: None,
                            cover: None,
                            artist: if artist_name == "Unknown" {
                                None
                            } else {
                                Some(ArtistRef {
                                    name: artist_name.clone(),
                                })
                            },
                            audio_quality: None,
                            media_metadata: None,
                            added_at: None,
                            album_type: None,
                        };

                        let track = Track {
                            id,
                            title: title.to_string(),
                            duration,
                            artist: if artist_name == "Unknown" {
                                None
                            } else {
                                Some(ArtistRef {
                                    name: artist_name.clone(),
                                })
                            },
                            artists: Vec::new(),
                            album,
                            audio_quality: None,
                            media_metadata: extract_media_metadata(attrs),
                            added_at: added_at_map.get(&track_id).cloned(),
                        };
                        tracks.push(track);
                    }
                }
            }
        }
    }

    let total = api_resp
        .get("data")
        .and_then(|v| v.get("attributes"))
        .and_then(|a| a.get("numberOfItems"))
        .and_then(|n| n.as_u64())
        .unwrap_or(tracks.len() as u64) as u32;

    let next_url = api_resp
        .get("data")
        .and_then(|v| {
            // First try collection endpoint structure
            v.get("relationships")
                .and_then(|r| r.get("items"))
                .and_then(|i| i.get("links"))
                .and_then(|l| l.get("next"))
                .and_then(|n| n.as_str())
                .or_else(|| {
                    // Fall back to relationship endpoint structure (links at top level of response)
                    api_resp
                        .get("links")
                        .and_then(|l| l.get("next"))
                        .and_then(|n| n.as_str())
                })
        })
        .map(|s| s.to_string());

    tracing::debug!(
        "Track parsing complete: {} tracks parsed (total: {})",
        tracks.len(),
        total
    );

    Ok((tracks, total, next_url))
}

fn parse_v2_track_details(api_resp: &serde_json::Value) -> Result<(Track, Option<String>)> {
    let track_data = api_resp.get("data").context("missing data field")?;
    let track_id = track_data
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .context("missing or invalid track id")?;

    let attrs = track_data.get("attributes").context("missing attributes")?;
    let title = attrs
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let duration_str = attrs
        .get("duration")
        .and_then(|v| v.as_str())
        .unwrap_or("PT0S");
    let duration = parse_iso_duration(duration_str);

    let album_id = track_data
        .get("relationships")
        .and_then(|v| v.get("albums"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut album = Album {
        id: album_id,
        title: "Unknown".to_string(),
        number_of_tracks: None,
        release_date: None,
        cover: None,
        artist: None,
        audio_quality: None,
        media_metadata: None,
        added_at: None,
        album_type: None,
    };

    let mut cover_url: Option<String> = None;
    let mut artist: Option<ArtistRef> = None;
    let mut artists: Vec<ArtistRef> = Vec::new();
    let mut artist_map = HashMap::new();

    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if let Some("artists") = item.get("type").and_then(|v| v.as_str()) {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if let Some(attrs) = item.get("attributes").and_then(|v| v.as_object()) {
                        let name = attrs
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        artist_map.insert(id.to_string(), name);
                    }
                }
            } else if let Some("albums") = item.get("type").and_then(|v| v.as_str()) {
                if let Some(item_id) = item.get("id").and_then(|v| v.as_str()) {
                    if item_id.parse::<u64>().ok() == Some(album_id) {
                        if let Some(attrs) = item.get("attributes").and_then(|v| v.as_object()) {
                            album.title = attrs
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            album.release_date = attrs
                                .get("releaseDate")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                    }
                }
            } else if let Some("artworks") = item.get("type").and_then(|v| v.as_str()) {
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
                    }
                }
            }
        }
    }

    if let Some(artist_ids) = track_data
        .get("relationships")
        .and_then(|v| v.get("artists"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
    {
        for artist_ref in artist_ids {
            if let Some(id) = artist_ref.get("id").and_then(|v| v.as_str()) {
                if let Some(name) = artist_map.get(id) {
                    let artist_ref = ArtistRef { name: name.clone() };
                    if artist.is_none() {
                        artist = Some(artist_ref.clone());
                    }
                    artists.push(artist_ref);
                }
            }
        }
    }

    let track = Track {
        id: track_id,
        title,
        duration,
        artist,
        artists,
        album,
        audio_quality: None,
        media_metadata: None,
        added_at: None,
    };

    Ok((track, cover_url))
}

impl ApiClient {
    pub async fn get_favorite_tracks(&self) -> Result<(Vec<Track>, u32)> {
        tracing::debug!("Fetching all favorite tracks");
        tracing::debug!("API request: GET /userCollectionTracks/me (v2)");

        let token = self.token.read().await.clone();
        let mut all_tracks = Vec::new();
        let mut total = 0u32;
        let mut next_url: Option<String> = Some(format!(
            "{OPENAPI_BASE}/userCollectionTracks/me?locale=en-US&include=items,items.artists"
        ));

        while let Some(mut url) = next_url {
            if !url.contains("include=") {
                let separator = if url.contains("?") { "&" } else { "?" };
                url = format!("{}{}include=items,items.artists", url, separator);
            }

            let full_url = if url.starts_with("http") {
                url.clone()
            } else {
                format!("{OPENAPI_BASE}{url}")
            };

            tracing::debug!("Fetching favorite tracks page: {}", full_url);

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
                tracing::error!("API error on /userCollectionTracks/me: {}", body);
                anyhow::bail!("HTTP {}", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;
            let (tracks, resp_total, next_cursor) = parse_v2_user_collection_tracks(&api_resp)?;

            all_tracks.extend(tracks);
            total = total.max(resp_total);
            next_url = next_cursor;

            if next_url.is_some() {
                tracing::debug!("More favorite tracks available, continuing pagination");
            } else {
                tracing::debug!("No more pages");
            }
        }

        tracing::debug!(
            "Fetched {} favorite tracks (total: {})",
            all_tracks.len(),
            total
        );
        Ok((all_tracks, total))
    }

    pub async fn get_track_lyrics(&self, track_id: u64) -> Result<LyricsResponse> {
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/tracks/{track_id}/relationships/lyrics?countryCode={}&include=lyrics",
            self.config.country_code
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
            tracing::debug!("API response for lyrics: {}", body);
            return Ok(LyricsResponse {
                lyrics: None,
                subtitles: None,
            });
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
            if let Some(lyrics_obj) = included.first() {
                if let Some(attributes) = lyrics_obj.get("attributes") {
                    let text = attributes
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let lrc = attributes
                        .get("lrcText")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Ok(LyricsResponse {
                        lyrics: text,
                        subtitles: lrc,
                    });
                }
            }
        }

        Ok(LyricsResponse {
            lyrics: None,
            subtitles: None,
        })
    }

    pub async fn add_favorite_track(&self, track_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": track_id.to_string(), "type": "tracks"}]});
        self.post_openapi_json("/userCollectionTracks/me/relationships/items", &body)
            .await
    }

    pub async fn remove_favorite_track(&self, track_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": track_id.to_string(), "type": "tracks"}]});
        self.delete_openapi_json("/userCollectionTracks/me/relationships/items", &body)
            .await
    }

    pub async fn get_track_details(&self, track_id: u64) -> Result<(Track, Option<String>)> {
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/tracks/{track_id}?countryCode=US&include=albums.coverArt&include=artists"
        );

        tracing::debug!("Fetching track details for track {track_id}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("API error {} fetching track {track_id}: {}", status, body);
            anyhow::bail!("HTTP {} fetching track details", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (track, cover_url) = parse_v2_track_details(&api_resp)?;
        Ok((track, cover_url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed capture of `GET /userCollectionTracks/me?include=items,items.artists`.
    fn collection_page() -> serde_json::Value {
        serde_json::json!({
            "data": {
                "id": "Hn4cnTxVVZDk",
                "type": "userCollectionTracks",
                "attributes": { "numberOfItems": 2 },
                "relationships": { "items": { "data": [
                    { "id": "178390467", "type": "tracks", "meta": { "addedAt": "2026-08-13T16:40:12.607Z" } },
                    { "id": "77712754",  "type": "tracks", "meta": { "addedAt": "2026-08-12T09:15:00.000Z" } }
                ] } }
            },
            "included": [
                { "id": "178390467", "type": "tracks", "attributes": {
                    "title": "Lanterns", "duration": "PT3M58S",
                    "mediaTags": ["HIRES_LOSSLESS", "LOSSLESS"] } },
                { "id": "77712754", "type": "tracks", "attributes": {
                    "title": "Hail to the King", "duration": "PT5M04S",
                    "mediaTags": ["LOSSLESS"] } }
            ]
        })
    }

    #[test]
    fn favorite_tracks_carry_quality_badges() {
        let (tracks, _, _) = parse_v2_user_collection_tracks(&collection_page()).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].quality_badge(), Some("MAX"));
        assert_eq!(tracks[1].quality_badge(), Some("HI-FI"));
    }

    #[test]
    fn favorite_tracks_report_the_collection_count() {
        let (_, total, _) = parse_v2_user_collection_tracks(&collection_page()).unwrap();
        assert_eq!(total, 2);
    }
}
