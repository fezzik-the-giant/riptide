// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use anyhow::{Result, anyhow};
use md5;
use serde::Deserialize;
use std::collections::BTreeMap;

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_URL: &str = "https://www.last.fm/api/auth/";

#[derive(Debug, Deserialize)]
pub struct SessionInfo {
    pub key: String,
    pub name: String,
}

pub struct LastfmClient {
    session_key: String,
    api_key: String,
    api_secret: String,
}

impl LastfmClient {
    pub fn new(session_key: String, api_key: String, api_secret: String) -> Self {
        Self {
            session_key,
            api_key,
            api_secret,
        }
    }

    /// Build API signature for Last.fm authentication
    fn build_signature(params: &BTreeMap<&str, &str>, api_secret: &str) -> String {
        let mut to_sign = String::new();
        for (k, v) in params.iter() {
            to_sign.push_str(k);
            to_sign.push_str(v);
        }
        to_sign.push_str(api_secret);
        format!("{:x}", md5::compute(to_sign.as_bytes()))
    }

    /// Scrobble a track
    pub async fn scrobble(
        &self,
        artist: &str,
        track: &str,
        timestamp: i64,
        album: Option<&str>,
    ) -> Result<()> {
        let timestamp_str = timestamp.to_string();
        let mut sig_params = BTreeMap::new();
        sig_params.insert("method", "track.scrobble");
        sig_params.insert("artist", artist);
        sig_params.insert("track", track);
        sig_params.insert("timestamp", timestamp_str.as_str());
        sig_params.insert("sk", self.session_key.as_str());
        sig_params.insert("api_key", self.api_key.as_str());

        if let Some(a) = album {
            sig_params.insert("album", a);
        }

        let api_sig = Self::build_signature(&sig_params, &self.api_secret);

        let mut params = sig_params;
        params.insert("api_sig", &api_sig);
        params.insert("format", "json");

        let client = reqwest::Client::new();
        let response = client.post(API_ROOT).form(&params).send().await?;

        let text = response.text().await?;
        let body: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!(
                "Last.fm error {}: {}",
                err.get("code").unwrap_or(&serde_json::json!(0)),
                body.get("message").unwrap_or(&serde_json::json!(""))
            ));
        }

        Ok(())
    }

    /// Update now playing track
    pub async fn update_now_playing(
        &self,
        artist: &str,
        track: &str,
        album: Option<&str>,
    ) -> Result<()> {
        let mut sig_params = BTreeMap::new();
        sig_params.insert("method", "track.updateNowPlaying");
        sig_params.insert("artist", artist);
        sig_params.insert("track", track);
        sig_params.insert("sk", self.session_key.as_str());
        sig_params.insert("api_key", self.api_key.as_str());

        if let Some(a) = album {
            sig_params.insert("album", a);
        }

        let api_sig = Self::build_signature(&sig_params, &self.api_secret);

        let mut params = sig_params;
        params.insert("api_sig", &api_sig);
        params.insert("format", "json");

        let client = reqwest::Client::new();
        let response = client.post(API_ROOT).form(&params).send().await?;

        let text = response.text().await?;
        let body: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| anyhow!("Failed to parse response: {}", e))?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!(
                "Last.fm error {}: {}",
                err.get("code").unwrap_or(&serde_json::json!(0)),
                body.get("message").unwrap_or(&serde_json::json!(""))
            ));
        }

        Ok(())
    }

    /// Get an authentication token
    pub async fn get_auth_token(api_key: &str, api_secret: &str) -> Result<String> {
        let mut params = BTreeMap::new();
        params.insert("method", "auth.getToken");
        params.insert("api_key", api_key);

        let api_sig = Self::build_signature(&params, api_secret);
        params.insert("api_sig", &api_sig);
        params.insert("format", "json");

        let client = reqwest::Client::new();
        let response = client.post(API_ROOT).form(&params).send().await?;

        let status = response.status();
        let text = response.text().await?;

        if !status.is_success() {
            return Err(anyhow!("Last.fm API error ({}): {}", status, text));
        }

        let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            anyhow!(
                "Failed to parse Last.fm response: {} (response: {})",
                e,
                text
            )
        })?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!("Last.fm error: {}", err));
        }

        let token = body
            .get("token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow!("No token in response: {}", text))?;

        Ok(token.to_string())
    }

    /// Get the authorization URL for user to visit
    pub fn get_auth_url(api_key: &str, token: &str) -> String {
        format!("{}?api_key={}&token={}", AUTH_URL, api_key, token)
    }

    /// Get session key after user has authorized
    pub async fn get_session_key(
        api_key: &str,
        api_secret: &str,
        token: &str,
    ) -> Result<SessionInfo> {
        let mut params = BTreeMap::new();
        params.insert("method", "auth.getSession");
        params.insert("token", token);
        params.insert("api_key", api_key);

        let api_sig = Self::build_signature(&params, api_secret);
        params.insert("api_sig", &api_sig);
        params.insert("format", "json");

        let client = reqwest::Client::new();
        let response = client.post(API_ROOT).form(&params).send().await?;

        let text = response.text().await?;
        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow!("Failed to parse response: {} (response: {})", e, text))?;

        if let Some(err) = body.get("error") {
            let err_code = body.get("error").and_then(|e| e.as_i64()).unwrap_or(0);
            if err_code == 14 {
                return Err(anyhow!("Token has not been granted yet"));
            }
            return Err(anyhow!("Last.fm error: {}", err));
        }

        let session = body
            .get("session")
            .ok_or_else(|| anyhow!("No session in response"))?;

        let key = session
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow!("No session key"))?;
        let name = session
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| anyhow!("No session name"))?;

        Ok(SessionInfo {
            key: key.to_string(),
            name: name.to_string(),
        })
    }
}
