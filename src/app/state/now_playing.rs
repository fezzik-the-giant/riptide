// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Playback state: the queue, current track, and derived display helpers.

use super::*;

// ── Now playing ───────────────────────────────────────────────────────────────

pub struct NowPlaying {
    pub track: Option<Track>,
    /// True only after mpv fires TrackStarted; false on startup and after the queue empties.
    pub active: bool,
    pub paused: bool,
    pub position: f64,
    pub duration: f64,
    pub queue: Vec<Track>,
    pub queue_index: usize,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub presentation_art_bytes: Option<Vec<u8>>,
    pub presentation_art_loading: bool,
    pub art_source: Option<String>,
    pub lyrics_synced: Vec<(f64, String)>,
    pub lyrics_plain: Vec<String>,
    pub lyrics_loading: bool,
    pub sample_rate: Option<u32>,
    pub codec: Option<String>,
    pub volume: u8,
    pub shuffle: bool,
    /// UUID of the playlist this queue originated from, used to append arriving pages.
    pub source_playlist_uuid: Option<String>,
    /// How many tracks from that playlist have been loaded into the queue so far.
    pub source_playlist_next_offset: u32,
    /// Cursor for pagination of the source playlist
    pub source_playlist_cursor: Option<String>,
    /// Saved queue order before shuffling; restored when shuffle is toggled off.
    pub original_queue: Vec<Track>,
    /// Whether this track has been sent to Last.fm for scrobbling
    pub lastfm_sent: bool,
}

impl Default for NowPlaying {
    fn default() -> Self {
        Self {
            track: None,
            active: false,
            paused: true,
            position: 0.0,
            duration: 0.0,
            queue: Vec::new(),
            queue_index: 0,
            art_bytes: None,
            art_loading: false,
            presentation_art_bytes: None,
            presentation_art_loading: false,
            art_source: None,
            lyrics_synced: Vec::new(),
            lyrics_plain: Vec::new(),
            lyrics_loading: false,
            sample_rate: None,
            codec: None,
            volume: 100,
            shuffle: false,
            source_playlist_uuid: None,
            source_playlist_next_offset: 0,
            source_playlist_cursor: None,
            original_queue: Vec::new(),
            lastfm_sent: false,
        }
    }
}

impl NowPlaying {
    pub fn progress_ratio(&self) -> f64 {
        if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn position_display(&self) -> String {
        fmt_secs(self.position as u32)
    }

    pub fn duration_display(&self) -> String {
        fmt_secs(self.duration as u32)
    }
}

pub(super) fn fmt_secs(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}
