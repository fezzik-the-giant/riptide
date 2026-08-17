// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

use super::models::{Config, DeviceAuthResponse, SessionInfo, TokenResponse};

// Built-in client credentials — must match a client that has lossless
// streaming entitlement. These are the same credentials tiddl uses
// (https://github.com/oskvr37/tiddl), which are known to work for FLAC.
const DEFAULT_CLIENT_ID: &str = "4N3n6Q1x95LL5K7p";
const DEFAULT_CLIENT_SECRET: &str = "oKOXfJW371cX6xaZ0PyhgGNBdNLlBZd4AKKYougMjik=";

fn client_id(config: &Config) -> &str {
    config.client_id.as_deref().unwrap_or(DEFAULT_CLIENT_ID)
}

fn client_secret(config: &Config) -> &str {
    config
        .client_secret
        .as_deref()
        .unwrap_or(DEFAULT_CLIENT_SECRET)
}

const AUTH_BASE: &str = "https://auth.tidal.com/v1/oauth2";

/// Current auth generation. Bump this when changing client credentials
/// or auth method — forces users to re-authenticate.
const CURRENT_AUTH_GENERATION: u32 = 1;

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("riptide")
        .join("config.json")
}

pub fn load_config() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = config_path();
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, serde_json::to_string_pretty(config)?)?;
    Ok(())
}

fn is_token_valid(config: &Config) -> bool {
    let Some(ref token) = config.access_token else {
        return false;
    };
    if token.is_empty() {
        return false;
    }
    let Some(ref expires_at) = config.expires_at else {
        return false;
    };
    let Ok(expiry) = chrono::DateTime::parse_from_rfc3339(expires_at) else {
        return false;
    };
    expiry > chrono::Utc::now() + chrono::Duration::seconds(60)
}

pub fn ensure_auth(config: &mut Config) -> Result<()> {
    // Force re-auth whenever client credentials or auth method change.
    if config.access_token.is_some() && config.auth_generation < CURRENT_AUTH_GENERATION {
        eprintln!(
            "[riptide] Auth upgrade (gen {} → {}) — re-authenticating for lossless streaming...",
            config.auth_generation, CURRENT_AUTH_GENERATION,
        );
        config.access_token = None;
        config.refresh_token = None;
        config.expires_at = None;
        config.session_id = None;
        config.auth_generation = CURRENT_AUTH_GENERATION;
        save_config(config)?;
    }

    if is_token_valid(config) {
        // Re-fetch session info on each startup (session_id is ephemeral)
        if let Some(ref token) = config.access_token.clone() {
            let client = make_blocking_client()?;
            let _ = fetch_session_info(&client, token, config);
            save_config(config)?;
        }
        return Ok(());
    }

    if config.refresh_token.is_some() {
        match try_refresh_blocking(config) {
            Ok(()) => {
                config.auth_generation = CURRENT_AUTH_GENERATION;
                save_config(config)?;
                return Ok(());
            }
            Err(_) => {
                config.access_token = None;
                config.refresh_token = None;
            }
        }
    }

    run_device_auth_flow(config)?;
    config.auth_generation = CURRENT_AUTH_GENERATION;
    save_config(config)?;
    Ok(())
}

fn make_blocking_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36")
        .build()?)
}

fn fetch_session_info(
    client: &reqwest::blocking::Client,
    access_token: &str,
    config: &mut Config,
) -> Result<()> {
    let resp = client
        .get("https://api.tidal.com/v1/sessions")
        .bearer_auth(access_token)
        .send()?;

    if !resp.status().is_success() {
        bail!("GET /sessions returned {}", resp.status());
    }

    let info: SessionInfo = resp.json()?;
    config.session_id = Some(info.session_id);
    config.user_id = Some(info.user_id);
    if !info.country_code.is_empty() {
        config.country_code = info.country_code;
    }
    Ok(())
}

fn try_refresh_blocking(config: &mut Config) -> Result<()> {
    let client = make_blocking_client()?;

    let refresh_token = config
        .refresh_token
        .as_deref()
        .context("no refresh token")?
        .to_string();

    // Use HTTP Basic Auth (like tiddl does). Sending client_secret as
    // a form field grants a restricted token that only serves AAC.
    let resp = client
        .post(format!("{AUTH_BASE}/token"))
        .basic_auth(client_id(config), Some(client_secret(config)))
        .form(&[
            ("client_id", client_id(config)),
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
        ])
        .send()?;

    if !resp.status().is_success() {
        bail!("refresh failed: {}", resp.status());
    }

    let token: TokenResponse = resp.json()?;
    let access_token = token.access_token.clone();
    apply_token(config, token);
    fetch_session_info(&client, &access_token, config)?;
    save_config(config)?;
    Ok(())
}

pub fn run_device_auth_flow(config: &mut Config) -> Result<()> {
    let client = make_blocking_client()?;

    let resp = client
        .post(format!("{AUTH_BASE}/device_authorization"))
        .form(&[
            ("client_id", client_id(config)),
            ("scope", "r_usr w_usr w_sub"),
        ])
        .send()
        .context("device authorization request failed")?;

    if !resp.status().is_success() {
        bail!("device_authorization returned {}", resp.status());
    }

    let auth: DeviceAuthResponse = resp.json()?;

    println!();
    println!("╔══════════════════════════════════════════╗");
    println!("║           Tidal Authorization            ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Open:                                   ║");
    println!("║  {:<40}  ║", &auth.verification_uri_complete);
    println!("╠══════════════════════════════════════════╣");
    println!("║  Code: {:<34}║", &auth.user_code);
    println!("╚══════════════════════════════════════════╝");
    println!();
    println!("Waiting for authorization…");

    let _ = open::that(&auth.verification_uri_complete);

    let interval = std::time::Duration::from_secs(auth.interval as u64);

    loop {
        std::thread::sleep(interval);

        // Use HTTP Basic Auth for the token exchange — this grants
        // a token with full lossless streaming privileges (like tiddl).
        // Sending client_secret as a form field grants restricted AAC-only tokens.
        let result = client
            .post(format!("{AUTH_BASE}/token"))
            .basic_auth(client_id(config), Some(client_secret(config)))
            .form(&[
                ("client_id", client_id(config)),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", auth.device_code.as_str()),
                ("scope", "r_usr w_usr w_sub"),
            ])
            .send()?;

        match result.status().as_u16() {
            200 => {
                let token: TokenResponse = result.json()?;
                let access_token = token.access_token.clone();
                apply_token(config, token);
                fetch_session_info(&client, &access_token, config)?;
                save_config(config)?;
                println!("Authorized successfully.");
                return Ok(());
            }
            400 => {
                let body: serde_json::Value = result.json()?;
                match body["error"].as_str() {
                    Some("authorization_pending") => continue,
                    Some("expired_token") => bail!("Device code expired. Please restart."),
                    Some(e) => bail!("Auth error: {e}"),
                    None => bail!("Unknown auth error: {body}"),
                }
            }
            code => bail!("Unexpected status {code}"),
        }
    }
}
fn apply_token(config: &mut Config, token: TokenResponse) {
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(token.expires_in as i64);
    config.access_token = Some(token.access_token);
    if let Some(rt) = token.refresh_token {
        config.refresh_token = Some(rt);
    }
    config.expires_at = Some(expires_at.to_rfc3339());
    if let Some(user) = token.user {
        config.user_id = Some(user.user_id);
        if !user.country_code.is_empty() {
            config.country_code = user.country_code;
        }
    }
    if config.country_code.is_empty() {
        config.country_code = "US".to_string();
    }
}
