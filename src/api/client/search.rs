// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Search across tracks, artists and playlists.
//!
//! The v2 search endpoint returns two different shapes: the initial request
//! nests results under a `searchResults` object with relationships, while
//! paginated requests return a flat `data` array. The parsers here handle both.

use anyhow::{Context, Result};

use super::parse::*;
use super::{ApiClient, OPENAPI_BASE};
use crate::api::models::*;

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

                let media_metadata = item["attributes"]["mediaTags"].as_array().map(|arr| {
                    let tags = arr
                        .iter()
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
                        media_metadata: None,
                        added_at: None,
                        album_type: None,
                    },
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

impl ApiClient {
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
}
