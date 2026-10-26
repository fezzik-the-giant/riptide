// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

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
        // Built from the credentials alone rather than from `enabled`, which
        // the settings modal can flip mid-session: a client that only existed
        // when scrobbling started enabled would make turning it on need a restart.
        let client = match (
            config.session_key.clone(),
            config.api_key.clone(),
            config.api_secret.clone(),
        ) {
            (Some(sk), Some(ak), Some(as_)) => Some(LastfmClient::new(sk, ak, as_)),
            _ => None,
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
        if self.client.is_none() {
            if self.config.enabled {
                warn!("Last.fm client not initialized - missing session key or API credentials");
            }
            // Without credentials there is nothing the setting can turn on, so
            // this loop exists only to keep the channels drained. It must still
            // stop when they close: a closed channel yields `None` from `recv`
            // immediately and forever, so ignoring it spins this loop inside a
            // single `poll` — never yielding, never ending. `main` joins the
            // worker thread on the way out, so that hung the whole quit at 100%
            // of a core (#61).
            loop {
                tokio::select! {
                    cmd = self.cmd_rx.recv() => if cmd.is_none() { break },
                    evt = self.player_evt_rx.recv() => if evt.is_none() { break },
                }
            }
            return;
        }

        debug!("Last.fm scrobbler started");
        match (self.config.enabled, &self.config.username) {
            (true, Some(username)) => info!("Last.fm scrobbling enabled for user: {username}"),
            (false, _) => debug!("Last.fm scrobbling disabled"),
            _ => {}
        }

        let mut scrobble_ticker = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    let cmd = match cmd {
                        Some(c) => c,
                        None => break,
                    };
                    // Being switched off gates the scrobbling commands but not
                    // the one that switches it back on.
                    if self.config.enabled || matches!(cmd, LastfmCmd::SetEnabled(_)) {
                        self.handle_cmd(cmd).await;
                    }
                }
                evt = self.player_evt_rx.recv() => {
                    let evt = match evt {
                        Some(e) => e,
                        None => break,
                    };
                    if self.config.enabled {
                        self.handle_player_event(evt).await;
                    }
                }
                _ = scrobble_ticker.tick() => {
                    if self.config.enabled {
                        self.check_scrobble().await;
                    }
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

                if let Some(client) = &self.client
                    && let Err(e) = client
                        .update_now_playing(&artist, &track_name, album.as_deref())
                        .await
                {
                    warn!("Failed to update now playing: {}", e);
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
            LastfmCmd::SetEnabled(on) => {
                if self.config.enabled == on {
                    return;
                }
                self.config.enabled = on;
                // A track that was playing while scrobbling was off was never
                // timed from its start, so submitting it would be a guess. The
                // next track to start is the first one that can be scrobbled.
                self.current_track = None;
                self.last_scrobble_track_id = None;
                debug!("Last.fm scrobbling {}", if on { "on" } else { "off" });
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
                self.is_paused = paused;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The worker runs inside `block_on` on the shared worker thread, which
    /// `main` joins on the way out, so a `run()` that never returns wedges the
    /// whole process. The spin happens inside one `poll`, so it starves any
    /// timer sharing its runtime — the worker needs its own thread for the
    /// hang to be observable at all.
    #[test]
    fn a_worker_without_credentials_exits_when_its_channels_close() {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (player_evt_tx, player_evt_rx) = mpsc::unbounded_channel();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        let worker = LastfmWorker::new(LastfmConfig::default(), cmd_rx, player_evt_rx, event_tx);
        assert!(worker.client.is_none(), "setup: no credentials configured");

        drop(cmd_tx);
        drop(player_evt_tx);

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("tokio runtime")
                .block_on(worker.run());
            let _ = done_tx.send(());
        });

        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("run() must return once its senders are dropped, not spin forever");
    }
}
