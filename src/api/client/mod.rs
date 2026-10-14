// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

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

pub use search::{SearchAlbumPage, SearchArtistPage, SearchPlaylistPage, SearchTrackPage};

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

/// How close to expiry a token may get before a request renews it. Matches the
/// margin `auth::is_token_valid` applies at startup.
const REFRESH_MARGIN: chrono::TimeDelta = chrono::TimeDelta::seconds(60);

/// Access token plus the instant it dies, so a long-lived session can renew
/// before a request rather than after a 401.
struct TokenState {
    access_token: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl TokenState {
    fn new(access_token: String, expires_at: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        Self {
            access_token,
            expires_at,
        }
    }

    fn apply(&mut self, token: &TokenResponse) {
        self.access_token = token.access_token.clone();
        self.expires_at =
            Some(chrono::Utc::now() + chrono::TimeDelta::seconds(token.expires_in as i64));
    }

    fn needs_refresh(&self) -> bool {
        self.expires_at
            .is_some_and(|expiry| expiry <= chrono::Utc::now() + REFRESH_MARGIN)
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    auth: RwLock<TokenState>,
    config: Config,
}

impl ApiClient {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client");
        let auth = TokenState::new(
            config.access_token.clone().unwrap_or_default(),
            config
                .expires_at
                .as_deref()
                .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
                .map(|expiry| expiry.with_timezone(&chrono::Utc)),
        );
        Self {
            http,
            auth: RwLock::new(auth),
            config,
        }
    }

    /// The bearer token for an outgoing request, renewed first if it is within
    /// [`REFRESH_MARGIN`] of expiry.
    ///
    /// Tidal's device-flow tokens live four hours, so any session left open
    /// longer than that used to outlive its credentials: the token was read
    /// once at startup and never renewed, and every request after the boundary
    /// came back 401. Renewal happens under the write lock, which serialises
    /// concurrent refreshes — the worker spawns each request, so a batch of
    /// them can cross the boundary together — and makes the losers wait for the
    /// new token instead of firing with the dead one.
    async fn access_token(&self) -> Result<String> {
        {
            let auth = self.auth.read().await;
            if !auth.needs_refresh() {
                return Ok(auth.access_token.clone());
            }
        }

        let mut auth = self.auth.write().await;
        // A concurrent request may have renewed it while this one waited.
        if !auth.needs_refresh() {
            return Ok(auth.access_token.clone());
        }

        let refreshed = self.request_refresh().await?;
        auth.apply(&refreshed);
        tracing::info!("access token renewed, valid for {}s", refreshed.expires_in);
        Ok(auth.access_token.clone())
    }

    /// Exchange the stored refresh token for a new access token.
    ///
    /// Credentials go in HTTP Basic auth, matching the device flow: passing the
    /// secret as a form field instead yields a restricted token that only
    /// serves AAC.
    async fn request_refresh(&self) -> Result<TokenResponse> {
        let refresh_token = self
            .config
            .refresh_token
            .as_deref()
            .context("access token expired and no refresh token is stored — sign in again")?;
        let client_id = crate::api::auth::client_id(&self.config);

        let resp = self
            .http
            .post(format!("{}/token", crate::api::auth::AUTH_BASE))
            .basic_auth(
                client_id,
                Some(crate::api::auth::client_secret(&self.config)),
            )
            .form(&[
                ("client_id", client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .context("token refresh request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            anyhow::bail!("token refresh returned {status}: {snippet}");
        }
        resp.json().await.context("parse token refresh response")
    }

    async fn get_openapi<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let token = self.access_token().await?;
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
        let token = self.access_token().await?;
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
        let token = self.access_token().await?;
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

/// Resolve a JSON:API `links.next` value against the v2 base.
///
/// These endpoints return `next` as a path (`/userCollectionAlbums/...`), not an
/// absolute URL, so following it verbatim produces an unusable request.
pub(super) fn absolute_url(url: &str) -> String {
    if url.starts_with("http") {
        url.to_string()
    } else {
        format!("{OPENAPI_BASE}{url}")
    }
}

// ── Multi-segment FLAC playlist ────────────────────────────────────────────────

// ── DASH → HLS conversion ─────────────────────────────────────────────────────

#[cfg(test)]
mod url_tests {
    use super::absolute_url;

    #[test]
    fn a_relative_next_link_is_resolved_against_the_v2_base() {
        // links.next comes back as a path; following it verbatim is not a URL.
        assert_eq!(
            absolute_url("/userCollectionAlbums/me/relationships/items?page%5Bcursor%5D=abc"),
            "https://openapi.tidal.com/v2/userCollectionAlbums/me/relationships/items?page%5Bcursor%5D=abc"
        );
    }

    #[test]
    fn an_absolute_next_link_is_left_alone() {
        let url = "https://openapi.tidal.com/v2/userCollectionAlbums/me?page=2";
        assert_eq!(absolute_url(url), url);
    }
}

#[cfg(test)]
mod token_tests {
    use super::{TokenResponse, TokenState};
    use chrono::{Duration, Utc};

    #[test]
    fn a_token_inside_the_expiry_margin_is_due_for_refresh() {
        let fresh = TokenState::new("t".into(), Some(Utc::now() + Duration::hours(1)));
        assert!(!fresh.needs_refresh(), "an hour of life left is not stale");

        let expiring = TokenState::new("t".into(), Some(Utc::now() + Duration::seconds(30)));
        assert!(
            expiring.needs_refresh(),
            "a token expiring mid-request must be renewed before it is sent"
        );

        let expired = TokenState::new("t".into(), Some(Utc::now() - Duration::hours(2)));
        assert!(expired.needs_refresh(), "an expired token is stale");
    }

    #[test]
    fn a_refresh_response_without_a_new_refresh_token_still_renews_the_session() {
        // Verified live against auth.tidal.com: the refresh grant returns only
        // an access token and its lifetime — no rotated refresh token — so the
        // one from the device flow stays usable for the life of the install.
        let body = r#"{
            "access_token": "renewed",
            "token_type": "Bearer",
            "expires_in": 14400,
            "user_id": 123
        }"#;
        let parsed: TokenResponse = serde_json::from_str(body).expect("parse refresh response");
        assert!(parsed.refresh_token.is_none());

        let mut state = TokenState::new("stale".into(), Some(Utc::now() - Duration::hours(2)));
        assert!(state.needs_refresh());

        state.apply(&parsed);

        assert_eq!(state.access_token, "renewed");
        assert!(
            !state.needs_refresh(),
            "a renewed token is good for 4 hours"
        );
    }

    #[test]
    fn an_unknown_expiry_is_never_refreshed() {
        // Nothing to renew against, and refreshing every request would turn a
        // missing timestamp into a request storm.
        let unknown = TokenState::new("t".into(), None);
        assert!(!unknown.needs_refresh());
    }
}
