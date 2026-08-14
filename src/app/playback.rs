// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use crate::api::models::Track;
use crate::mpris::MprisState;
use crate::player::PlayerCmd;
use super::{App, StatusLevel};

impl App {
    pub fn play_track(&mut self, track: Track) {
        let id = track.id;
        self.now_playing.queue = vec![track.clone()];
        self.now_playing.queue_index = 0;
        // Don't set now_playing.track yet - wait for successful StreamUrl response
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        self.now_playing.shuffle = false;
        self.now_playing.original_queue = Vec::new();
        self.now_playing.source_playlist_uuid = None;
        self.now_playing.source_playlist_next_offset = 0;
        let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
    }

    pub fn play_tracks(&mut self, tracks: Vec<Track>, start_index: usize) {
        if tracks.is_empty() { return; }
        if self.now_playing.shuffle {
            self.now_playing.original_queue = tracks.clone();
            let mut queue = tracks;
            let current = queue.remove(start_index);
            use rand::seq::SliceRandom;
            queue.shuffle(&mut rand::thread_rng());
            queue.insert(0, current);
            let track_id = queue.first().map(|t| t.id);
            // Don't set track yet - wait for successful StreamUrl response
            self.now_playing.queue = queue;
            self.now_playing.queue_index = 0;
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(id) = track_id {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
            }
        } else {
            self.now_playing.original_queue = Vec::new();
            self.now_playing.source_playlist_uuid = None;
            self.now_playing.source_playlist_next_offset = 0;
            let track_id = tracks.get(start_index).map(|t| t.id);
            // Don't set track yet - wait for successful StreamUrl response
            self.now_playing.queue = tracks;
            self.now_playing.queue_index = start_index;
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(id) = track_id {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
            }
        }
    }

    /// Like `play_tracks`, but records the source playlist UUID so that pages that
    /// arrive after playback starts are automatically appended to the queue.
    pub fn play_playlist_tracks(&mut self, tracks: Vec<Track>, start_index: usize, uuid: String) {
        let next_offset = tracks.len() as u32;
        self.play_tracks(tracks, start_index);
        self.now_playing.source_playlist_uuid = Some(uuid);
        self.now_playing.source_playlist_next_offset = next_offset;
    }

    pub fn toggle_pause(&mut self) {
        let _ = self.player_tx.send(PlayerCmd::TogglePause);
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.now_playing.paused != paused {
            let _ = self.player_tx.send(PlayerCmd::TogglePause);
        }
    }

    pub fn next_track(&mut self) {
        let next_idx = self.now_playing.queue_index + 1;
        if next_idx < self.now_playing.queue.len() {
            self.now_playing.queue_index = next_idx;
            // Don't set track yet - wait for successful StreamUrl response
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(next_idx) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
        }
    }

    pub fn prev_track(&mut self) {
        if self.now_playing.queue_index > 0 {
            let prev_idx = self.now_playing.queue_index - 1;
            self.now_playing.queue_index = prev_idx;
            // Don't set track yet - wait for successful StreamUrl response
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(prev_idx) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
        }
    }

    pub fn toggle_shuffle(&mut self) {
        if self.now_playing.queue.is_empty() { return; }
        if self.now_playing.shuffle {
            self.now_playing.shuffle = false;
            if !self.now_playing.original_queue.is_empty() {
                let current_id = self.now_playing.track.as_ref().map(|t| t.id);
                self.now_playing.queue = std::mem::take(&mut self.now_playing.original_queue);
                if let Some(id) = current_id {
                    if let Some(idx) = self.now_playing.queue.iter().position(|t| t.id == id) {
                        self.now_playing.queue_index = idx;
                        if let Some(next) = self.now_playing.queue.get(idx + 1) {
                            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
                        }
                    }
                }
            }
            self.set_status("Shuffle off".to_string(), StatusLevel::Info);
        } else {
            self.now_playing.original_queue = self.now_playing.queue.clone();
            self.now_playing.shuffle = true;
            let qi = self.now_playing.queue_index;
            let current = self.now_playing.queue.remove(qi);
            {
                use rand::seq::SliceRandom;
                self.now_playing.queue.shuffle(&mut rand::thread_rng());
            }
            self.now_playing.queue.insert(0, current);
            self.now_playing.queue_index = 0;
            let _ = self.player_tx.send(PlayerCmd::RemoveNext);
            if let Some(next) = self.now_playing.queue.get(1) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
            }
            self.set_status("Shuffle on".to_string(), StatusLevel::Info);
        }
    }

    pub fn move_queue_track_up(&mut self) {
        let idx = self.queue_cursor;
        if idx == 0 { return; }
        let qi = self.now_playing.queue_index;
        let old_next_id = self.now_playing.queue.get(qi + 1).map(|t| t.id);

        self.now_playing.queue.swap(idx, idx - 1);

        let new_qi = if idx == qi { qi - 1 } else if idx - 1 == qi { qi + 1 } else { qi };
        self.now_playing.queue_index = new_qi;

        let new_next_id = self.now_playing.queue.get(new_qi + 1).map(|t| t.id);
        if new_next_id != old_next_id {
            let _ = self.player_tx.send(PlayerCmd::RemoveNext);
            if let Some(next) = self.now_playing.queue.get(new_qi + 1) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
            }
        }

        self.queue_cursor = idx - 1;
    }

    pub fn move_queue_track_down(&mut self) {
        let idx = self.queue_cursor;
        if idx + 1 >= self.now_playing.queue.len() { return; }
        let qi = self.now_playing.queue_index;
        let old_next_id = self.now_playing.queue.get(qi + 1).map(|t| t.id);

        self.now_playing.queue.swap(idx, idx + 1);

        let new_qi = if idx == qi { qi + 1 } else if idx + 1 == qi { qi - 1 } else { qi };
        self.now_playing.queue_index = new_qi;

        let new_next_id = self.now_playing.queue.get(new_qi + 1).map(|t| t.id);
        if new_next_id != old_next_id {
            let _ = self.player_tx.send(PlayerCmd::RemoveNext);
            if let Some(next) = self.now_playing.queue.get(new_qi + 1) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
            }
        }

        self.queue_cursor = idx + 1;
    }

    pub fn add_to_queue(&mut self, track: Track) {
        if self.now_playing.track.is_none() {
            self.play_track(track);
            return;
        }
        let title = track.title.clone();
        self.now_playing.queue.push(track);
        let qi = self.now_playing.queue_index;
        let new_idx = self.now_playing.queue.len() - 1;
        if new_idx == qi + 1 {
            let id = self.now_playing.queue[new_idx].id;
            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
        }
        self.set_status(format!("Queued: {title}"), StatusLevel::Info);
    }

    pub fn focus_queue(&mut self) {
        // Nothing to focus when the panel is collapsed — the cursor would land
        // in a pane the user cannot see.
        if !self.queue_visible || self.now_playing.queue.is_empty() { return; }
        self.queue_focused = true;
        self.queue_cursor = self.now_playing.queue_index;
    }

    pub fn unfocus_queue(&mut self) {
        self.queue_focused = false;
    }

    pub fn play_from_queue(&mut self, idx: usize) {
        if idx >= self.now_playing.queue.len() { return; }
        self.now_playing.queue_index = idx;
        self.now_playing.track = self.now_playing.queue.get(idx).cloned();
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        if let Some(track) = self.now_playing.queue.get(idx) {
            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
        }
        self.fetch_now_playing_metadata();
        self.push_mpris_state();
        self.queue_focused = false;
    }

    pub fn remove_from_queue(&mut self, idx: usize) {
        if idx >= self.now_playing.queue.len() { return; }
        let qi = self.now_playing.queue_index;

        if idx == qi {
            self.now_playing.queue.remove(idx);
            if self.now_playing.queue.is_empty() {
                self.now_playing.track = None;
                self.now_playing.active = false;
                self.now_playing.queue_index = 0;
                let _ = self.player_tx.send(PlayerCmd::Stop);
                self.push_mpris_state();
                self.queue_focused = false;
                return;
            }
            let new_idx = idx.min(self.now_playing.queue.len() - 1);
            self.now_playing.queue_index = new_idx;
            self.now_playing.track = self.now_playing.queue.get(new_idx).cloned();
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(new_idx) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
            self.fetch_now_playing_metadata();
        } else if idx == qi + 1 {
            self.now_playing.queue.remove(idx);
            let _ = self.player_tx.send(PlayerCmd::RemoveNext);
            if let Some(next) = self.now_playing.queue.get(qi + 1) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
            }
        } else {
            self.now_playing.queue.remove(idx);
            if idx < qi {
                self.now_playing.queue_index -= 1;
            }
        }

        if self.queue_cursor >= self.now_playing.queue.len() && !self.now_playing.queue.is_empty() {
            self.queue_cursor = self.now_playing.queue.len() - 1;
        }
        if self.now_playing.queue.is_empty() {
            self.queue_focused = false;
        }
    }

    pub fn push_mpris_state(&self) {
        let state = match &self.now_playing.track {
            Some(t) => MprisState {
                title: t.title.clone(),
                artist: t.artist_name().to_owned(),
                album: t.album.title.clone(),
                art_url: t.album.cover.as_deref()
                    .map(|id| format!(
                        "https://resources.tidal.com/images/{}/320x320.jpg",
                        id.replace('-', "/")
                    ))
                    .unwrap_or_default(),
                duration_us: t.duration as i64 * 1_000_000,
                position_us: (self.now_playing.position * 1_000_000.0) as i64,
                paused: self.now_playing.paused,
                active: self.now_playing.active,
            },
            None => MprisState::default(),
        };
        let _ = self.mpris_tx.send(state);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::api::models::{Album, ArtistRef, Track};
    use crate::mpris::MprisState;
    use super::App;

    fn track(id: u64) -> Track {
        Track {
            id,
            title: format!("Track {id}"),
            duration: 180,
            artist: Some(ArtistRef { name: "Artist".to_string() }),
            artists: vec![],
            album: Album {
                id: 1,
                title: "Album".to_string(),
                number_of_tracks: None,
                release_date: None,
                cover: None,
                artist: None,
                audio_quality: None,
                media_metadata: None,
                added_at: None,
                album_type: None,
            },
            audio_quality: None,
            media_metadata: None,
            added_at: None,
        }
    }

    fn make_app() -> App {
        let (api_tx, _)    = tokio::sync::mpsc::unbounded_channel();
        let (player_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (mpris_tx, _)  = tokio::sync::watch::channel(MprisState::default());
        let (lastfm_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (visualizer_enabled_tx, _) = tokio::sync::watch::channel(false);
        let (_, spectrum_rx) = tokio::sync::watch::channel(crate::visualizer::SpectrumState::Disabled);
        App::new(
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
            visualizer_enabled_tx,
            spectrum_rx,
            crate::app::Preferences::default(),
        )
    }

    // ── Shuffle ───────────────────────────────────────────────────────────────

    #[test]
    fn shuffle_on_keeps_all_tracks() {
        let mut app = make_app();
        let tracks: Vec<Track> = (1..=10).map(track).collect();
        app.play_tracks(tracks, 0);
        app.toggle_shuffle();

        assert!(app.now_playing.shuffle);
        assert_eq!(app.now_playing.queue.len(), 10);
        // Every original ID must still be present.
        for id in 1..=10u64 {
            assert!(app.now_playing.queue.iter().any(|t| t.id == id));
        }
    }

    #[test]
    fn shuffle_on_places_current_track_at_index_0() {
        let mut app = make_app();
        let tracks: Vec<Track> = (1..=10).map(track).collect();
        let playing_id = tracks[3].id;
        app.play_tracks(tracks, 3); // start midway
        app.toggle_shuffle();

        assert_eq!(app.now_playing.queue_index, 0);
        assert_eq!(app.now_playing.queue[0].id, playing_id);
    }

    #[test]
    fn shuffle_off_restores_original_order() {
        let mut app = make_app();
        let tracks: Vec<Track> = (1..=10).map(track).collect();
        let original_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
        app.play_tracks(tracks, 0);

        app.toggle_shuffle(); // ON
        app.toggle_shuffle(); // OFF

        assert!(!app.now_playing.shuffle);
        let restored: Vec<u64> = app.now_playing.queue.iter().map(|t| t.id).collect();
        assert_eq!(restored, original_ids);
    }

    #[test]
    fn shuffle_off_positions_queue_index_on_current_track() {
        let mut app = make_app();
        let tracks: Vec<Track> = (1..=10).map(track).collect();
        app.play_tracks(tracks, 0);

        app.toggle_shuffle(); // ON — current track moves to front

        // Advance two tracks (simulating playback)
        app.now_playing.queue_index = 2;
        app.now_playing.track = app.now_playing.queue.get(2).cloned();
        let playing_id = app.now_playing.track.as_ref().unwrap().id;

        app.toggle_shuffle(); // OFF — should restore and find the correct index

        let new_idx = app.now_playing.queue_index;
        assert_eq!(app.now_playing.queue[new_idx].id, playing_id);
    }

    #[test]
    fn new_queue_clears_shuffle_state() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 0);
        app.toggle_shuffle();
        assert!(app.now_playing.shuffle);

        // Starting a new non-shuffle play clears everything.
        app.now_playing.shuffle = false; // toggle_shuffle back off first
        app.play_tracks((6..=10).map(track).collect(), 0);
        assert!(app.now_playing.original_queue.is_empty());
        assert!(app.now_playing.source_playlist_uuid.is_none());
    }

    // ── Queue reordering ──────────────────────────────────────────────────────

    #[test]
    fn move_track_down_swaps_correctly() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 0);
        app.focus_queue();

        // Move track at cursor=1 down to position 2.
        app.queue_cursor = 1;
        app.move_queue_track_down();

        assert_eq!(app.now_playing.queue[1].id, 3);
        assert_eq!(app.now_playing.queue[2].id, 2);
        assert_eq!(app.queue_cursor, 2);
    }

    #[test]
    fn move_track_up_swaps_correctly() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 0);
        app.focus_queue();

        app.queue_cursor = 2;
        app.move_queue_track_up();

        assert_eq!(app.now_playing.queue[1].id, 3);
        assert_eq!(app.now_playing.queue[2].id, 2);
        assert_eq!(app.queue_cursor, 1);
    }

    #[test]
    fn move_current_track_down_updates_queue_index() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 1); // playing track 2 at index 1
        app.focus_queue();
        let playing_id = app.now_playing.queue[1].id;

        app.queue_cursor = 1;
        app.move_queue_track_down();

        assert_eq!(app.now_playing.queue[2].id, playing_id);
        assert_eq!(app.now_playing.queue_index, 2);
    }

    #[test]
    fn move_track_down_at_last_position_is_no_op() {
        let mut app = make_app();
        app.play_tracks((1..=3).map(track).collect(), 0);
        app.focus_queue();
        let ids_before: Vec<u64> = app.now_playing.queue.iter().map(|t| t.id).collect();

        app.queue_cursor = 2; // last item
        app.move_queue_track_down();

        let ids_after: Vec<u64> = app.now_playing.queue.iter().map(|t| t.id).collect();
        assert_eq!(ids_before, ids_after);
        assert_eq!(app.queue_cursor, 2);
    }

    #[test]
    fn move_track_up_at_first_position_is_no_op() {
        let mut app = make_app();
        app.play_tracks((1..=3).map(track).collect(), 0);
        app.focus_queue();
        let ids_before: Vec<u64> = app.now_playing.queue.iter().map(|t| t.id).collect();

        app.queue_cursor = 0;
        app.move_queue_track_up();

        let ids_after: Vec<u64> = app.now_playing.queue.iter().map(|t| t.id).collect();
        assert_eq!(ids_before, ids_after);
        assert_eq!(app.queue_cursor, 0);
    }

    // ── Queue removal ─────────────────────────────────────────────────────────

    #[test]
    fn remove_non_current_track_shrinks_queue() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 0);
        app.focus_queue();

        app.remove_from_queue(2); // remove middle track

        assert_eq!(app.now_playing.queue.len(), 4);
        assert!(!app.now_playing.queue.iter().any(|t| t.id == 3));
        assert_eq!(app.now_playing.queue_index, 0); // unchanged
    }

    #[test]
    fn remove_track_before_current_adjusts_queue_index() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 3); // playing index 3
        app.focus_queue();

        app.remove_from_queue(1); // remove a track before current

        assert_eq!(app.now_playing.queue_index, 2); // shifted down by 1
    }

    #[test]
    fn remove_only_track_clears_now_playing() {
        let mut app = make_app();
        app.play_track(track(42));
        app.focus_queue();

        app.remove_from_queue(0);

        assert!(app.now_playing.queue.is_empty());
        assert!(app.now_playing.track.is_none());
        assert!(!app.queue_focused);
    }

    #[test]
    fn remove_current_track_advances_to_next() {
        let mut app = make_app();
        app.play_tracks((1..=3).map(track).collect(), 0);
        app.focus_queue();
        let next_id = app.now_playing.queue[1].id;

        app.remove_from_queue(0);

        assert_eq!(app.now_playing.queue[0].id, next_id);
        assert_eq!(app.now_playing.queue_index, 0);
        assert_eq!(app.now_playing.queue.len(), 2);
    }
}
