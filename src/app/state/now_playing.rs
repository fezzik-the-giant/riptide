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
    art_image: Option<CachedImage>,
    pub art_loading: bool,
    presentation_art_image: Option<CachedImage>,
    presentation_art_load: PresentationArtLoad,
    pub art_source: Option<String>,
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
            art_image: None,
            art_loading: false,
            presentation_art_image: None,
            presentation_art_load: PresentationArtLoad::Idle,
            art_source: None,
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
    pub(crate) fn set_art_bytes(&mut self, bytes: Option<Vec<u8>>) {
        self.art_image = bytes.map(CachedImage::new);
    }

    pub(crate) fn set_presentation_art_bytes(&mut self, bytes: Option<Vec<u8>>) {
        self.presentation_art_image = bytes.map(CachedImage::new);
    }

    pub(crate) fn art_bytes(&self) -> Option<&[u8]> {
        self.art_image.as_ref().map(CachedImage::bytes)
    }

    pub(crate) fn presentation_art_bytes(&self) -> Option<&[u8]> {
        self.presentation_art_image.as_ref().map(CachedImage::bytes)
    }

    pub(crate) fn art_image(&self) -> Option<&CachedImage> {
        self.art_image.as_ref()
    }

    pub(crate) fn presentation_art_image(&self) -> Option<&CachedImage> {
        self.presentation_art_image.as_ref()
    }

    pub(crate) fn presentation_art_loading(&self) -> bool {
        self.presentation_art_load != PresentationArtLoad::Idle
    }

    pub(crate) fn presentation_art_discovering_cover(&self) -> bool {
        self.presentation_art_load == PresentationArtLoad::DiscoveringCover
    }

    pub(crate) fn begin_presentation_art_discovery(&mut self) {
        self.presentation_art_load = PresentationArtLoad::DiscoveringCover;
    }

    pub(crate) fn begin_presentation_art_fetch(&mut self) {
        self.presentation_art_load = PresentationArtLoad::Fetching;
    }

    pub(crate) fn finish_presentation_art_load(&mut self) {
        self.presentation_art_load = PresentationArtLoad::Idle;
    }

    pub(crate) fn is_current_album(&self, album_id: u64) -> bool {
        self.track.as_ref().map(|track| track.album.id) == Some(album_id)
    }

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PresentationArtLoad {
    #[default]
    Idle,
    DiscoveringCover,
    Fetching,
}

pub(crate) struct CachedImage {
    bytes: Vec<u8>,
    content_hash: u64,
}

impl CachedImage {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        let content_hash = image_content_hash(&bytes);
        Self {
            bytes,
            content_hash,
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn content_hash(&self) -> u64 {
        self.content_hash
    }
}

/// Hash image contents rather than their address because allocator reuse can
/// give different images the same pointer over the lifetime of the process.
pub(crate) fn image_content_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn fmt_secs(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_image_keeps_bytes_and_hash_in_sync() {
        let mut now_playing = NowPlaying::default();
        now_playing.set_art_bytes(Some(vec![1, 2, 3]));
        let first_hash = now_playing.art_image().unwrap().content_hash();

        now_playing.set_art_bytes(Some(vec![3, 2, 1]));
        let second = now_playing.art_image().unwrap();

        assert_eq!(second.bytes(), [3, 2, 1]);
        assert_ne!(second.content_hash(), first_hash);
    }
}
