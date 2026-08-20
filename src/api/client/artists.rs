// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Artist catalogue and the followed-artists collection.

use anyhow::Result;
use std::collections::HashMap;

use super::playlists::parse_playlist_relationship_items;
use super::{ApiClient, OPENAPI_BASE, absolute_url};
use crate::api::models::*;

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

fn parse_v2_user_collection_artists(
    api_resp: &serde_json::Value,
) -> Result<(Vec<Artist>, u32, Option<String>)> {
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

    Ok((artists, total, next_url))
}

impl ApiClient {
    pub async fn get_favorite_artists_v2(&self) -> Result<(Vec<Artist>, u32)> {
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

        tracing::info!("loaded {} followed artists", all_artists.len());
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

    pub async fn follow_artist(&self, artist_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": artist_id.to_string(), "type": "artists"}]});
        self.post_openapi_json("/userCollectionArtists/me/relationships/items", &body)
            .await
    }

    pub async fn unfollow_artist(&self, artist_id: u64) -> Result<()> {
        let body = serde_json::json!({"data": [{"id": artist_id.to_string(), "type": "artists"}]});
        self.delete_openapi_json("/userCollectionArtists/me/relationships/items", &body)
            .await
    }
}
