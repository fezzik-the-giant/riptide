// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Daily, discovery and new-release mixes shown on the Home tab.

use anyhow::Result;
use std::collections::HashMap;

use super::parse::{build_included_map, resolve_cover_art};
use super::playlists::parse_v2_playlist_tracks;
use super::{ApiClient, OPENAPI_BASE};
use crate::api::models::*;

/// Build a mix from a `playlists` object out of an `included` array.
fn mix_from_playlist(
    playlist_obj: &serde_json::Value,
    artwork_map: &HashMap<String, serde_json::Value>,
) -> Option<Playlist> {
    let id = playlist_obj.get("id").and_then(|v| v.as_str())?;
    let attrs = playlist_obj.get("attributes").and_then(|v| v.as_object())?;
    Some(Playlist {
        uuid: id.to_string(),
        title: attrs
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        number_of_tracks: attrs
            .get("numberOfItems")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        description: None,
        cover: resolve_cover_art(playlist_obj, artwork_map),
        added_at: None,
    })
}

impl ApiClient {
    async fn get_mixes(&self, endpoint: &str) -> Result<Vec<Playlist>> {
        let token = self.token.read().await.clone();
        // `items.coverArt` is the only way to get a mix's cover: the playlist
        // objects themselves carry no image attribute, verified live against all
        // three endpoints. Without it every mix reaches the Home tab coverless.
        let url = format!("{OPENAPI_BASE}{endpoint}?locale=en-US&include=items,items.coverArt");

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

        let playlist_map = build_included_map(&api_resp, "playlists");
        let artwork_map = build_included_map(&api_resp, "artworks");

        // `data` carries the order; `included` carries the objects.
        let mut playlists: Vec<Playlist> = api_resp
            .get("data")
            .and_then(|v| v.as_array())
            .map(|data| {
                data.iter()
                    .filter_map(|item_ref| item_ref.get("id").and_then(|v| v.as_str()))
                    .filter_map(|id| playlist_map.get(id))
                    .filter_map(|obj| mix_from_playlist(obj, &artwork_map))
                    .collect()
            })
            .unwrap_or_default();

        // Fallback: if data array was empty or missing, use all playlists from included
        if playlists.is_empty() {
            playlists = playlist_map
                .values()
                .filter_map(|obj| mix_from_playlist(obj, &artwork_map))
                .collect();
            playlists.sort_by(|a, b| {
                let get_num = |s: &str| {
                    s.split_whitespace()
                        .last()
                        .and_then(|w| w.parse::<u32>().ok())
                };
                match (get_num(&a.title), get_num(&b.title)) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    _ => a.title.cmp(&b.title),
                }
            });
        }

        tracing::debug!(
            "Parsed {} mixes from {endpoint}, {} with cover art",
            playlists.len(),
            playlists.iter().filter(|p| p.cover.is_some()).count()
        );

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape taken from a live `/userNewReleaseMixes/me?include=items,items.coverArt`
    /// response, with the artwork hrefs stubbed. Note the playlist object carries
    /// no image attribute of its own — the cover is only reachable through the
    /// `coverArt` relationship.
    const RESPONSE: &str = r#"{
      "data": [{"id": "0113f754", "type": "playlists"}],
      "included": [
        {
          "id": "0113f754",
          "type": "playlists",
          "attributes": {"name": "My New Arrivals"},
          "relationships": {
            "coverArt": {"data": [{"id": "86EFpfvW", "type": "artworks"}]}
          }
        },
        {
          "id": "86EFpfvW",
          "type": "artworks",
          "attributes": {
            "mediaType": "IMAGE",
            "files": [
              {"href": "https://images.tidal.com/COVER-160", "meta": {"width": 160, "height": 160}},
              {"href": "https://images.tidal.com/COVER-320", "meta": {"width": 320, "height": 320}},
              {"href": "https://images.tidal.com/COVER-1080", "meta": {"width": 1080, "height": 1080}}
            ]
          }
        }
      ]
    }"#;

    /// The fixture deliberately omits the 480 px variant the cap names, so this
    /// also covers the pick falling back to the largest size actually on offer.
    #[test]
    fn a_mix_takes_its_cover_from_the_artwork_relationship() {
        let resp: serde_json::Value = serde_json::from_str(RESPONSE).unwrap();
        let playlists = build_included_map(&resp, "playlists");
        let artworks = build_included_map(&resp, "artworks");

        let mix = mix_from_playlist(playlists.get("0113f754").unwrap(), &artworks).unwrap();

        assert_eq!(mix.title, "My New Arrivals");
        assert_eq!(
            mix.cover.as_deref(),
            Some("https://images.tidal.com/COVER-320")
        );
    }

    #[test]
    fn a_mix_without_artwork_is_still_listed() {
        let resp: serde_json::Value = serde_json::from_str(RESPONSE).unwrap();
        let playlists = build_included_map(&resp, "playlists");

        let mix = mix_from_playlist(playlists.get("0113f754").unwrap(), &HashMap::new()).unwrap();

        assert_eq!(mix.title, "My New Arrivals");
        assert_eq!(mix.cover, None);
    }
}
