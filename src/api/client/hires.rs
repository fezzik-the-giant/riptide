// SPDX-License-Identifier: GPL-3.0-or-later

//! Optional second Tidal login, used only to request hi-res streams.

use anyhow::{Context, Result};
use std::path::Path;
use tokio::sync::Mutex;

use crate::api::auth;
use crate::api::models::{Config, SessionInfo, TokenResponse};

// Stream requests and prefetches can arrive together. Read the current file
// under one lock so a rotated refresh token is persisted before the next use.
static SESSION_LOCK: Mutex<()> = Mutex::const_new(());

pub(super) async fn load(
    http: &reqwest::Client,
    path: &Path,
    expected_user: Option<u64>,
) -> Result<Option<Config>> {
    load_at(http, path, expected_user, auth::AUTH_BASE, super::BASE).await
}

async fn load_at(
    http: &reqwest::Client,
    path: &Path,
    expected_user: Option<u64>,
    auth_base: &str,
    api_base: &str,
) -> Result<Option<Config>> {
    let Some(expected_user) = expected_user else {
        return Ok(None);
    };
    let _guard = SESSION_LOCK.lock().await;
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read hi-res login"),
    };
    let mut config: Config = serde_json::from_slice(&bytes).context("parse hi-res login")?;
    anyhow::ensure!(
        config.user_id == Some(expected_user),
        "Hi-res login belongs to a different Tidal account"
    );
    if !auth::is_token_valid(&config) {
        let client_id = config
            .client_id
            .as_deref()
            .context("hi-res client ID missing")?;
        let client_secret = config
            .client_secret
            .as_deref()
            .context("hi-res client secret missing")?;
        let refresh_token = config
            .refresh_token
            .as_deref()
            .context("hi-res refresh token missing")?;
        let response = http
            .post(format!("{auth_base}/token"))
            .timeout(std::time::Duration::from_secs(20))
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
                ("scope", "r_usr w_usr w_sub"),
            ])
            .send()
            .await
            .context("refresh hi-res login")?
            .error_for_status()
            .context("hi-res login rejected; run riptide --hires-auth")?;
        let token: TokenResponse = response.json().await.context("parse hi-res token")?;
        auth::apply_token(&mut config, token);
        anyhow::ensure!(
            config.user_id == Some(expected_user),
            "Hi-res account changed during refresh"
        );
        // Persist rotated tokens even if the following session lookup fails.
        auth::save_config_at(&config, path)?;
        let session: SessionInfo = http
            .get(format!("{api_base}/sessions"))
            .timeout(std::time::Duration::from_secs(20))
            .bearer_auth(config.access_token.as_deref().unwrap_or_default())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("read refreshed hi-res session")?;
        anyhow::ensure!(
            session.user_id == expected_user,
            "Hi-res account changed during refresh"
        );
        config.session_id = Some(session.session_id);
        if !session.country_code.is_empty() {
            config.country_code = session.country_code;
        }
        auth::save_config_at(&config, path)?;
    }
    Ok(Some(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};

    fn temporary_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("riptide-refresh-{}.json", rand::random::<u64>()))
    }

    #[tokio::test]
    async fn rotates_and_persists_refresh_token_then_reuses_saved_access_token() {
        let path = temporary_path();
        auth::save_config_at(
            &Config {
                client_id: Some("hires-client".into()),
                client_secret: Some("hires-secret".into()),
                access_token: Some("expired-access".into()),
                refresh_token: Some("old-refresh".into()),
                expires_at: Some("2000-01-01T00:00:00Z".into()),
                user_id: Some(42),
                country_code: "NL".into(),
                ..Config::default()
            },
            &path,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            timeout(Duration::from_secs(5), async move {
                for (route, response) in [
                    ("POST /token ", json!({"access_token": "new-access", "refresh_token": "rotated-refresh", "expires_in": 3600})),
                    ("GET /sessions ", json!({"sessionId": "new-session", "userId": 42, "countryCode": "NL"})),
                ] {
                    let (socket, _) = listener.accept().await.unwrap();
                    let mut stream = BufReader::new(socket);
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    assert!(line.starts_with(route));
                    let mut length = 0;
                    let mut authorization = String::new();
                    loop {
                        line.clear();
                        stream.read_line(&mut line).await.unwrap();
                        if line == "\r\n" { break; }
                        let (key, value) = line.split_once(':').unwrap();
                        if key.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap();
                        }
                        if key.eq_ignore_ascii_case("authorization") {
                            authorization = value.trim().to_owned();
                        }
                    }
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).await.unwrap();
                    if route.starts_with("POST") {
                        let body = String::from_utf8(body).unwrap();
                        assert!(body.contains("refresh_token=old-refresh"));
                        assert!(body.contains("client_secret=hires-secret"));
                        assert!(authorization.is_empty());
                    } else {
                        assert_eq!(authorization, "Bearer new-access");
                    }
                    let body = response.to_string();
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    stream.get_mut().write_all(response.as_bytes()).await.unwrap();
                    stream.get_mut().shutdown().await.unwrap();
                }
            }).await.unwrap();
        });
        let http = reqwest::Client::new();
        let config = load_at(&http, &path, Some(42), &base, &base)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        assert_eq!(config.refresh_token.as_deref(), Some("rotated-refresh"));
        assert_eq!(config.session_id.as_deref(), Some("new-session"));
        // The server is gone: reloading must use the persisted access token.
        let reloaded = load_at(&http, &path, Some(42), &base, &base)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.access_token.as_deref(), Some("new-access"));
        assert_eq!(reloaded.refresh_token.as_deref(), Some("rotated-refresh"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn ignores_missing_file_and_rejects_a_different_account() {
        let path = temporary_path();
        let http = reqwest::Client::new();
        assert!(load(&http, &path, Some(42)).await.unwrap().is_none());
        auth::save_config_at(
            &Config {
                user_id: Some(99),
                ..Config::default()
            },
            &path,
        )
        .unwrap();
        let error = load(&http, &path, Some(42)).await.unwrap_err();
        assert!(error.to_string().contains("different Tidal account"));
        std::fs::remove_file(path).unwrap();
    }
}
