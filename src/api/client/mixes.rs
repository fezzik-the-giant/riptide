// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Daily, discovery and new-release mixes shown on the Home tab.

use anyhow::Result;
use std::collections::HashMap;

use super::playlists::parse_v2_playlist_tracks;
use super::{ApiClient, OPENAPI_BASE};
use crate::api::models::*;

impl ApiClient {
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
    ) -> Result<(Vec<Track>, u32, Option<String>, Option<String>)> {
        let token = self.token.read().await.clone();
        let url = format!(
            "{OPENAPI_BASE}/playlists/{mix_id}?countryCode=US&include=items.artists,coverArt"
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
