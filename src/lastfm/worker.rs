// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use super::{LastfmCmd, LastfmConfig, LastfmEvent, ScrobbleState};
use crate::player::PlayerEvent;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use tracing::{debug, info, warn};

use super::client::LastfmClient;

/// Minimum seconds to scrobble (cannot be less than this)
const MIN_SCROBBLE_SECONDS: f64 = 30.0;
/// Minimum percent to scrobble (cannot be less than this)
const MIN_SCROBBLE_PERCENT: f64 = 30.0;

pub struct LastfmWorker {
    config: LastfmConfig,
    cmd_rx: mpsc::UnboundedReceiver<LastfmCmd>,
    player_evt_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    event_tx: mpsc::UnboundedSender<LastfmEvent>,
    client: Option<LastfmClient>,
    current_track: Option<ScrobbleState>,
    is_paused: bool,
    last_scrobble_track_id: Option<u64>,
}

impl LastfmWorker {
    pub fn new(
        config: LastfmConfig,
        cmd_rx: mpsc::UnboundedReceiver<LastfmCmd>,
        player_evt_rx: mpsc::UnboundedReceiver<PlayerEvent>,
        event_tx: mpsc::UnboundedSender<LastfmEvent>,
    ) -> Self {
        let client = if config.enabled {
            match (
                config.session_key.clone(),
                config.api_key.clone(),
                config.api_secret.clone(),
            ) {
                (Some(sk), Some(ak), Some(as_)) => Some(LastfmClient::new(sk, ak, as_)),
                _ => None,
            }
        } else {
            None
        };

        Self {
            config,
            cmd_rx,
            player_evt_rx,
            event_tx,
            client,
            current_track: None,
            is_paused: false,
            last_scrobble_track_id: None,
        }
    }

    pub async fn run(mut self) {
        if !self.config.enabled {
            debug!("Last.fm scrobbling disabled");
            loop {
                tokio::select! {
                    _ = self.cmd_rx.recv() => {},
                    _ = self.player_evt_rx.recv() => {},
                }
            }
        }

        if self.client.is_none() {
            warn!("Last.fm client not initialized - missing session key or API credentials");
            loop {
                tokio::select! {
                    _ = self.cmd_rx.recv() => {},
                    _ = self.player_evt_rx.recv() => {},
                }
            }
        }

        debug!("Last.fm scrobbler started");
        if let Some(ref username) = self.config.username {
            info!("Last.fm scrobbling enabled for user: {}", username);
        }

        let mut scrobble_ticker = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    let cmd = match cmd {
                        Some(c) => c,
                        None => break,
                    };
                    self.handle_cmd(cmd).await;
                }
                evt = self.player_evt_rx.recv() => {
                    let evt = match evt {
                        Some(e) => e,
                        None => break,
                    };
                    self.handle_player_event(evt).await;
                }
                _ = scrobble_ticker.tick() => {
                    self.check_scrobble().await;
                }
            }
        }
    }

    async fn handle_cmd(&mut self, cmd: LastfmCmd) {
        match cmd {
            LastfmCmd::UpdatePlayingTrack {
                track_id,
                artist,
                track_name,
                album,
                duration,
            } => {
                debug!(
                    "► Track: {} by {} ({}s, album: {})",
                    track_name,
                    artist,
                    duration as u32,
                    album.as_deref().unwrap_or("unknown")
                );

                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                self.current_track = Some(ScrobbleState {
                    track_id,
                    artist: artist.clone(),
                    track_name: track_name.clone(),
                    album: album.clone(),
                    duration,
                    timestamp,
                });

                self.last_scrobble_track_id = None;
                self.is_paused = false;

                if let Some(client) = &self.client {
                    if let Err(e) = client
                        .update_now_playing(&artist, &track_name, album.as_deref())
                        .await
                    {
                        warn!("Failed to update now playing: {}", e);
                    }
                }
            }
            LastfmCmd::Pause => {
                debug!("Playback paused");
                self.is_paused = true;
            }
            LastfmCmd::Resume => {
                debug!("Playback resumed");
                self.is_paused = false;
            }
        }
    }

    async fn handle_player_event(&mut self, evt: PlayerEvent) {
        match evt {
            PlayerEvent::TrackEnded => {
                self.current_track = None;
                self.last_scrobble_track_id = None;
            }
            PlayerEvent::Paused(paused) => {
                if paused {
                    self.is_paused = true;
                } else {
                    self.is_paused = false;
                }
            }
            _ => {}
        }
    }

    async fn check_scrobble(&mut self) {
        let Some(track) = &self.current_track else {
            return;
        };

        // Don't scrobble if already scrobbled
        if self.last_scrobble_track_id == Some(track.track_id) {
            return;
        }

        // Don't scrobble if paused or duration unknown
        if self.is_paused || track.duration <= 0.0 {
            return;
        }

        let elapsed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            - track.timestamp;

        let elapsed_secs = elapsed as f64;

        // Enforce minimum thresholds (users cannot configure below these)
        let min_secs = self.config.min_seconds.max(MIN_SCROBBLE_SECONDS);
        let min_percent = self.config.min_percent.max(MIN_SCROBBLE_PERCENT);

        // Scrobble when whichever is less: percentage of track OR minimum seconds
        let threshold = (track.duration * (min_percent / 100.0)).min(min_secs);

        if elapsed_secs >= threshold {
            debug!("Threshold reached, attempting scrobble");
            if let Some(client) = &self.client {
                info!(
                    "Sending to Last.fm: {} by {} ({})",
                    track.track_name,
                    track.artist,
                    track.album.as_deref().unwrap_or("no album")
                );
                match client
                    .scrobble(
                        &track.artist,
                        &track.track_name,
                        track.timestamp,
                        track.album.as_deref(),
                    )
                    .await
                {
                    Ok(_) => {
                        self.last_scrobble_track_id = Some(track.track_id);
                        info!("✓ Scrobbled: {} by {}", track.track_name, track.artist);
                        let _ = self.event_tx.send(LastfmEvent::Scrobbled {
                            track_name: track.track_name.clone(),
                            artist: track.artist.clone(),
                        });
                    }
                    Err(e) => {
                        warn!(
                            "✗ Failed to scrobble {} by {}: {}",
                            track.track_name, track.artist, e
                        );
                    }
                }
            } else {
                warn!("No Last.fm client available for scrobbling");
            }
        }
    }
}
