// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use super::client::LastfmClient;
use crate::api::auth;
use anyhow::Result;

/// Initiate Last.fm authentication flow
pub async fn authenticate() -> Result<()> {
    let config = auth::load_config()?;

    println!("Riptide Last.fm Setup");
    println!("====================\n");

    if config.lastfm.api_key.is_none() || config.lastfm.api_secret.is_none() {
        println!("To enable Last.fm scrobbling, you need API credentials.");
        println!("These are free to create at: https://www.last.fm/api/account/create\n");
        println!("After registering, add your credentials to ~/.config/riptide/config.json:");
        println!(
            r#"
  "lastfm": {{
    "api_key": "your-api-key-here",
    "api_secret": "your-api-secret-here"
  }}
"#
        );
        println!("Then run this command again.");
        return Ok(());
    }

    let api_key = config.lastfm.api_key.clone().unwrap();
    let api_secret = config.lastfm.api_secret.clone().unwrap();

    println!("Getting Last.fm authentication token...");
    let token = match LastfmClient::get_auth_token(&api_key, &api_secret).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to get auth token: {}", e);
            eprintln!("\nMake sure your API key and secret are correct in:");
            eprintln!("~/.config/riptide/config.json");
            return Err(e);
        }
    };

    let auth_url = LastfmClient::get_auth_url(&api_key, &token);
    println!("\nOpen this URL in your browser to authorize Riptide:");
    println!("{}\n", auth_url);

    println!("Waiting for authorization... (press Ctrl+C to cancel)");
    println!("This may take a minute...\n");

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        match LastfmClient::get_session_key(&api_key, &api_secret, &token).await {
            Ok(session) => {
                println!("✓ Authorization successful!");
                println!("Username: {}", session.name);

                // Load config and update it
                let mut config = auth::load_config()?;
                config.lastfm.enabled = true;
                config.lastfm.username = Some(session.name);
                config.lastfm.session_key = Some(session.key);
                auth::save_config(&config)?;

                println!("✓ Last.fm scrobbling enabled in config");
                return Ok(());
            }
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("token has not been granted") {
                    println!("Waiting for authorization...");
                } else {
                    println!("Last.fm: {}", e);
                }
            }
        }
    }
}
