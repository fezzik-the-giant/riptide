// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use tokio::sync::RwLock;

use super::models::*;

const BASE: &str = "https://api.tidal.com/v1";
const OPENAPI_BASE: &str = "https://openapi.tidal.com/v2";

// Private types for the openapi.tidal.com/v2 JSON:API collection endpoints.
#[derive(serde::Deserialize)]
struct OpenApiRelPage {
    data: Vec<OpenApiRelItem>,
    #[serde(default)]
    included: Vec<OpenApiIncluded>,
    links: Option<OpenApiLinks>,
}

#[derive(serde::Deserialize)]
struct OpenApiRelItem {
    id: String,
    meta: Option<OpenApiItemMeta>,
}

#[derive(serde::Deserialize)]
struct OpenApiItemMeta {
    #[serde(rename = "addedAt")]
    added_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct OpenApiIncluded {
    id: String,
    attributes: Option<OpenApiPlaylistAttrs>,
}

#[derive(serde::Deserialize)]
struct OpenApiPlaylistAttrs {
    name: String,
    #[serde(rename = "numberOfItems")]
    number_of_items: Option<u32>,
}

#[derive(serde::Deserialize)]
struct OpenApiLinks {
    meta: Option<OpenApiLinksMeta>,
}

#[derive(serde::Deserialize)]
struct OpenApiLinksMeta {
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36";

fn parse_iso_duration(s: &str) -> u32 {
    // Parse ISO 8601 duration format (e.g., "PT5M10S" = 5 min 10 sec = 310 sec)
    let s = s.trim_start_matches('P').trim_start_matches('T');
    let mut seconds = 0u32;
    let mut current_num = String::new();

    for ch in s.chars() {
        match ch {
            '0'..='9' => current_num.push(ch),
            'H' => {
                if let Ok(hours) = current_num.parse::<u32>() {
                    seconds += hours * 3600;
                }
                current_num.clear();
            }
            'M' => {
                if let Ok(minutes) = current_num.parse::<u32>() {
                    seconds += minutes * 60;
                }
                current_num.clear();
            }
            'S' => {
                if let Ok(secs) = current_num.parse::<u32>() {
                    seconds += secs;
                }
                current_num.clear();
            }
            _ => {}
        }
    }
    seconds
}

fn extract_artist_from_track(
    track_obj: &serde_json::Value,
    artist_map: &HashMap<String, serde_json::Value>,
) -> String {
    track_obj
        .get("relationships")
        .and_then(|v| v.get("artists"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|artist_id| artist_map.get(artist_id))
        .and_then(|artist_obj| artist_obj.get("attributes"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string()
}

fn extract_artist_from_album(
    album_obj: &serde_json::Value,
    artist_map: &HashMap<String, serde_json::Value>,
) -> Option<String> {
    album_obj
        .get("relationships")
        .and_then(|v| v.get("artists"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|artist_id| artist_map.get(artist_id))
        .and_then(|artist_obj| artist_obj.get("attributes"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_cover_from_album(album_obj: &serde_json::Value) -> Option<String> {
    album_obj
        .get("relationships")
        .and_then(|v| v.get("coverArt"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_media_metadata(
    attrs: &serde_json::Map<String, serde_json::Value>,
) -> Option<MediaMetadata> {
    attrs
        .get("mediaTags")
        .and_then(|v| v.as_array())
        .map(|tags| {
            let tag_strs: Vec<String> = tags
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect();
            MediaMetadata { tags: tag_strs }
        })
}

fn extract_album_id_from_track(track_obj: &serde_json::Value) -> u64 {
    track_obj
        .get("relationships")
        .and_then(|v| v.get("albums"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

fn build_artist_map(api_resp: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    let mut artist_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("artists") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    artist_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }
    artist_map
}

fn parse_v2_playlist_tracks(
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

    let total = api_resp
        .get("meta")
        .and_then(|v| v.get("totalNumberOfItems"))
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

fn parse_radio_response(api_resp: &serde_json::Value) -> Result<Vec<Track>> {
    tracing::debug!("Parsing radio response");

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
            tracing::debug!("Found {} track references in playlist", track_refs.len());
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
        tracing::debug!("Built track_map with {} tracks", track_map.len());
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
        }
    }

    tracing::debug!("Parsed {} radio tracks total", tracks.len());
    Ok(tracks)
}

fn parse_artist_albums(api_resp: &serde_json::Value) -> Result<Vec<Album>> {
    let mut album_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data").and_then(|v| v.as_array()) {
        for item_ref in items_data.iter() {
            if let Some(album_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                album_ids.push(album_id.to_string());
            }
        }
    }

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

    let mut albums = Vec::new();
    for album_id in album_ids {
        if let Some(album_obj) = album_map.get(&album_id) {
            if let Some(attrs) = album_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(title) = attrs.get("title").and_then(|v| v.as_str()) {
                    if let Ok(id) = album_id.parse::<u64>() {
                        let album_type = attrs
                            .get("albumType")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let number_of_items = attrs
                            .get("numberOfItems")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32);

                        let release_date = attrs
                            .get("releaseDate")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

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

                        albums.push(Album {
                            id,
                            title: title.to_string(),
                            number_of_tracks: number_of_items,
                            release_date,
                            cover: None,
                            artist: None,
                            audio_quality: None,
                            media_metadata,
                            added_at: None,
                            album_type,
                        });
                    }
                }
            }
        }
    }

    Ok(albums)
}

fn parse_album_tracks(
    api_resp: &serde_json::Value,
    album_id: u64,
) -> Result<(Vec<Track>, Option<String>)> {
    tracing::debug!("Parsing album tracks response");

    let mut track_ids = Vec::new();
    if let Some(items_data) = api_resp.get("data").and_then(|v| v.as_array()) {
        tracing::debug!("Found items in data array, count: {}", items_data.len());
        for item_ref in items_data.iter() {
            if let Some(track_id) = item_ref.get("id").and_then(|v| v.as_str()) {
                track_ids.push(track_id.to_string());
            }
        }
    }

    tracing::debug!("Extracted {} track IDs", track_ids.len());

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
        }
    }

    let next_url = api_resp
        .get("links")
        .and_then(|v| v.get("next"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((tracks, next_url))
}

fn parse_playlist_relationship_items(
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

fn parse_v2_user_playlists(api_resp: &serde_json::Value) -> Result<(Vec<Playlist>, u32)> {
    tracing::debug!("Parsing user playlists from JSON:API response");

    let mut playlist_ids = Vec::new();
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
            }
            if let Some(added_at) = item_ref
                .get("meta")
                .and_then(|m| m.get("addedAt"))
                .and_then(|v| v.as_str())
            {
                tracing::debug!("Playlist added at: {}", added_at);
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
                        added_at: None,
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

fn parse_v2_user_collection_artists(
    api_resp: &serde_json::Value,
) -> Result<(Vec<Artist>, u32, Option<String>)> {
    tracing::debug!("Parsing favorite artists from JSON:API response");

    let mut artist_ids = Vec::new();
    let mut added_at_map = HashMap::new();

    // Handle both response structures:
    // 1. Collection endpoint (/userCollectionArtists/me): data.relationships.items.data
    // 2. Relationship endpoint (/userCollectionArtists/{id}/relationships/items): data directly
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
        tracing::debug!(
            "Extracting {} artist references from relationships",
            items.len()
        );
        for item_ref in items {
            if let Some(id) = item_ref.get("id").and_then(|v| v.as_str()) {
                artist_ids.push(id.to_string());
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

    let mut artist_map = HashMap::new();
    if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
        for item in included {
            if item.get("type").and_then(|v| v.as_str()) == Some("artists") {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    artist_map.insert(id.to_string(), item.clone());
                }
            }
        }
    }

    let mut artists = Vec::new();
    for artist_id in artist_ids {
        if let Some(artist_obj) = artist_map.get(&artist_id) {
            if let Some(attrs) = artist_obj.get("attributes").and_then(|v| v.as_object()) {
                if let Some(name) = attrs.get("name").and_then(|v| v.as_str()) {
                    let artist = Artist {
                        id: artist_id.parse().unwrap_or(0),
                        name: name.to_string(),
                        added_at: added_at_map.get(&artist_id).cloned(),
                    };
                    artists.push(artist);
                }
            }
        }
    }

    let total = api_resp
        .get("data")
        .and_then(|v| {
            // Collection endpoint has numberOfItems in data.attributes
            v.get("attributes")
                .and_then(|a| a.get("numberOfItems"))
                .and_then(|n| n.as_u64())
                .or_else(|| {
                    // Relationship endpoint may have it in meta
                    v.get("meta")
                        .and_then(|m| m.get("totalNumberOfItems"))
                        .and_then(|n| n.as_u64())
                })
        })
        .unwrap_or(artists.len() as u64) as u32;

    // Handle both response structures for next_url
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
        "Artist parsing complete: {} artists parsed (total: {})",
        artists.len(),
        total
    );

    Ok((artists, total, next_url))
}

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
                            audio_quality: attrs
                                .get("audioQuality")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
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
                            audio_quality: attrs
                                .get("audioQuality")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            media_metadata: None,
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

#[derive(Debug)]
pub struct SearchTrackPage {
    pub tracks: Vec<Track>,
    #[allow(dead_code)]
    pub total: u32,
    pub next_url: Option<String>,
}

#[derive(Debug)]
pub struct SearchArtistPage {
    pub artists: Vec<Artist>,
    #[allow(dead_code)]
    pub total: u32,
    pub next_url: Option<String>,
}

#[derive(Debug)]
pub struct SearchPlaylistPage {
    pub playlists: Vec<Playlist>,
    #[allow(dead_code)]
    pub total: u32,
    pub next_url: Option<String>,
}

fn parse_search_track_page(api_resp: &serde_json::Value) -> Result<SearchTrackPage> {
    let data_array = api_resp["data"]
        .as_array()
        .context("data is not an array")?;

    // Extract track refs, next URL, and included based on response structure
    let empty_array = Vec::new();
    let (track_refs_src, next_url, included_opt, _total_count) = if data_array.is_empty() {
        (&empty_array, None, None, 0u32)
    } else if data_array[0].get("relationships").is_some() {
        // Initial search response: data is array with searchResults object that has relationships
        let rels = &data_array[0]["relationships"]["tracks"];
        let refs = rels["data"].as_array().context("missing tracks data")?;
        let next = rels["links"]["next"].as_str().map(String::from);
        let inc = api_resp["included"]
            .as_array()
            .context("missing included")?;

        // Extract total from searchResults attributes
        let total = data_array[0]["attributes"]["totalNumberOfResults"]
            .as_u64()
            .unwrap_or(0) as u32;
        tracing::debug!(
            "Initial search response: {} tracks loaded, {} total results",
            refs.len(),
            total
        );

        (refs, next, Some(inc), total)
    } else {
        // Pagination response: data is array of track IDs
        let next = api_resp["links"]["next"].as_str().map(String::from);
        let inc = api_resp["included"].as_array();
        (data_array, next, inc, 0)
    };

    let mut tracks = Vec::new();
    let mut artist_map = std::collections::HashMap::new();
    let mut track_map = std::collections::HashMap::new();

    if let Some(included) = included_opt {
        for item in included {
            if item["type"] == "artists" {
                if let Ok(id) = item["id"]
                    .as_str()
                    .context("missing artist id")?
                    .parse::<u64>()
                {
                    let name = item["attributes"]["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let artist = Artist {
                        id,
                        name,
                        added_at: None,
                    };
                    artist_map.insert(id, artist);
                }
            }
        }

        for item in included {
            if item["type"] == "tracks" {
                let id = item["id"]
                    .as_str()
                    .context("missing track id")?
                    .parse::<u64>()?;
                let title = item["attributes"]["title"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let duration_str = item["attributes"]["duration"].as_str().unwrap_or("PT0S");
                let duration = parse_iso_duration(duration_str);

                let mut artist_refs = Vec::new();
                if let Some(artist_rels) = item["relationships"]["artists"]["data"].as_array() {
                    for artist_ref in artist_rels {
                        if let Some(artist_id_str) = artist_ref["id"].as_str() {
                            if let Ok(artist_id) = artist_id_str.parse::<u64>() {
                                if let Some(artist) = artist_map.get(&artist_id) {
                                    artist_refs.push(ArtistRef {
                                        name: artist.name.clone(),
                                    });
                                }
                            }
                        }
                    }
                }

                let artist_name = if !artist_refs.is_empty() {
                    Some(artist_refs[0].clone())
                } else {
                    None
                };

                let album_id = item["relationships"]["albums"]["data"][0]["id"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let album_title = item["attributes"]["album"]["title"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let media_metadata = item["attributes"]["mediaTags"]
                    .as_array()
                    .map(|arr| {
                        let tags = arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                        MediaMetadata { tags }
                    });

                let track = Track {
                    id,
                    title,
                    duration,
                    artist: artist_name,
                    artists: artist_refs,
                    album: Album {
                        id: album_id,
                        title: album_title,
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
                };
                track_map.insert(id, track);
            }
        }
    }

    for track_ref in track_refs_src {
        if let Some(id_str) = track_ref["id"].as_str() {
            if let Ok(id) = id_str.parse::<u64>() {
                if let Some(track) = track_map.remove(&id) {
                    tracks.push(track);
                }
            }
        }
    }

    let total = tracks.len() as u32;
    Ok(SearchTrackPage {
        tracks,
        total,
        next_url,
    })
}

fn parse_search_artist_page(api_resp: &serde_json::Value) -> Result<SearchArtistPage> {
    let (artist_refs, next_url, included_opt) = if let Some(data_arr) = api_resp["data"].as_array()
    {
        if data_arr.is_empty() {
            (&Vec::new(), None, None)
        } else if data_arr[0].get("relationships").is_some() {
            // Initial search response: data is array with searchResults object
            let rels = &data_arr[0]["relationships"]["artists"];
            let refs = rels["data"].as_array().context("missing artists data")?;
            let next = rels["links"]["next"].as_str().map(String::from);
            let inc = api_resp["included"]
                .as_array()
                .context("missing included")?;
            (refs, next, Some(inc))
        } else {
            // Pagination response: data is array of artist objects
            let next = api_resp["links"]["next"].as_str().map(String::from);
            let inc = api_resp["included"].as_array();
            (data_arr, next, inc)
        }
    } else {
        anyhow::bail!("unexpected search artists response structure")
    };

    let mut artists = Vec::new();
    let mut artist_map = std::collections::HashMap::new();

    if let Some(included) = included_opt {
        for item in included {
            if item["type"] == "artists" {
                let id = item["id"]
                    .as_str()
                    .context("missing artist id")?
                    .parse::<u64>()?;
                let name = item["attributes"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let artist = Artist {
                    id,
                    name,
                    added_at: None,
                };
                artist_map.insert(id, artist);
            }
        }

        for artist_ref in artist_refs {
            if let Some(id_str) = artist_ref["id"].as_str() {
                if let Ok(id) = id_str.parse::<u64>() {
                    if let Some(artist) = artist_map.remove(&id) {
                        artists.push(artist);
                    }
                }
            }
        }
    }

    Ok(SearchArtistPage {
        artists: artists.clone(),
        total: artists.len() as u32,
        next_url,
    })
}

fn parse_search_playlist_page(api_resp: &serde_json::Value) -> Result<SearchPlaylistPage> {
    // Handle both initial search response (nested) and pagination response (flat)
    let (playlist_refs, next_url, included_opt) =
        if let Some(data_arr) = api_resp["data"].as_array() {
            if data_arr.is_empty() {
                (&Vec::new(), None, None)
            } else if data_arr[0].get("relationships").is_some() {
                // Initial search response: data is array with searchResults object
                let rels = &data_arr[0]["relationships"]["playlists"];
                let refs = rels["data"].as_array().context("missing playlists data")?;
                let next = rels["links"]["next"].as_str().map(String::from);
                let inc = api_resp["included"]
                    .as_array()
                    .context("missing included")?;
                (refs, next, Some(inc))
            } else {
                // Pagination response: data is array of playlist objects
                let next = api_resp["links"]["next"].as_str().map(String::from);
                let inc = api_resp["included"].as_array();
                (data_arr, next, inc)
            }
        } else {
            anyhow::bail!("unexpected search playlists response structure")
        };

    let mut playlists = Vec::new();
    let mut playlist_map = std::collections::HashMap::new();

    if let Some(included) = included_opt {
        for item in included {
            if item["type"] == "playlists" {
                let uuid = item["id"]
                    .as_str()
                    .context("missing playlist id")?
                    .to_string();
                let title = item["attributes"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let number_of_tracks = item["attributes"]["numberOfItems"]
                    .as_u64()
                    .map(|n| n as u32);
                let description = item["attributes"]["description"].as_str().map(String::from);

                tracing::debug!(
                    "found playlist: {} ({} items)",
                    title,
                    number_of_tracks.unwrap_or(0)
                );

                let playlist = Playlist {
                    uuid: uuid.clone(),
                    title,
                    number_of_tracks,
                    description,
                    cover: None,
                    added_at: None,
                };
                playlist_map.insert(uuid, playlist);
            }
        }

        for playlist_ref in playlist_refs {
            if let Some(uuid) = playlist_ref["id"].as_str() {
                if let Some(playlist) = playlist_map.remove(uuid) {
                    playlists.push(playlist);
                }
            }
        }
    }

    Ok(SearchPlaylistPage {
        playlists: playlists.clone(),
        total: playlists.len() as u32,
        next_url,
    })
}

pub struct ApiClient {
    http: reqwest::Client,
    token: RwLock<String>,
    config: Config,
}

impl ApiClient {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client");
        let token = config.access_token.clone().unwrap_or_default();
        Self {
            http,
            token: RwLock::new(token),
            config,
        }
    }

    async fn get_openapi<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{path}");
        let mut all_params = vec![("countryCode", self.config.country_code.clone())];
        all_params.extend_from_slice(params);

        let bytes = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("openapi GET failed")?
            .error_for_status()?
            .bytes()
            .await?;

        serde_json::from_slice::<T>(&bytes).map_err(|e| {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(400).collect();
            anyhow::anyhow!("{e} — body: {snippet}")
        })
    }

    async fn post_openapi_json(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{path}");
        self.http
            .post(&url)
            .bearer_auth(&token)
            .query(&[("countryCode", &self.config.country_code)])
            .json(body)
            .send()
            .await
            .context("openapi POST failed")?
            .error_for_status()?;
        Ok(())
    }

    async fn delete_openapi_json(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{path}");
        self.http
            .delete(&url)
            .bearer_auth(&token)
            .query(&[("countryCode", &self.config.country_code)])
            .json(body)
            .send()
            .await
            .context("openapi DELETE failed")?
            .error_for_status()?;
        Ok(())
    }

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

    // ── Artists ───────────────────────────────────────────────────────────────

    pub async fn get_favorite_artists_v2(&self) -> Result<(Vec<Artist>, u32)> {
        tracing::debug!("Fetching all favorite artists");

        let token = self.token.read().await.clone();
        let mut all_artists = Vec::new();
        let mut total = 0u32;
        let mut next_url: Option<String> = Some(format!(
            "{OPENAPI_BASE}/userCollectionArtists/me?locale=en-US&include=items"
        ));

        while let Some(mut url) = next_url {
            // Ensure include=items is present for pagination URLs
            if !url.contains("include=") {
                let separator = if url.contains("?") { "&" } else { "?" };
                url = format!("{}{}include=items", url, separator);
            }

            let full_url = if url.starts_with("http") {
                url.clone()
            } else {
                format!("{OPENAPI_BASE}{url}")
            };

            tracing::debug!("Fetching favorite artists page: {}", full_url);
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
                tracing::error!("API error on /userCollectionArtists/me: {}", body);
                anyhow::bail!("HTTP {}", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;
            let (artists, resp_total, next_cursor) = parse_v2_user_collection_artists(&api_resp)?;

            all_artists.extend(artists);
            // Keep the maximum total from all pages (first page usually has the correct total)
            total = total.max(resp_total);
            next_url = next_cursor;
        }

        tracing::debug!(
            "Fetched {} favorite artists (total: {})",
            all_artists.len(),
            total
        );
        Ok((all_artists, total))
    }

    pub async fn get_artist_picture(&self, artist_id: u64) -> Result<Option<String>> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}/artists/{artist_id}?locale=en-US&include=profileArt");

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
            tracing::error!("API error fetching artist {}: {}", artist_id, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let mut artwork_map = HashMap::new();
        if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
            for item in included {
                if item.get("type").and_then(|v| v.as_str()) == Some("artworks") {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        artwork_map.insert(id.to_string(), item.clone());
                    }
                }
            }
        }

        let picture = api_resp
            .get("data")
            .and_then(|d| d.get("relationships"))
            .and_then(|r| r.get("profileArt"))
            .and_then(|p| p.get("data"))
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("id"))
            .and_then(|id| id.as_str())
            .and_then(|artwork_id| {
                artwork_map
                    .get(artwork_id)
                    .and_then(|artwork| artwork.get("attributes"))
                    .and_then(|attrs| attrs.get("files"))
                    .and_then(|files| files.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|file| file.get("href"))
                    .and_then(|href| href.as_str())
                    .map(|s| s.to_string())
            });

        Ok(picture)
    }

    pub async fn get_artist_top_tracks(&self, artist_id: u64, _limit: u32) -> Result<Page<Track>> {
        tracing::debug!("Fetching all top tracks for artist {}", artist_id);

        let token = self.token.read().await.clone();
        let mut all_tracks = Vec::new();
        let mut next_url: Option<String> = Some(format!(
            "{OPENAPI_BASE}/artists/{artist_id}/relationships/tracks?locale=en-US&include=tracks.albums,tracks.artists&collapseBy=FINGERPRINT"
        ));

        while let Some(url) = next_url {
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
                    "API error on /artists/{}/relationships/tracks: {}",
                    artist_id,
                    body
                );
                anyhow::bail!("HTTP {}", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;

            let (mut tracks, _total, _next_cursor) =
                parse_playlist_relationship_items(&api_resp, 0)?;
            all_tracks.append(&mut tracks);

            next_url = api_resp
                .get("links")
                .and_then(|v| v.get("next"))
                .and_then(|v| v.as_str())
                .map(|s| format!("{OPENAPI_BASE}{}", s));
        }

        tracing::debug!("Fetched {} total top tracks", all_tracks.len());

        let total = all_tracks.len() as u32;
        Ok(Page {
            items: all_tracks,
            total,
        })
    }

    pub async fn get_artist_albums(&self, artist_id: u64, _limit: u32) -> Result<Page<Album>> {
        tracing::debug!("Fetching all albums for artist {}", artist_id);

        let token = self.token.read().await.clone();
        let mut all_albums = Vec::new();
        let mut next_url: Option<String> = Some(format!(
            "{OPENAPI_BASE}/artists/{artist_id}/relationships/albums?locale=en-US&include=albums"
        ));

        while let Some(url) = next_url {
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
                    "API error on /artists/{}/relationships/albums: {}",
                    artist_id,
                    body
                );
                anyhow::bail!("HTTP {}", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;

            let mut albums = parse_artist_albums(&api_resp)?;
            all_albums.append(&mut albums);

            next_url = api_resp
                .get("links")
                .and_then(|v| v.get("next"))
                .and_then(|v| v.as_str())
                .map(|s| format!("{OPENAPI_BASE}{}", s));
        }

        let filtered: Vec<Album> = all_albums
            .into_iter()
            .filter(|a| a.album_type.as_deref() == Some("ALBUM"))
            .collect();

        tracing::debug!("Fetched {} total albums", filtered.len());

        let total = filtered.len() as u32;
        Ok(Page {
            items: filtered,
            total,
        })
    }

    pub async fn get_artist_eps(&self, artist_id: u64, _limit: u32) -> Result<Page<Album>> {
        let token = self.token.read().await.clone();
        let mut all_albums = Vec::new();
        let mut next_url: Option<String> = Some(format!(
            "{OPENAPI_BASE}/artists/{artist_id}/relationships/albums?locale=en-US&include=albums"
        ));

        while let Some(url) = next_url {
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
                    "API error on /artists/{}/relationships/albums: {}",
                    artist_id,
                    body
                );
                anyhow::bail!("HTTP {}", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;

            let mut albums = parse_artist_albums(&api_resp)?;
            all_albums.append(&mut albums);

            next_url = api_resp
                .get("links")
                .and_then(|v| v.get("next"))
                .and_then(|v| v.as_str())
                .map(|s| format!("{OPENAPI_BASE}{}", s));
        }

        let filtered: Vec<Album> = all_albums
            .into_iter()
            .filter(|a| a.album_type.as_deref() == Some("EP"))
            .collect();

        let total = filtered.len() as u32;
        Ok(Page {
            items: filtered,
            total,
        })
    }

    pub async fn get_artist_singles(&self, artist_id: u64, _limit: u32) -> Result<Page<Album>> {
        let token = self.token.read().await.clone();
        let mut all_albums = Vec::new();
        let mut next_url: Option<String> = Some(format!(
            "{OPENAPI_BASE}/artists/{artist_id}/relationships/albums?locale=en-US&include=albums"
        ));

        while let Some(url) = next_url {
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
                    "API error on /artists/{}/relationships/albums: {}",
                    artist_id,
                    body
                );
                anyhow::bail!("HTTP {}", status);
            }

            let body = resp.text().await?;
            let api_resp: serde_json::Value = serde_json::from_str(&body)?;

            let mut albums = parse_artist_albums(&api_resp)?;
            all_albums.append(&mut albums);

            next_url = api_resp
                .get("links")
                .and_then(|v| v.get("next"))
                .and_then(|v| v.as_str())
                .map(|s| format!("{OPENAPI_BASE}{}", s));
        }

        let filtered: Vec<Album> = all_albums
            .into_iter()
            .filter(|a| a.album_type.as_deref() == Some("SINGLE"))
            .collect();

        let total = filtered.len() as u32;
        Ok(Page {
            items: filtered,
            total,
        })
    }

    pub async fn get_artist_bio(&self, artist_id: u64) -> Result<String> {
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/artists/{artist_id}/relationships/biography?countryCode={}&include=biography",
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
            tracing::error!(
                "API error {} fetching artist bio for {artist_id}: {}",
                status,
                body
            );
            anyhow::bail!("HTTP {} fetching artist bio", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let bio_text = api_resp
            .get("included")
            .and_then(|arr| arr.as_array())
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("attributes"))
            .and_then(|attrs| attrs.get("text"))
            .and_then(|text| text.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        tracing::debug!(
            "Fetched bio for artist {}: {} chars",
            artist_id,
            bio_text.len()
        );
        Ok(bio_text)
    }

    // ── Playlists ─────────────────────────────────────────────────────────────

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

    // ── Favorites ─────────────────────────────────────────────────────────────

    pub async fn get_favorite_albums(
        &self,
        next_url: Option<String>,
    ) -> Result<(Vec<Album>, u32, Option<String>)> {
        tracing::debug!("Fetching favorite albums");
        let token = self.token.read().await.clone();
        let url = next_url.unwrap_or_else(|| {
            format!("{OPENAPI_BASE}/userCollectionAlbums/me/relationships/items?locale=en-US&sort=-addedAt&include=items.artists,items.coverArt")
        });

        tracing::debug!("Favorite albums URL: {}", url);
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
        let mut total = 0u32;
        let mut next_cursor = None;

        if let Some(data_array) = api_resp.get("data").and_then(|v| v.as_array()) {
            total = data_array.len() as u32;
        }

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
                                        audio_quality: None,
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

        Ok((albums, total, next_cursor))
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

    // ── Search ────────────────────────────────────────────────────────────────

    pub async fn search_tracks(&self, query: &str) -> Result<SearchTrackPage> {
        tracing::debug!("=== executing v2 search for tracks: '{}' ===", query);
        let url = format!("{OPENAPI_BASE}/searchResults");

        let mut all_params: Vec<(&str, String)> = vec![
            ("filter[query]", query.to_string()),
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }
        all_params.push(("include", "tracks.artists".to_string()));

        let token = self.token.read().await.clone();
        let body = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("HTTP request failed")?
            .json::<serde_json::Value>()
            .await
            .context("failed to parse search tracks response")?;

        parse_search_track_page(&body)
    }

    pub async fn search_artists(&self, query: &str) -> Result<SearchArtistPage> {
        tracing::debug!("=== executing v2 search for artists: '{}' ===", query);
        let url = format!("{OPENAPI_BASE}/searchResults");

        let mut all_params: Vec<(&str, String)> = vec![
            ("filter[query]", query.to_string()),
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }
        all_params.push(("include", "artists.profileArt".to_string()));

        let token = self.token.read().await.clone();
        let body = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("HTTP request failed")?
            .json::<serde_json::Value>()
            .await
            .context("failed to parse search artists response")?;

        parse_search_artist_page(&body)
    }

    pub async fn search_playlists(&self, query: &str) -> Result<SearchPlaylistPage> {
        tracing::debug!("=== executing v2 search for playlists: '{}' ===", query);
        let url = format!("{OPENAPI_BASE}/searchResults");

        let mut all_params: Vec<(&str, String)> = vec![
            ("filter[query]", query.to_string()),
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }
        all_params.push(("include", "playlists".to_string()));

        let token = self.token.read().await.clone();
        let body = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .query(&all_params)
            .send()
            .await
            .context("HTTP request failed")?
            .json::<serde_json::Value>()
            .await
            .context("failed to parse search playlists response")?;

        parse_search_playlist_page(&body)
    }

    pub async fn search_tracks_next(&self, next_url: &str) -> Result<SearchTrackPage> {
        let sep = if next_url.contains('?') { "&" } else { "?" };
        let url = format!(
            "{OPENAPI_BASE}{next_url}{sep}include=tracks.artists",
            sep = sep
        );
        tracing::debug!("pagination request (tracks): {}", url);
        let body = self
            .http
            .get(&url)
            .bearer_auth(&self.token.read().await.clone())
            .send()
            .await
            .context("HTTP request failed")?
            .json()
            .await
            .context("failed to parse search tracks response")?;
        parse_search_track_page(&body)
    }

    pub async fn search_artists_next(&self, next_url: &str) -> Result<SearchArtistPage> {
        let sep = if next_url.contains('?') { "&" } else { "?" };
        let url = format!(
            "{OPENAPI_BASE}{next_url}{sep}include=artists.profileArt",
            sep = sep
        );
        let body = self
            .http
            .get(&url)
            .bearer_auth(&self.token.read().await.clone())
            .send()
            .await
            .context("HTTP request failed")?
            .json()
            .await
            .context("failed to parse search artists response")?;
        parse_search_artist_page(&body)
    }

    pub async fn search_playlists_next(&self, next_url: &str) -> Result<SearchPlaylistPage> {
        let sep = if next_url.contains('?') { "&" } else { "?" };
        let url = format!("{OPENAPI_BASE}{next_url}{sep}include=playlists", sep = sep);
        let body = self
            .http
            .get(&url)
            .bearer_auth(&self.token.read().await.clone())
            .send()
            .await
            .context("HTTP request failed")?
            .json()
            .await
            .context("failed to parse search playlists response")?;
        parse_search_playlist_page(&body)
    }

    // ── Albums ────────────────────────────────────────────────────────────────

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

        tracing::debug!("Album {} cover extracted: {:?}", album_id, cover_url);
        let album = Album {
            id: album_id,
            title,
            number_of_tracks,
            release_date,
            cover: cover_url.clone(),
            artist: None,
            audio_quality: None,
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
            let full_url = if url.starts_with("http") {
                url.clone()
            } else {
                format!("{OPENAPI_BASE}{url}")
            };

            tracing::debug!("Fetching album tracks page: {}", full_url);
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
            tracing::debug!("Album tracks page loaded, total so far: {}", tracks.len());

            next_url = next_url_from_response;
        }

        let total = tracks.len() as u32;
        tracing::debug!("Album tracks fully loaded: {} total", total);
        Ok(Page {
            items: tracks,
            total,
        })
    }

    // ── Radio ─────────────────────────────────────────────────────────────────

    pub async fn get_track_radio(&self, track_id: u64) -> Result<Page<Track>> {
        tracing::debug!("Fetching radio for track {}", track_id);
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/tracks/{track_id}/relationships/radio?locale=en-US&include=radio.items.albums,radio.items.artists"
        );

        tracing::debug!("Radio URL: {}", url);
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

        tracing::debug!("Radio URL: {}", url);
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

    // ── Lyrics ───────────────────────────────────────────────────────────────

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

    // ── Playback ──────────────────────────────────────────────────────────────

    /// Fetch raw bytes from a public URL (e.g. Tidal's cover art CDN).
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).send().await?;
        let status = resp.status();

        let bytes = resp
            .error_for_status()
            .map_err(|e| {
                tracing::warn!("Failed to fetch bytes ({}): {}", status.as_u16(), e);
                e
            })?
            .bytes()
            .await?;

        tracing::debug!("Fetched {} bytes", bytes.len());
        Ok(bytes.to_vec())
    }

    pub async fn add_favorite_track(&self, track_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": track_id.to_string(), "type": "tracks"}]});
        self.post_openapi_json("/userCollectionTracks/me/relationships/items", &body)
            .await
    }

    pub async fn follow_artist(&self, artist_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": artist_id.to_string(), "type": "artists"}]});
        self.post_openapi_json("/userCollectionArtists/me/relationships/items", &body)
            .await
    }

    pub async fn remove_favorite_track(&self, track_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": track_id.to_string(), "type": "tracks"}]});
        self.delete_openapi_json("/userCollectionTracks/me/relationships/items", &body)
            .await
    }

    pub async fn unfollow_artist(&self, artist_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": artist_id.to_string(), "type": "artists"}]});
        self.delete_openapi_json("/userCollectionArtists/me/relationships/items", &body)
            .await
    }

    /// Resolve a playable URL for `track_id`.
    ///
    /// # This stays on v1 permanently — do not migrate it
    ///
    /// Every other endpoint in this client has moved to openapi.tidal.com/v2.
    /// This one cannot, and the reason is DRM rather than effort. Verified
    /// against track 431291038 on a live subscriber token:
    ///
    /// v1 `playbackinfopostpaywall` (LOSSLESS) returns a BTS manifest reading
    /// `{"codecs":"flac","encryptionType":"NONE","urls":[...]}` — a plain HTTPS
    /// URL to an unencrypted FLAC that mpv plays directly.
    ///
    /// The v2 equivalent, `GET /trackManifests/{id}`, only accepts
    /// `manifestType` of `HLS` or `MPEG_DASH` (no BTS), and every combination
    /// of `formats` / `usage` / `adaptive` returns CENC-encrypted content:
    ///
    /// | manifestType | formats           | Result                              |
    /// |--------------|-------------------|-------------------------------------|
    /// | HLS          | FLAC, FLAC_HIRES  | FairPlay, `initData: ["skd://…"]`   |
    /// | MPEG_DASH    | FLAC, FLAC_HIRES  | Widevine                            |
    /// | MPEG_DASH    | AACLC             | Widevine                            |
    /// | MPEG_DASH    | `usage=DOWNLOAD`  | Widevine                            |
    ///
    /// The `"drmData": {"initData": null}` on the DASH responses is a red
    /// herring — the init data lives in the `.mpd` body itself:
    /// `<ContentProtection value="cbcs" cenc:default_KID="…">` plus Widevine
    /// (`edef8ba9-…`) and PlayReady (`9a04f079-…`) PSSH boxes.
    ///
    /// Decrypting that needs a CDM, which mpv/ffmpeg do not have. TIDAL's own
    /// Player SDK solves it by delegating to a browser's EME stack
    /// (`shaka-player`, `fairplay-drm.ts`), so adopting the SDK would mean
    /// embedding a browser engine and dropping mpv — and third-party apps on
    /// that path are limited to 30-second previews unless the client ID is
    /// entitled. Ours is not: `/trackFiles/{id}` returns
    /// `403 CLIENT_NOT_ENTITLED`, and `/tracks/{id}/relationships/download`
    /// yields only a resource identifier, no file.
    ///
    /// v1 also already carries the loudness data v2 advertises
    /// (`albumReplayGain`, `trackReplayGain`, and both peak amplitudes), so
    /// there is nothing to gain by switching even setting DRM aside.
    ///
    /// Consequence: `BASE`, `dash_to_hls`, `build_flac_m3u8` and the localhost
    /// manifest server in `src/manifest.rs` are all load-bearing and must stay.
    pub async fn get_stream_url(&self, track_id: u64) -> Result<String> {
        // Quality fallback chain for streaming.
        //
        // | Quality          | Manifest MIME type         | Container   | Actual codec  |
        // |------------------|----------------------------|-------------|---------------|
        // | LOSSLESS         | application/vnd.tidal.bts  | audio/flac  | FLAC (raw)    |
        // | HI_RES_LOSSLESS  | application/dash+xml       | audio/mp4   | FLAC or AAC   |
        // | HIGH             | application/vnd.tidal.bts  | audio/mp4   | AAC           |
        //
        // LOSSLESS → BTS manifest with `codecs: "flac"` → guaranteed raw FLAC.
        // HI_RES_LOSSLESS → DASH manifest where codecs MAY be "flac" or "mp4a.40.2".
        // Strategy: try LOSSLESS first (guaranteed FLAC), then HI_RES_LOSSLESS
        // (only if its DASH codec is actually FLAC), then HIGH as last resort.
        const QUALITIES: &[&str] = &["LOSSLESS", "HI_RES_LOSSLESS", "HIGH"];
        let path = format!("/tracks/{track_id}/playbackinfopostpaywall");
        let debug = std::env::var("RIPTIDE_QUALITY_DEBUG").is_ok();
        let token = self.token.read().await.clone();
        let base_url = format!("{BASE}{path}");

        for &quality in QUALITIES {
            let mut all_params: Vec<(&str, String)> = vec![
                ("countryCode", self.config.country_code.clone()),
                ("audioquality", quality.to_string()),
                ("playbackmode", "STREAM".to_string()),
                ("assetpresentation", "FULL".to_string()),
            ];
            if let Some(sid) = &self.config.session_id {
                all_params.push(("sessionId", sid.clone()));
            }

            let resp = self
                .http
                .get(&base_url)
                .bearer_auth(&token.clone())
                .query(&all_params)
                .send()
                .await
                .context("HTTP request failed")?;

            let status = resp.status();

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                let error_msg = if body.is_empty() {
                    status.to_string()
                } else {
                    // Truncate to first 200 chars for readability
                    let snippet: String = body.chars().take(200).collect();
                    format!("{}: {}", status, snippet)
                };

                if debug {
                    eprintln!("[quality] track {track_id}: {quality} request failed — {error_msg}");
                }

                if matches!(
                    status,
                    reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
                ) {
                    tracing::debug!("Track {track_id} ({quality}): {error_msg}");
                    continue;
                }
                return Err(anyhow::anyhow!("Track {track_id} ({quality}): {error_msg}"));
            }

            let body = resp.text().await?;
            let info: PlaybackInfo =
                serde_json::from_str(&body).context("parse playback info response")?;

            let mime = info.manifest_mime_type.clone();
            if debug {
                let aq = info.audio_quality.as_deref().unwrap_or("?");
                eprintln!(
                    "[quality] track {track_id}: requested {quality}, \
                     server returned manifestMimeType={mime}, \
                     audioQuality={aq} (200 OK)",
                );
            }

            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&info.manifest)
                .context("base64 decode of manifest")?;

            match mime.as_str() {
                "application/vnd.tidal.bts" => {
                    let manifest: BtsManifest =
                        serde_json::from_slice(&bytes).context("parse BTS manifest")?;

                    if manifest.urls.is_empty() {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: BTS manifest has empty urls — skip"
                            );
                        }
                        continue;
                    }

                    let codec = manifest.codecs.as_deref().unwrap_or("(missing)");
                    if debug {
                        eprintln!(
                            "[quality] track {track_id}: BTS codecs={codec}, \
                             urls={} segment(s)",
                            manifest.urls.len(),
                        );
                    }

                    // BTS with FLAC codec → real lossless.
                    if manifest.is_flac() {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: ✓ FLAC stream accepted ({quality})"
                            );
                        }
                        if manifest.urls.len() == 1 {
                            return Ok(manifest.urls.into_iter().next().unwrap());
                        }
                        let m3u8 = build_flac_m3u8(track_id, &manifest.urls);
                        return Ok(m3u8);
                    }

                    // BTS with non-FLAC codec.
                    // For LOSSLESS requests: the API downgraded us → skip.
                    // For HIGH requests: this is expected AAC → accept.
                    if quality == "HIGH" {
                        if debug {
                            eprintln!("[quality] track {track_id}: accepting AAC stream (HIGH)");
                        }
                        if let Some(url) = manifest.urls.into_iter().next() {
                            return Ok(url);
                        }
                    } else {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: BTS codec is '{codec}' \
                                 (not flac) for {quality} request — falling through",
                            );
                        }
                        continue;
                    }
                }
                "application/dash+xml" => {
                    let xml = String::from_utf8_lossy(&bytes);

                    let sets = find_adaptation_sets(&xml);
                    let has_flac = sets.iter().any(|s| s.codecs == "flac");

                    if debug {
                        let codecs: Vec<&str> = sets.iter().map(|s| s.codecs.as_str()).collect();
                        eprintln!(
                            "[quality] track {track_id}: DASH with {} AdaptationSet(s), \
                             codecs={:?}, has_flac={has_flac}",
                            sets.len(),
                            codecs,
                        );
                    }

                    if (quality == "LOSSLESS" || quality == "HI_RES_LOSSLESS") && !has_flac {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: DASH has no FLAC codec \
                                 — falling through to next tier",
                            );
                        }
                        continue;
                    }

                    if debug {
                        eprintln!("[quality] track {track_id}: ✓ DASH/FLAC accepted ({quality})");
                    }
                    let hls =
                        dash_to_hls(track_id, &xml).context("convert DASH manifest to HLS")?;
                    return Ok(hls);
                }
                _ => {
                    if debug {
                        eprintln!(
                            "[quality] track {track_id}: unknown manifest MIME type '{mime}' — skip",
                        );
                    }
                    continue;
                }
            }
        }

        Err(anyhow::anyhow!(
            "no stream URL available for track {track_id}"
        ))
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

    async fn get_mixes(&self, endpoint: &str) -> Result<Vec<Playlist>> {
        let token = self.token.read().await.clone();
        let url = format!("{OPENAPI_BASE}{endpoint}?locale=en-US&include=items.items");

        tracing::debug!("API request: GET {endpoint}");

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}", resp.status());
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let mut playlists = Vec::new();

        // Build a map of playlist IDs to their details from the included array
        let mut playlist_map = HashMap::new();
        if let Some(included) = api_resp.get("included").and_then(|v| v.as_array()) {
            for item in included {
                if let Some("playlists") = item.get("type").and_then(|v| v.as_str()) {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                        playlist_map.insert(id.to_string(), item.clone());
                    }
                }
            }
        }

        // Use the order from the data array to create playlists in the correct order
        if let Some(data) = api_resp.get("data").and_then(|v| v.as_array()) {
            if !data.is_empty() {
                for item_ref in data {
                    if let Some(id) = item_ref.get("id").and_then(|v| v.as_str()) {
                        if let Some(playlist_obj) = playlist_map.get(id) {
                            if let Some(attrs) =
                                playlist_obj.get("attributes").and_then(|v| v.as_object())
                            {
                                let title = attrs
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let number_of_tracks = attrs
                                    .get("numberOfItems")
                                    .and_then(|v| v.as_u64())
                                    .map(|n| n as u32);
                                let cover = attrs
                                    .get("image")
                                    .and_then(|v| v.as_str())
                                    .or_else(|| attrs.get("squareImage").and_then(|v| v.as_str()))
                                    .map(String::from);

                                tracing::debug!(
                                    "Mix playlist: title={}, tracks={}, has_cover={}",
                                    title,
                                    number_of_tracks.unwrap_or(0),
                                    cover.is_some()
                                );

                                playlists.push(Playlist {
                                    uuid: id.to_string(),
                                    title,
                                    number_of_tracks,
                                    description: None,
                                    cover,
                                    added_at: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Fallback: if data array was empty or missing, use all playlists from included
        if playlists.is_empty() {
            for (_, playlist_obj) in playlist_map.iter() {
                if let Some(attrs) = playlist_obj.get("attributes").and_then(|v| v.as_object()) {
                    if let Some(id) = playlist_obj.get("id").and_then(|v| v.as_str()) {
                        let title = attrs
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let number_of_tracks = attrs
                            .get("numberOfItems")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32);
                        let cover = attrs
                            .get("image")
                            .and_then(|v| v.as_str())
                            .or_else(|| attrs.get("squareImage").and_then(|v| v.as_str()))
                            .map(String::from);

                        playlists.push(Playlist {
                            uuid: id.to_string(),
                            title,
                            number_of_tracks,
                            description: None,
                            cover,
                            added_at: None,
                        });
                    }
                }
            }
            playlists.sort_by(|a, b| {
                let get_num = |s: &str| {
                    s.split_whitespace()
                        .last()
                        .and_then(|w| w.parse::<u32>().ok())
                };
                let a_num = get_num(&a.title);
                let b_num = get_num(&b.title);
                match (a_num, b_num) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    _ => a.title.cmp(&b.title),
                }
            });
        }

        Ok(playlists)
    }

    pub async fn get_daily_mixes(&self) -> Result<Vec<Playlist>> {
        self.get_mixes("/userDailyMixes/me").await
    }

    pub async fn get_discovery_mixes(&self) -> Result<Vec<Playlist>> {
        self.get_mixes("/userDiscoveryMixes/me").await
    }

    pub async fn get_mix_tracks(
        &self,
        mix_id: &str,
        offset: u32,
    ) -> Result<(Vec<Track>, u32, Option<String>, Option<String>)> {
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/playlists/{mix_id}?countryCode=US&include=items.artists,coverArt&offset={offset}&limit=100"
        );

        tracing::debug!(
            "API request: GET /playlists/{} with include=items.artists,coverArt",
            mix_id
        );

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.api+json")
            .send()
            .await?;

        let status = resp.status();
        tracing::debug!("API response: {} /playlists/{}", status, mix_id);

        if !status.is_success() {
            let body = resp.text().await?;
            tracing::error!("API error {} on /playlists/{}: {}", status, mix_id, body);
            anyhow::bail!("HTTP {}", status);
        }

        let body = resp.text().await?;
        let api_resp: serde_json::Value = serde_json::from_str(&body)?;

        let (tracks, total, _, description, cover) = parse_v2_playlist_tracks(&api_resp)?;
        Ok((tracks, total, cover, description))
    }

    pub async fn get_new_release_mixes(&self) -> Result<Vec<Playlist>> {
        self.get_mixes("/userNewReleaseMixes/me").await
    }
}

// ── Multi-segment FLAC playlist ────────────────────────────────────────────────

/// Build a simple M3U8 playlist for multi-segment raw FLAC URLs so mpv can
/// play them gaplessly in sequence.
fn build_flac_m3u8(track_id: u64, urls: &[String]) -> String {
    let mut m3u8 = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    // Each segment is a standalone FLAC file; mpv handles concatenation natively.
    for url in urls {
        // We don't know exact durations upfront, but mpv will determine them
        // from the FLAC stream headers. Use a generous placeholder.
        m3u8.push_str("#EXTINF:10.0,\n");
        m3u8.push_str(url);
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    let playlist_path = format!("/tmp/riptide_hls_{track_id}.m3u8");
    let _ = std::fs::write(&playlist_path, &m3u8);
    format!("http://127.0.0.1:{}/{track_id}.m3u8", crate::manifest::PORT)
}

// ── DASH → HLS conversion ─────────────────────────────────────────────────────

/// Represents a single `<AdaptationSet>` found in the DASH manifest.
struct DashAdaptationSet {
    /// The `codecs` attribute from the AdaptationSet or Representation element.
    codecs: String,
    /// Position of this AdaptationSet in the original XML (byte offset of opening tag).
    _offset: usize,
}

/// Find all AdaptationSet elements and their codec info.
/// Returns them so we can prefer FLAC over AAC.
fn find_adaptation_sets(xml: &str) -> Vec<DashAdaptationSet> {
    let mut sets = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find("<AdaptationSet") {
        let set_start = pos;
        let fragment = &rest[pos..];
        // Find codecs in the AdaptationSet or its Representation child.
        let codecs = dash_attr(fragment, "codecs").unwrap_or_default();
        let offset = xml.len() - rest.len() + set_start;
        sets.push(DashAdaptationSet {
            codecs,
            _offset: offset,
        });
        // Advance past this AdaptationSet to find the next one.
        if let Some(end) = fragment.find("</AdaptationSet>") {
            rest = &fragment[end + "</AdaptationSet>".len()..];
        } else {
            break;
        }
    }
    sets
}

/// Convert a Tidal DASH manifest to an HLS playlist served via local HTTP.
///
/// When the manifest contains multiple AdaptationSets (e.g. AAC and FLAC),
/// we prefer the FLAC one so mpv plays real lossless audio.
fn dash_to_hls(track_id: u64, xml: &str) -> anyhow::Result<String> {
    // If there are multiple AdaptationSets, try to find a FLAC one.
    let adaptation_sets = find_adaptation_sets(xml);

    // Determine which region of the XML to use for attribute extraction.
    // If we have a FLAC adaptation set, extract attributes from within it.
    let search_region = if adaptation_sets.len() > 1 {
        if let Some(flac_set) = adaptation_sets.iter().find(|s| s.codecs == "flac") {
            // Extract from just this AdaptationSet's region of the XML.
            let start = flac_set._offset;
            let rest = &xml[start..];
            if let Some(end) = rest.find("</AdaptationSet>") {
                &rest[..end + "</AdaptationSet>".len()]
            } else {
                xml
            }
        } else {
            xml
        }
    } else {
        xml
    };

    let codecs = dash_attr(search_region, "codecs").unwrap_or_default();

    let init_url = dash_attr(search_region, "initialization")
        .context("no initialization URL in DASH manifest")?;
    let media_tmpl =
        dash_attr(search_region, "media").context("no media template in DASH manifest")?;
    let timescale: f64 = dash_attr(search_region, "timescale")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let start_num: u64 = dash_attr(search_region, "startNumber")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let durations = dash_segment_durations(search_region, timescale);
    anyhow::ensure!(!durations.is_empty(), "no segments in DASH manifest");

    let target = durations.iter().cloned().fold(0f64, f64::max).ceil() as u64;
    let mut m3u8 = format!("#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-TARGETDURATION:{target}\n");
    // Include codec info so mpv knows what to expect.
    if !codecs.is_empty() {
        m3u8.push_str(&format!("#EXT-X-CODECS:{codecs}\n"));
    }
    m3u8.push_str(&format!("#EXT-X-MAP:URI=\"{init_url}\"\n"));

    for (i, dur) in durations.iter().enumerate() {
        m3u8.push_str(&format!("#EXTINF:{dur:.5},\n"));
        m3u8.push_str(&media_tmpl.replace("$Number$", &(start_num + i as u64).to_string()));
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    std::fs::write(format!("/tmp/riptide_hls_{track_id}.m3u8"), &m3u8)
        .context("write HLS playlist")?;
    Ok(format!(
        "http://127.0.0.1:{}/{track_id}.m3u8",
        crate::manifest::PORT
    ))
}

/// Extract an XML attribute value by name, checking that it isn't a substring
/// of a longer attribute name (e.g. `d` must not match `id`).
fn dash_attr(xml: &str, name: &str) -> Option<String> {
    let needle = format!("{}=\"", name);
    let mut haystack = xml;
    while let Some(pos) = haystack.find(&needle) {
        let before = pos
            .checked_sub(1)
            .and_then(|i| haystack.as_bytes().get(i).copied())
            .map(|b| b as char)
            .unwrap_or(' ');
        if !before.is_alphanumeric() && before != '_' && before != '-' {
            let start = pos + needle.len();
            let end = haystack[start..].find('"')? + start;
            return Some(haystack[start..end].to_owned());
        }
        haystack = &haystack[pos + needle.len()..];
    }
    None
}

/// Parse `<S d="..." r="..."/>` elements inside `<SegmentTimeline>`.
fn dash_segment_durations(xml: &str, timescale: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let tl_start = match xml.find("<SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let tl = &xml[tl_start..];
    let tl_end = match tl.find("</SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let mut rest = &tl[..tl_end];
    while let Some(pos) = rest.find("<S ") {
        let inner_start = pos + 3;
        let inner_end = rest[inner_start..]
            .find("/>")
            .map(|p| p + inner_start)
            .unwrap_or(rest.len());
        let elem = &rest[inner_start..inner_end];
        let d: f64 = dash_attr(elem, "d")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let r: usize = dash_attr(elem, "r")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let dur = d / timescale;
        for _ in 0..=r {
            out.push(dur);
        }
        rest = &rest[inner_end..];
    }
    out
}
