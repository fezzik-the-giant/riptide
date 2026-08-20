// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

pub mod auth;
pub mod client;
pub mod worker;

use serde::{Deserialize, Serialize};

/// Last.fm configuration stored in riptide's config
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastfmConfig {
    /// Last.fm username
    pub username: Option<String>,
    /// Last.fm session key (auth token)
    pub session_key: Option<String>,
    /// Custom API key (optional, uses default if not provided)
    pub api_key: Option<String>,
    /// Custom API secret (optional, uses default if not provided)
    pub api_secret: Option<String>,
    /// Whether scrobbling is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Minimum seconds to play before scrobbling (default: 30, minimum: 30)
    #[serde(default = "default_min_seconds")]
    pub min_seconds: f64,
    /// Minimum percentage of track to play before scrobbling (default: 30, minimum: 30)
    #[serde(default = "default_min_percent")]
    pub min_percent: f64,
}

fn default_enabled() -> bool {
    true
}

fn default_min_seconds() -> f64 {
    30.0
}

fn default_min_percent() -> f64 {
    30.0
}

/// Scrobble submission state
#[derive(Debug, Clone)]
pub struct ScrobbleState {
    /// Track ID being played
    pub track_id: u64,
    /// Artist name
    pub artist: String,
    /// Track name
    pub track_name: String,
    /// Album name (optional)
    pub album: Option<String>,
    /// Track duration in seconds
    pub duration: f64,
    /// Timestamp when track started playing (Unix timestamp)
    pub timestamp: i64,
}

/// Commands sent to the Last.fm worker
#[derive(Debug)]
pub enum LastfmCmd {
    UpdatePlayingTrack {
        track_id: u64,
        artist: String,
        track_name: String,
        album: Option<String>,
        duration: f64,
    },
    Pause,
    Resume,
}

/// Events from the Last.fm worker
#[derive(Debug, Clone)]
pub enum LastfmEvent {
    Scrobbled {
        #[allow(dead_code)]
        track_name: String,
        #[allow(dead_code)]
        artist: String,
    },
}
