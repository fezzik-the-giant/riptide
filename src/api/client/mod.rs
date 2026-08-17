// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Tidal API client.
//!
//! [`ApiClient`] holds the HTTP core — auth, the JSON:API request helpers and
//! the shared constants. Its methods are implemented across the domain modules
//! below, each of which owns both its requests and the parsers for their
//! responses.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use super::models::*;

mod parse;

mod albums;
mod artists;
mod mixes;
mod playlists;
mod radio;
mod search;
mod streaming;
mod tracks;

pub use search::{SearchArtistPage, SearchPlaylistPage, SearchTrackPage};

const BASE: &str = "https://api.tidal.com/v1";
const OPENAPI_BASE: &str = "https://openapi.tidal.com/v2";

// Private types for the openapi.tidal.com/v2 JSON:API collection endpoints.
#[derive(serde::Deserialize)]
pub(super) struct OpenApiRelPage {
    data: Vec<OpenApiRelItem>,
    #[serde(default)]
    included: Vec<OpenApiIncluded>,
    links: Option<OpenApiLinks>,
}

#[derive(serde::Deserialize)]
pub(super) struct OpenApiRelItem {
    id: String,
    meta: Option<OpenApiItemMeta>,
}

#[derive(serde::Deserialize)]
pub(super) struct OpenApiItemMeta {
    #[serde(rename = "addedAt")]
    added_at: Option<String>,
}

#[derive(serde::Deserialize)]
pub(super) struct OpenApiIncluded {
    id: String,
    attributes: Option<OpenApiPlaylistAttrs>,
}

#[derive(serde::Deserialize)]
pub(super) struct OpenApiPlaylistAttrs {
    name: String,
    #[serde(rename = "numberOfItems")]
    number_of_items: Option<u32>,
}

#[derive(serde::Deserialize)]
pub(super) struct OpenApiLinks {
    meta: Option<OpenApiLinksMeta>,
}

#[derive(serde::Deserialize)]
pub(super) struct OpenApiLinksMeta {
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36";

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

    // ── Artists ───────────────────────────────────────────────────────────────

    // ── Playlists ─────────────────────────────────────────────────────────────

    // ── Favorites ─────────────────────────────────────────────────────────────

    // ── Search ────────────────────────────────────────────────────────────────

    // ── Albums ────────────────────────────────────────────────────────────────

    // ── Radio ─────────────────────────────────────────────────────────────────

    // ── Lyrics ───────────────────────────────────────────────────────────────

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
}

// ── Multi-segment FLAC playlist ────────────────────────────────────────────────

// ── DASH → HLS conversion ─────────────────────────────────────────────────────
