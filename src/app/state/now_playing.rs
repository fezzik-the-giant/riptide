// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Playback state: the queue, current track, and derived display helpers.

use super::*;

// ── Now playing ───────────────────────────────────────────────────────────────

/// A seek sent to mpv whose landing has not shown up in a position poll yet.
/// Polls already in flight when the seek was issued still answer with the
/// pre-seek position; a poll nearer `origin_secs` than `target_secs` is such a
/// straggler and must be dropped, or the progress bar snaps back and a bogus
/// `Seeked` fires. The budget bounds the damage if mpv never lands the seek.
pub struct PendingSeek {
    pub target_secs: f64,
    pub origin_secs: f64,
    pub polls_remaining: u8,
}

impl PendingSeek {
    pub const POLL_BUDGET: u8 = 3;
}

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
    /// Where the current track's cover lives, as handed to MPRIS clients. Kept
    /// separately from `track.album.cover` because v2 track details deliver the
    /// artwork as a URL while leaving `album.cover` empty.
    pub art_url: Option<String>,
    /// Bumped on every discontinuous position change so the MPRIS server knows
    /// to emit `Seeked`; ordinary playback progress must not trigger it.
    pub position_epoch: u64,
    /// Seek issued to mpv whose landing has not been observed yet; see
    /// [`PendingSeek`].
    pub seek_pending: Option<PendingSeek>,
    pub lyrics_synced: Vec<(f64, String)>,
    pub lyrics_plain: Vec<String>,
    pub lyrics_loading: bool,
    pub sample_rate: Option<u32>,
    /// What Tidal served for the current track. The quality badge describes the
    /// catalogue; this describes what is actually playing, and the two differ
    /// whenever the client is not entitled to the advertised tier.
    pub delivered: DeliveredQuality,
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
    /// Kept a superset of `queue` while shuffle is on, so tracks queued in the
    /// meantime survive the restore.
    pub original_queue: Vec<Track>,
    /// Track whose stream URL is currently appended to mpv's playlist as the next
    /// entry. mpv advances on its own, so this is the only way to tell whether it
    /// moved to the track the app is about to display.
    pub next_prefetched: Option<u64>,
    /// Whether mpv has run off the end of its playlist. Nothing may be appended
    /// while this holds: mpv *plays* a file appended to an exhausted playlist
    /// instead of queueing it, which would move the audio without the app knowing.
    pub mpv_exhausted: bool,
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
            art_url: None,
            position_epoch: 0,
            seek_pending: None,
            lyrics_synced: Vec::new(),
            lyrics_plain: Vec::new(),
            lyrics_loading: false,
            sample_rate: None,
            delivered: DeliveredQuality::default(),
            codec: None,
            volume: 100,
            shuffle: false,
            source_playlist_uuid: None,
            source_playlist_next_offset: 0,
            source_playlist_cursor: None,
            original_queue: Vec::new(),
            next_prefetched: None,
            mpv_exhausted: true,
            lastfm_sent: false,
        }
    }
}

impl NowPlaying {
    /// Detach the queue from the playlist it came from, so pages still in flight
    /// for that playlist are not appended to a queue the user has since replaced.
    pub fn clear_source_playlist(&mut self) {
        self.source_playlist_uuid = None;
        self.source_playlist_next_offset = 0;
        self.source_playlist_cursor = None;
    }

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
