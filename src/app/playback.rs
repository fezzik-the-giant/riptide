// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use super::{App, StatusLevel};
use crate::api::ApiRequest;
use crate::api::models::{Track, presentation_art_url};
use crate::mpris::MprisState;
use crate::player::PlayerCmd;

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
        self.now_playing.clear_source_playlist();
        let _ = self
            .api_tx
            .send(ApiRequest::ResolveStreamUrl { track_id: id });
    }

    pub fn play_tracks(&mut self, tracks: Vec<Track>, start_index: usize) {
        if tracks.is_empty() {
            return;
        }
        let start_index = start_index.min(tracks.len() - 1);
        self.now_playing.clear_source_playlist();

        let (queue, queue_index) = if self.now_playing.shuffle {
            self.now_playing.original_queue = tracks.clone();
            let mut queue = tracks;
            let current = queue.remove(start_index);
            use rand::seq::SliceRandom;
            queue.shuffle(&mut rand::thread_rng());
            queue.insert(0, current);
            (queue, 0)
        } else {
            self.now_playing.original_queue = Vec::new();
            (tracks, start_index)
        };

        let track_id = queue.get(queue_index).map(|t| t.id);
        // Don't set track yet - wait for successful StreamUrl response
        self.now_playing.queue = queue;
        self.now_playing.queue_index = queue_index;
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        if let Some(id) = track_id {
            let _ = self
                .api_tx
                .send(ApiRequest::ResolveStreamUrl { track_id: id });
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
        // set_property is idempotent where `cycle` is not: DEs repeat MPRIS
        // Pause/Play, and two toggles racing the 500 ms pause-state poll would
        // undo each other.
        let _ = self.player_tx.send(PlayerCmd::SetPaused(paused));
    }

    pub fn next_track(&mut self) {
        let next_idx = self.now_playing.queue_index + 1;
        if next_idx < self.now_playing.queue.len() {
            self.now_playing.queue_index = next_idx;
            // Don't set track yet - wait for successful StreamUrl response
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(next_idx) {
                let _ = self
                    .api_tx
                    .send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
            // No MPRIS push here: `active` is false until the stream URL
            // resolves, and pushing would flash Stopped at clients on every
            // skip. The StreamUrl handler pushes as soon as playback restarts.
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
                let _ = self
                    .api_tx
                    .send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
        }
    }

    pub fn toggle_shuffle(&mut self) {
        if self.now_playing.queue.is_empty() {
            return;
        }
        if self.now_playing.shuffle {
            self.now_playing.shuffle = false;
            // Restoring is only safe if the current track can still be located in
            // the saved order; without that anchor `queue_index` would end up
            // addressing a queue it no longer belongs to. Anchor on the queue rather
            // than `now_playing.track`, which stays None until a stream URL resolves.
            let restore_to = self
                .now_playing
                .queue
                .get(self.now_playing.queue_index)
                .map(|t| t.id)
                .and_then(|id| {
                    self.now_playing
                        .original_queue
                        .iter()
                        .position(|t| t.id == id)
                });
            match restore_to {
                Some(idx) => {
                    self.now_playing.queue = std::mem::take(&mut self.now_playing.original_queue);
                    self.now_playing.queue_index = idx;
                    self.replace_prefetched_next();
                }
                None => self.now_playing.original_queue.clear(),
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
            self.replace_prefetched_next();
            self.set_status("Shuffle on".to_string(), StatusLevel::Info);
        }
        self.push_mpris_state();
    }

    /// Point mpv at whatever now follows the current track. Call after any
    /// reordering, so mpv's idea of "next" cannot outlive the queue that produced it.
    ///
    /// The entry mpv already holds is deliberately left alone until the replacement
    /// URL arrives — `PlayerCmd::SetNext` swaps them in one step. Clearing it here
    /// instead would leave mpv with an empty playlist for a whole round-trip, and a
    /// file appended to an exhausted playlist starts playing rather than queueing.
    /// `next_prefetched` therefore keeps describing what mpv actually holds, which is
    /// what lets `TrackEnded` notice if the track runs out first.
    pub(super) fn replace_prefetched_next(&mut self) {
        match self.now_playing.queue.get(self.now_playing.queue_index + 1) {
            Some(next) if self.now_playing.next_prefetched != Some(next.id) => {
                let track_id = next.id;
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id });
            }
            Some(_) => {}
            None => {
                let _ = self.player_tx.send(PlayerCmd::ClearNext);
                self.now_playing.next_prefetched = None;
            }
        }
    }

    pub fn move_queue_track_up(&mut self) {
        let idx = self.queue_cursor;
        if idx == 0 {
            return;
        }
        let qi = self.now_playing.queue_index;
        let old_next_id = self.now_playing.queue.get(qi + 1).map(|t| t.id);

        self.now_playing.queue.swap(idx, idx - 1);

        let new_qi = if idx == qi {
            qi - 1
        } else if idx - 1 == qi {
            qi + 1
        } else {
            qi
        };
        self.now_playing.queue_index = new_qi;

        let new_next_id = self.now_playing.queue.get(new_qi + 1).map(|t| t.id);
        if new_next_id != old_next_id {
            self.replace_prefetched_next();
        }

        self.queue_cursor = idx - 1;
        self.push_mpris_state();
    }

    pub fn move_queue_track_down(&mut self) {
        let idx = self.queue_cursor;
        if idx + 1 >= self.now_playing.queue.len() {
            return;
        }
        let qi = self.now_playing.queue_index;
        let old_next_id = self.now_playing.queue.get(qi + 1).map(|t| t.id);

        self.now_playing.queue.swap(idx, idx + 1);

        let new_qi = if idx == qi {
            qi + 1
        } else if idx + 1 == qi {
            qi - 1
        } else {
            qi
        };
        self.now_playing.queue_index = new_qi;

        let new_next_id = self.now_playing.queue.get(new_qi + 1).map(|t| t.id);
        if new_next_id != old_next_id {
            self.replace_prefetched_next();
        }

        self.queue_cursor = idx + 1;
        self.push_mpris_state();
    }

    pub fn add_to_queue(&mut self, track: Track) {
        if self.now_playing.track.is_none() {
            self.play_track(track);
            return;
        }
        let title = track.title.clone();
        if self.now_playing.shuffle {
            self.now_playing.original_queue.push(track.clone());
        }
        self.now_playing.queue.push(track);
        let qi = self.now_playing.queue_index;
        let new_idx = self.now_playing.queue.len() - 1;
        if new_idx == qi + 1 {
            let id = self.now_playing.queue[new_idx].id;
            let _ = self
                .api_tx
                .send(ApiRequest::ResolveStreamUrl { track_id: id });
        }
        self.set_status(format!("Queued: {title}"), StatusLevel::Info);
        self.push_mpris_state();
    }

    pub fn focus_queue(&mut self) {
        // Nothing to focus when the panel is collapsed — the cursor would land
        // in a pane the user cannot see.
        if !self.queue_visible || self.now_playing.queue.is_empty() {
            return;
        }
        self.queue_focused = true;
        self.queue_cursor = self.now_playing.queue_index;
    }

    pub fn unfocus_queue(&mut self) {
        self.queue_focused = false;
    }

    pub fn play_from_queue(&mut self, idx: usize) {
        if idx >= self.now_playing.queue.len() {
            return;
        }
        self.now_playing.queue_index = idx;
        self.now_playing.track = self.now_playing.queue.get(idx).cloned();
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        if let Some(track) = self.now_playing.queue.get(idx) {
            self.now_playing.play_pending = Some(track.id);
            let _ = self
                .api_tx
                .send(ApiRequest::ResolveStreamUrl { track_id: track.id });
        }
        self.fetch_now_playing_metadata();
        self.push_mpris_state();
        self.queue_focused = false;
    }

    pub fn remove_from_queue(&mut self, idx: usize) {
        if idx >= self.now_playing.queue.len() {
            return;
        }
        let qi = self.now_playing.queue_index;

        // Drop it from the pre-shuffle order too, or turning shuffle off would
        // bring it back.
        if self.now_playing.shuffle {
            let removed_id = self.now_playing.queue[idx].id;
            self.now_playing
                .original_queue
                .retain(|t| t.id != removed_id);
        }

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
                let _ = self
                    .api_tx
                    .send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
            self.fetch_now_playing_metadata();
        } else if idx == qi + 1 {
            self.now_playing.queue.remove(idx);
            self.replace_prefetched_next();
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
        self.push_mpris_state();
    }

    pub fn push_mpris_state(&self) {
        let np = &self.now_playing;
        let can_next = np.queue_index + 1 < np.queue.len();
        let can_prev = np.queue_index > 0 && !np.queue.is_empty();
        let state = match &np.track {
            Some(t) => MprisState {
                track_id: t.id,
                title: t.title.clone(),
                artists: if t.artists.is_empty() {
                    t.artist.iter().map(|a| a.name.clone()).collect()
                } else {
                    t.artists.iter().map(|a| a.name.clone()).collect()
                },
                album: t.album.title.clone(),
                art_url: np
                    .art_source
                    .as_deref()
                    .or(t.album.cover.as_deref())
                    .map(presentation_art_url)
                    .unwrap_or_default(),
                duration_us: t.duration as i64 * 1_000_000,
                position_us: (np.position * 1_000_000.0) as i64,
                position_epoch: np.position_epoch,
                paused: np.paused,
                active: np.active,
                volume: np.volume,
                shuffle: np.shuffle,
                can_next,
                can_prev,
                has_track: true,
            },
            None => MprisState {
                position_epoch: np.position_epoch,
                volume: np.volume,
                shuffle: np.shuffle,
                can_next,
                can_prev,
                ..MprisState::default()
            },
        };
        let _ = self.mpris_tx.send(state);
    }

    /// MPRIS Stop. mpv's `stop` clears its playlist, so nothing is prefetched
    /// any more and the queue can only be resumed through a fresh `Play`.
    pub fn stop_playback(&mut self) {
        let _ = self.player_tx.send(PlayerCmd::Stop);
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        self.now_playing.next_prefetched = None;
        self.now_playing.mpv_exhausted = true;
        self.now_playing.seek_pending = None;
        self.now_playing.play_pending = None;
        self.push_mpris_state();
    }

    /// MPRIS Play: resume when paused, restart the current queue entry when
    /// stopped (after Stop, or after the queue ran out).
    pub fn mpris_play(&mut self) {
        if self.stopped_with_queue() {
            self.play_from_queue(self.now_playing.queue_index);
        } else {
            self.set_paused(false);
        }
    }

    pub fn mpris_play_pause(&mut self) {
        if self.stopped_with_queue() {
            self.play_from_queue(self.now_playing.queue_index);
        } else {
            self.toggle_pause();
        }
    }

    /// Distinguishes "stopped" from the transient not-yet-active window while a
    /// stream URL resolves: only a stop or a run-out leaves mpv exhausted, and
    /// `track` stays None until a fresh queue's very first URL resolves — the
    /// one window where `mpv_exhausted` is still at its startup value.
    fn stopped_with_queue(&self) -> bool {
        self.now_playing.track.is_some()
            && !self.now_playing.active
            && self.now_playing.mpv_exhausted
            && self.now_playing.play_pending.is_none()
            && !self.now_playing.queue.is_empty()
    }

    pub fn set_volume_percent(&mut self, pct: u8) {
        let pct = pct.min(100);
        let _ = self.player_tx.send(PlayerCmd::SetVolume(pct));
        self.now_playing.volume = pct;
        self.push_mpris_state();
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.now_playing.shuffle {
            return;
        }
        // `toggle_shuffle` bails on an empty queue because there is no order to
        // rewrite, which silently dropped `playerctl shuffle` before anything was
        // playing — and left a saved shuffle preference impossible to turn off.
        if self.now_playing.queue.is_empty() {
            self.now_playing.shuffle = on;
            self.push_mpris_state();
            return;
        }
        self.toggle_shuffle();
    }

    pub fn seek_by_us(&mut self, offset_us: i64) {
        if !self.now_playing.active {
            return;
        }
        let target = self.now_playing.position + offset_us as f64 / 1_000_000.0;
        if self.now_playing.duration > 0.0 && target >= self.now_playing.duration {
            // The spec: seeking beyond the end acts like Next, and stops when
            // there is no next track. `next_track` is a no-op on the last entry,
            // which left the request doing nothing at all.
            if self.now_playing.queue_index + 1 < self.now_playing.queue.len() {
                self.next_track();
            } else {
                self.stop_playback();
            }
            return;
        }
        self.seek_to_secs(target.max(0.0));
    }

    pub fn set_position_us(&mut self, track_id: u64, position_us: i64) {
        // Stale-call protection required by the spec: the client's trackid must
        // still be the current track, and an out-of-range position is ignored.
        if self.now_playing.track.as_ref().map(|t| t.id) != Some(track_id) {
            return;
        }
        if !self.now_playing.active || position_us < 0 {
            return;
        }
        let target = position_us as f64 / 1_000_000.0;
        if self.now_playing.duration > 0.0 && target > self.now_playing.duration {
            return;
        }
        self.seek_to_secs(target);
    }

    fn seek_to_secs(&mut self, secs: f64) {
        let _ = self.player_tx.send(PlayerCmd::SeekAbsolute(secs));
        self.now_playing.seek_pending = Some(crate::app::PendingSeek {
            target_secs: secs,
            origin_secs: self.now_playing.position,
            polls_remaining: crate::app::PendingSeek::POLL_BUDGET,
        });
        self.now_playing.position = secs;
        self.now_playing.position_epoch += 1;
        self.push_mpris_state();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::App;
    use crate::api::ApiRequest;
    use crate::api::models::Track;
    use crate::app::test_support::track;
    use crate::mpris::MprisState;
    use crate::player::{PlayerCmd, PlayerEvent};
    use tokio::sync::mpsc;

    fn make_app() -> App {
        make_app_watching_api().0
    }

    /// Keeps the API receiver alive so tests can assert on what was requested.
    fn make_app_watching_api() -> (App, mpsc::UnboundedReceiver<ApiRequest>) {
        let (app, api_rx, _) = make_app_watching_all();
        (app, api_rx)
    }

    #[allow(clippy::type_complexity)]
    fn make_app_watching_all() -> (
        App,
        mpsc::UnboundedReceiver<ApiRequest>,
        mpsc::UnboundedReceiver<PlayerCmd>,
    ) {
        let (api_tx, api_rx) = mpsc::unbounded_channel();
        let (player_tx, player_rx) = mpsc::unbounded_channel();
        let (mpris_tx, _) = tokio::sync::watch::channel(MprisState::default());
        let (lastfm_tx, _) = mpsc::unbounded_channel();
        let app = App::new(
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
            crate::app::Preferences::default(),
        );
        (app, api_rx, player_rx)
    }

    fn resolved_track_ids(rx: &mut mpsc::UnboundedReceiver<ApiRequest>) -> Vec<u64> {
        let mut ids = Vec::new();
        while let Ok(req) = rx.try_recv() {
            if let ApiRequest::ResolveStreamUrl { track_id } = req {
                ids.push(track_id);
            }
        }
        ids
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

    /// Stand-in for mpv's playlist, matching the behaviour verified against the real
    /// player: entries are appended and never dropped, `pos` advances on EOF,
    /// `loadfile replace` resets both, `playlist-clear` keeps only the playing entry,
    /// and — the part that bites — a file appended to an exhausted playlist starts
    /// playing rather than queueing.
    #[derive(Default)]
    struct FakeMpv {
        playlist: Vec<u64>,
        pos: usize,
        exhausted: bool,
    }

    impl FakeMpv {
        fn apply(&mut self, cmd: PlayerCmd) {
            match cmd {
                PlayerCmd::Play(url) => {
                    self.playlist = vec![id_of(&url)];
                    self.pos = 0;
                    self.exhausted = false;
                }
                PlayerCmd::SetNext(url) => {
                    self.clear_but_playing();
                    self.append(id_of(&url));
                }
                PlayerCmd::ClearNext => self.clear_but_playing(),
                _ => {}
            }
        }

        fn clear_but_playing(&mut self) {
            if let Some(&cur) = self.playlist.get(self.pos) {
                self.playlist = vec![cur];
                self.pos = 0;
            }
        }

        fn append(&mut self, id: u64) {
            self.playlist.push(id);
            if self.exhausted {
                self.pos = self.playlist.len() - 1;
                self.exhausted = false;
            }
        }

        fn playing(&self) -> Option<u64> {
            if self.exhausted {
                return None;
            }
            self.playlist.get(self.pos).copied()
        }

        /// Natural end of the current entry; true if mpv rolled to another entry.
        fn end_of_file(&mut self) -> bool {
            if self.pos + 1 < self.playlist.len() {
                self.pos += 1;
                true
            } else {
                self.exhausted = true;
                false
            }
        }
    }

    fn id_of(url: &str) -> u64 {
        url.trim_start_matches("url-").parse().unwrap()
    }

    /// Drives one full turn of the real loop: player commands into mpv, API
    /// requests answered, responses fed back to the app until it settles.
    fn settle(
        app: &mut App,
        mpv: &mut FakeMpv,
        api_rx: &mut mpsc::UnboundedReceiver<ApiRequest>,
        player_rx: &mut mpsc::UnboundedReceiver<PlayerCmd>,
    ) {
        for _ in 0..32 {
            let mut progressed = false;
            while let Ok(cmd) = player_rx.try_recv() {
                let was_playing = mpv.playing();
                mpv.apply(cmd);
                // mpv reports file-loaded as soon as it loads media, long before any
                // API response could come back.
                if mpv.playing().is_some() && mpv.playing() != was_playing {
                    app.handle_player_event(PlayerEvent::TrackStarted);
                }
                progressed = true;
            }
            while let Ok(req) = api_rx.try_recv() {
                if let ApiRequest::ResolveStreamUrl { track_id } = req {
                    app.handle_api_response(crate::api::ApiResponse::StreamUrl {
                        track_id,
                        url: format!("url-{track_id}"),
                        delivered: Default::default(),
                    });
                }
                progressed = true;
            }
            if !progressed {
                return;
            }
        }
        panic!("app and mpv never settled");
    }

    /// The invariant the whole design exists to protect: whatever mpv is playing is
    /// what Now Playing describes. Vacuous once mpv has run out of playlist.
    fn assert_in_sync(app: &App, mpv: &FakeMpv, when: &str) {
        let Some(playing) = mpv.playing() else {
            return;
        };
        assert_eq!(
            app.now_playing.track.as_ref().map(|t| t.id),
            Some(playing),
            "{when}: Now Playing shows {:?} but mpv is playing {playing} (queue {:?}, qi {})",
            app.now_playing.track.as_ref().map(|t| t.id),
            app.now_playing
                .queue
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            app.now_playing.queue_index,
        );
    }

    #[test]
    fn unshuffling_and_reshuffling_does_not_change_the_playing_track() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();

        app.now_playing.shuffle = true;
        app.play_tracks((1..=8).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        assert_in_sync(&app, &mpv, "after starting playback");

        let playing = mpv.playing();

        app.toggle_shuffle();
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        assert_eq!(mpv.playing(), playing, "unshuffling changed the audio");
        assert_in_sync(&app, &mpv, "after z (unshuffle)");

        app.toggle_shuffle();
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        assert_eq!(mpv.playing(), playing, "reshuffling changed the audio");
        assert_in_sync(&app, &mpv, "after z (reshuffle)");
    }

    /// The reported bug: `z` used to clear mpv's queued entry immediately and only
    /// then go ask for a replacement URL. A track ending inside that round-trip left
    /// mpv with an exhausted playlist, so the late prefetch started playing instead
    /// of queueing — the audio moved while Now Playing did not.
    #[test]
    fn a_track_ending_while_a_prefetch_is_in_flight_does_not_hijack_playback() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();

        app.now_playing.shuffle = true;
        app.play_tracks((1..=8).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        let playing = mpv.playing();

        // Both presses land before any stream URL comes back.
        app.toggle_shuffle();
        app.toggle_shuffle();
        while let Ok(cmd) = player_rx.try_recv() {
            mpv.apply(cmd);
        }
        assert_eq!(
            mpv.playing(),
            playing,
            "mpv must still be playing the same track while the prefetch is in flight"
        );
        assert!(
            mpv.playlist.len() > mpv.pos + 1,
            "mpv must have something queued behind the current track, or a late \
             prefetch will start playing instead of queueing"
        );

        // Only now does the track run out, with the responses still outstanding.
        mpv.end_of_file();
        app.handle_player_event(PlayerEvent::TrackEnded);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);

        assert_in_sync(&app, &mpv, "after the track ended mid-prefetch");
    }

    #[test]
    fn queue_stays_in_sync_across_shuffling_and_natural_advances() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();

        app.play_tracks((1..=8).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);

        for step in 0..6 {
            // mpv reports EOF whether or not another entry follows.
            mpv.end_of_file();
            app.handle_player_event(PlayerEvent::TrackEnded);
            settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
            assert_in_sync(&app, &mpv, &format!("after advance {step}"));

            app.toggle_shuffle();
            settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
            assert_in_sync(&app, &mpv, &format!("after toggling shuffle at {step}"));
        }
    }

    /// Issue #43: with the same track at `queue_index` and `queue_index + 1`, the
    /// prefetch response satisfied the "current track" branch, re-issued `Play`,
    /// and requested the URL again — restarting the song in a loop until Tidal
    /// answered 429.
    #[test]
    fn a_duplicated_track_does_not_restart_playback_in_a_loop() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(7), track(7), track(9)], 0);

        let mut plays = 0;
        for _ in 0..12 {
            while let Ok(cmd) = player_rx.try_recv() {
                let was = mpv.playing();
                if matches!(cmd, PlayerCmd::Play(_)) {
                    plays += 1;
                }
                mpv.apply(cmd);
                if mpv.playing().is_some() && mpv.playing() != was {
                    app.handle_player_event(PlayerEvent::TrackStarted);
                }
            }
            let reqs: Vec<ApiRequest> = std::iter::from_fn(|| api_rx.try_recv().ok()).collect();
            if reqs.is_empty() {
                break;
            }
            for req in reqs {
                if let ApiRequest::ResolveStreamUrl { track_id } = req {
                    app.handle_api_response(crate::api::ApiResponse::StreamUrl {
                        track_id,
                        url: format!("url-{track_id}"),
                        delivered: Default::default(),
                    });
                }
            }
        }

        assert_eq!(plays, 1, "the track must be loaded once, not restarted");
        assert_eq!(mpv.playing(), Some(7));
        assert_eq!(
            mpv.playlist.len(),
            2,
            "the duplicate belongs in mpv's queue, not on top of what is playing"
        );
    }

    #[test]
    fn queueing_the_playing_track_again_does_not_restart_it() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(7)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        assert_eq!(mpv.playing(), Some(7));

        app.add_to_queue(track(7));

        let mut plays = 0;
        for _ in 0..12 {
            while let Ok(cmd) = player_rx.try_recv() {
                if matches!(cmd, PlayerCmd::Play(_)) {
                    plays += 1;
                }
                mpv.apply(cmd);
            }
            let reqs: Vec<ApiRequest> = std::iter::from_fn(|| api_rx.try_recv().ok()).collect();
            if reqs.is_empty() {
                break;
            }
            for req in reqs {
                if let ApiRequest::ResolveStreamUrl { track_id } = req {
                    app.handle_api_response(crate::api::ApiResponse::StreamUrl {
                        track_id,
                        url: format!("url-{track_id}"),
                        delivered: Default::default(),
                    });
                }
            }
        }

        assert_eq!(
            plays, 0,
            "queueing a track must never restart the current one"
        );
        assert_eq!(mpv.playing(), Some(7));
    }

    // ── Library removal and undo (#39) ────────────────────────────────────────

    #[test]
    fn undo_restores_the_last_removed_track() {
        let (mut app, mut api_rx) = make_app_watching_api();
        app.favorites.append(vec![track(1), track(2)], 2);
        app.rebuild_favorite_track_ids();
        let victim = app.favorites.items[0].clone();

        app.unfavorite_track(&victim);
        // The list itself shrinks when the API confirms; what matters here is that
        // the removal was recorded and can be replayed.
        assert!(app.last_removal.is_some());
        while api_rx.try_recv().is_ok() {}

        app.undo_last_removal();

        assert!(app.last_removal.is_none(), "the slot is consumed");
        let requested: Vec<u64> = std::iter::from_fn(|| api_rx.try_recv().ok())
            .filter_map(|r| match r {
                ApiRequest::FavoriteTrack { track_id } => Some(track_id),
                _ => None,
            })
            .collect();
        assert_eq!(
            requested,
            vec![victim.id],
            "re-favorites exactly what was removed"
        );
    }

    #[test]
    fn undo_with_nothing_removed_is_harmless() {
        let (mut app, mut api_rx) = make_app_watching_api();
        while api_rx.try_recv().is_ok() {} // App::new queues the startup loads

        app.undo_last_removal();

        assert!(app.last_removal.is_none());
        assert!(
            api_rx.try_recv().is_err(),
            "an idle undo must not call the API"
        );
    }

    #[test]
    fn a_second_undo_does_not_re_add_something_removed_on_purpose() {
        let (mut app, mut api_rx) = make_app_watching_api();
        app.favorites.append(vec![track(1)], 1);
        let victim = app.favorites.items[0].clone();

        app.unfavorite_track(&victim);
        app.undo_last_removal();
        while api_rx.try_recv().is_ok() {}

        app.undo_last_removal();

        assert!(
            api_rx.try_recv().is_err(),
            "the slot was already spent; a second undo must do nothing"
        );
    }

    #[test]
    fn only_the_most_recent_removal_is_undoable() {
        let (mut app, mut api_rx) = make_app_watching_api();
        app.favorites.append(vec![track(1), track(2)], 2);
        let first = app.favorites.items[0].clone();
        let second = app.favorites.items[1].clone();

        app.unfavorite_track(&first);
        app.unfavorite_track(&second);
        while api_rx.try_recv().is_ok() {}

        app.undo_last_removal();

        let requested: Vec<u64> = std::iter::from_fn(|| api_rx.try_recv().ok())
            .filter_map(|r| match r {
                ApiRequest::FavoriteTrack { track_id } => Some(track_id),
                _ => None,
            })
            .collect();
        assert_eq!(requested, vec![second.id]);
    }

    #[test]
    fn shuffle_off_keeps_tracks_queued_while_shuffling() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 0);
        app.now_playing.track = Some(app.now_playing.queue[0].clone());

        app.toggle_shuffle();
        app.add_to_queue(track(99));
        app.toggle_shuffle();

        assert!(app.now_playing.queue.iter().any(|t| t.id == 99));
    }

    #[test]
    fn shuffle_off_without_a_resolved_track_keeps_queue_index_valid() {
        let mut app = make_app();
        app.play_tracks((1..=5).map(track).collect(), 0);
        app.toggle_shuffle();
        // `now_playing.track` is still None here: play_tracks waits for a stream URL.
        let playing_id = app.now_playing.queue[app.now_playing.queue_index].id;

        app.toggle_shuffle();

        assert_eq!(
            app.now_playing.queue[app.now_playing.queue_index].id, playing_id,
            "the track at queue_index must not change under the player"
        );
    }

    #[test]
    fn new_queue_clears_source_playlist_while_shuffled() {
        let mut app = make_app();
        app.play_playlist_tracks((1..=5).map(track).collect(), 0, "playlist-a".to_string());
        app.now_playing.shuffle = true;

        app.play_tracks((6..=10).map(track).collect(), 0);

        assert!(app.now_playing.source_playlist_uuid.is_none());
        assert_eq!(app.now_playing.source_playlist_next_offset, 0);
    }

    // ── Advancing on mpv's own ────────────────────────────────────────────────

    #[test]
    fn track_ended_prefetches_the_following_track_when_mpv_kept_up() {
        let (mut app, mut api_rx) = make_app_watching_api();
        app.play_tracks((1..=3).map(track).collect(), 0);
        app.now_playing.active = true;
        app.now_playing.next_prefetched = Some(2);
        let _ = resolved_track_ids(&mut api_rx);

        app.handle_player_event(PlayerEvent::TrackEnded);

        assert_eq!(app.now_playing.queue_index, 1);
        assert_eq!(resolved_track_ids(&mut api_rx), vec![3]);
        assert!(app.now_playing.active);
    }

    #[test]
    fn track_ended_replays_the_current_track_when_mpv_had_nothing_queued() {
        let (mut app, mut api_rx) = make_app_watching_api();
        app.play_tracks((1..=3).map(track).collect(), 0);
        app.now_playing.active = true;
        app.now_playing.next_prefetched = None;
        let _ = resolved_track_ids(&mut api_rx);

        app.handle_player_event(PlayerEvent::TrackEnded);

        assert_eq!(app.now_playing.queue_index, 1);
        // Resolving the *current* track routes through the Play branch, which resets
        // mpv's playlist — rather than narrating a track mpv never loaded.
        assert_eq!(resolved_track_ids(&mut api_rx), vec![2]);
        assert!(!app.now_playing.active);
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

    // ── MPRIS control ─────────────────────────────────────────────────────────

    /// Keeps the MPRIS watch receiver alive: `watch::Sender::send` does not
    /// update the channel once every receiver is gone, so asserting on pushed
    /// state needs one held open.
    #[allow(clippy::type_complexity)]
    fn make_app_watching_mpris() -> (
        App,
        tokio::sync::watch::Receiver<MprisState>,
        mpsc::UnboundedReceiver<ApiRequest>,
        mpsc::UnboundedReceiver<PlayerCmd>,
    ) {
        let (api_tx, api_rx) = mpsc::unbounded_channel();
        let (player_tx, player_rx) = mpsc::unbounded_channel();
        let (mpris_tx, mpris_rx) = tokio::sync::watch::channel(MprisState::default());
        let (lastfm_tx, _) = mpsc::unbounded_channel();
        let app = App::new(
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
            crate::app::Preferences::default(),
        );
        (app, mpris_rx, api_rx, player_rx)
    }

    fn drain<T>(rx: &mut mpsc::UnboundedReceiver<T>) {
        while rx.try_recv().is_ok() {}
    }

    #[test]
    fn stop_reports_stopped_and_play_restarts_the_current_track() {
        let (mut app, mpris_rx, mut api_rx, mut player_rx) = make_app_watching_mpris();
        let mut mpv = FakeMpv::default();
        app.play_tracks((1..=3).map(track).collect(), 1);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        assert!(app.now_playing.active);

        app.stop_playback();
        assert!(!app.now_playing.active);
        assert!(app.now_playing.mpv_exhausted);
        assert!(!mpris_rx.borrow().active, "MPRIS must see Stopped");

        drain(&mut api_rx);
        app.mpris_play();
        assert_eq!(resolved_track_ids(&mut api_rx), vec![2]);
    }

    #[test]
    fn play_while_merely_paused_does_not_restart_the_track() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks((1..=3).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.handle_player_event(PlayerEvent::Paused(true));

        drain(&mut api_rx);
        drain(&mut player_rx);
        app.mpris_play();

        assert!(resolved_track_ids(&mut api_rx).is_empty());
        assert!(matches!(
            player_rx.try_recv(),
            Ok(PlayerCmd::SetPaused(false))
        ));
    }

    #[test]
    fn play_during_the_initial_resolve_does_not_double_resolve() {
        let (mut app, mut api_rx, _player_rx) = make_app_watching_all();
        app.play_tracks((1..=3).map(track).collect(), 0);
        drain(&mut api_rx); // the fresh queue's first resolve is in flight

        app.mpris_play();

        assert!(resolved_track_ids(&mut api_rx).is_empty());
    }

    /// The stopped-state Play has to be idempotent too: a repeated media key, or
    /// a desktop that sends Play twice, otherwise resolves the same track twice
    /// and each response restarts it from the beginning.
    #[test]
    fn repeated_play_while_stopped_resolves_once() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks((1..=3).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.stop_playback();
        drain(&mut api_rx);

        app.mpris_play();
        let first = resolved_track_ids(&mut api_rx);
        app.mpris_play();
        let second = resolved_track_ids(&mut api_rx);

        assert_eq!(first.len(), 1, "the first Play resolves the stopped track");
        assert!(second.is_empty(), "the second must wait for that URL");
    }

    /// A stream URL that never arrives must not wedge Play for good.
    #[test]
    fn a_failed_resolve_lets_play_try_again() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks((1..=3).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.stop_playback();
        drain(&mut api_rx);

        app.mpris_play();
        drain(&mut api_rx);
        app.handle_api_response(crate::api::ApiResponse::Error(
            "no stream URL available for track 1".to_string(),
        ));

        app.mpris_play();
        assert_eq!(resolved_track_ids(&mut api_rx).len(), 1);
    }

    /// `toggle_shuffle` bails on an empty queue, which used to swallow the
    /// setting entirely over D-Bus.
    #[test]
    fn shuffle_can_be_set_before_anything_is_playing() {
        let (mut app, _api_rx, _player_rx) = make_app_watching_all();
        assert!(app.now_playing.queue.is_empty());

        app.set_shuffle(true);
        assert!(app.now_playing.shuffle);

        app.set_shuffle(false);
        assert!(!app.now_playing.shuffle);
    }

    /// Seeking past the end of the *last* track has nowhere to advance to, and
    /// used to do nothing at all rather than stopping.
    #[test]
    fn seeking_past_the_end_of_the_last_track_stops() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 100.0;
        app.now_playing.position = 99.0;

        app.seek_by_us(60_000_000);

        assert!(!app.now_playing.active, "playback stopped");
        assert!(app.now_playing.mpv_exhausted);
    }

    #[test]
    fn skipping_does_not_flash_stopped_at_mpris_clients() {
        let (mut app, mpris_rx, mut api_rx, mut player_rx) = make_app_watching_mpris();
        let mut mpv = FakeMpv::default();
        app.play_tracks((1..=3).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        assert!(mpris_rx.borrow().active);

        app.next_track();

        // Mid-resolve, the last published state must still be the playing one.
        assert!(mpris_rx.borrow().active);
    }

    #[test]
    fn seek_within_the_track_moves_position_and_mpv() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;
        app.now_playing.position = 10.0;
        let epoch = app.now_playing.position_epoch;

        drain(&mut player_rx);
        app.seek_by_us(30_000_000);

        assert_eq!(app.now_playing.position, 40.0);
        assert_eq!(app.now_playing.position_epoch, epoch + 1);
        assert!(matches!(player_rx.try_recv(), Ok(PlayerCmd::SeekAbsolute(s)) if s == 40.0));
    }

    #[test]
    fn seek_past_the_end_acts_like_next() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks((1..=2).map(track).collect(), 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;
        app.now_playing.position = 170.0;

        drain(&mut api_rx);
        drain(&mut player_rx);
        app.seek_by_us(30_000_000);

        assert_eq!(app.now_playing.queue_index, 1);
        assert_eq!(resolved_track_ids(&mut api_rx), vec![2]);
        assert!(
            player_rx.try_recv().is_err(),
            "no seek is sent when advancing"
        );
    }

    #[test]
    fn seek_before_the_start_clamps_to_zero() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;
        app.now_playing.position = 5.0;

        drain(&mut player_rx);
        app.seek_by_us(-60_000_000);

        assert_eq!(app.now_playing.position, 0.0);
        assert!(matches!(player_rx.try_recv(), Ok(PlayerCmd::SeekAbsolute(s)) if s == 0.0));
    }

    #[test]
    fn set_position_for_a_stale_track_is_ignored() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;

        drain(&mut player_rx);
        app.set_position_us(9999, 10_000_000);

        assert!(player_rx.try_recv().is_err());
    }

    #[test]
    fn stale_position_polls_after_a_seek_are_dropped() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;
        app.now_playing.position = 100.0;

        app.seek_by_us(-90_000_000);
        assert_eq!(app.now_playing.position, 10.0);

        app.handle_player_event(PlayerEvent::Position(100.4));
        assert_eq!(
            app.now_playing.position, 10.0,
            "pre-seek poll must be dropped"
        );

        app.handle_player_event(PlayerEvent::Position(10.5));
        assert_eq!(app.now_playing.position, 10.5);
    }

    /// A seek shorter than any fixed tolerance: the stale poll sits close to
    /// the target, so staleness must be judged relative to the seek's origin,
    /// not by absolute distance.
    #[test]
    fn a_small_seek_does_not_wobble_or_signal_twice() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;
        app.now_playing.position = 50.0;

        app.seek_by_us(2_000_000);
        let epoch = app.now_playing.position_epoch;
        assert_eq!(app.now_playing.position, 52.0);

        app.handle_player_event(PlayerEvent::Position(50.0)); // pre-seek straggler
        assert_eq!(app.now_playing.position, 52.0, "must not snap back");

        app.handle_player_event(PlayerEvent::Position(52.4)); // the landing
        assert_eq!(app.now_playing.position, 52.4);
        assert_eq!(
            app.now_playing.position_epoch, epoch,
            "the landing must not fire a second Seeked"
        );
    }

    #[test]
    fn seek_poll_budget_exhaustion_falls_back_to_what_mpv_reports() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;
        app.now_playing.position = 100.0;

        app.seek_by_us(-90_000_000);
        let epoch = app.now_playing.position_epoch;

        for _ in 0..3 {
            app.handle_player_event(PlayerEvent::Position(100.5));
            assert_eq!(app.now_playing.position, 10.0);
        }
        // Budget spent: mpv evidently never landed the seek, so its position
        // is reality again and the jump back is a signalled discontinuity.
        app.handle_player_event(PlayerEvent::Position(100.5));
        assert_eq!(app.now_playing.position, 100.5);
        assert!(app.now_playing.seek_pending.is_none());
        assert_eq!(app.now_playing.position_epoch, epoch + 1);
    }

    #[test]
    fn set_position_accepts_in_range_and_rejects_out_of_range() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);
        app.now_playing.duration = 180.0;

        drain(&mut player_rx);
        app.set_position_us(1, 30_000_000);
        assert_eq!(app.now_playing.position, 30.0);
        assert!(matches!(
            player_rx.try_recv(),
            Ok(PlayerCmd::SeekAbsolute(s)) if s == 30.0
        ));

        app.set_position_us(1, -5);
        app.set_position_us(1, 200_000_000);
        assert!(player_rx.try_recv().is_err());
        assert_eq!(app.now_playing.position, 30.0);
    }

    #[test]
    fn mpris_volume_reaches_both_state_and_mpv() {
        let (mut app, _api_rx, mut player_rx) = make_app_watching_all();
        drain(&mut player_rx);

        app.set_volume_percent(55);

        assert_eq!(app.now_playing.volume, 55);
        assert!(matches!(player_rx.try_recv(), Ok(PlayerCmd::SetVolume(55))));
    }

    /// Queue selection fetches art immediately so the HUD and MPRIS update
    /// before the stream URL returns. That landing must not fetch again.
    #[test]
    fn queue_selection_does_not_refetch_presentation_art_when_stream_url_lands() {
        let (mut app, mut api_rx, mut player_rx) = make_app_watching_all();
        let mut mpv = FakeMpv::default();
        let mut first = track(1);
        first.album.id = 10;
        first.album.cover = Some("cover-1".into());
        let mut second = track(2);
        second.album.id = 20;
        second.album.cover = Some("cover-2".into());
        app.play_tracks(vec![first, second], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);

        app.art_fullscreen = true;
        app.play_from_queue(1);
        drain(&mut player_rx);

        let mut presentation_fetches = 0;
        while let Ok(req) = api_rx.try_recv() {
            if matches!(req, ApiRequest::FetchPresentationArt { album_id: 20, .. }) {
                presentation_fetches += 1;
            }
        }
        assert_eq!(presentation_fetches, 1);

        app.handle_api_response(crate::api::ApiResponse::AlbumArt {
            album_id: 20,
            image_data: vec![3, 2, 1],
        });
        app.handle_api_response(crate::api::ApiResponse::PresentationArt {
            album_id: 20,
            image_data: Some(vec![9, 8, 7]),
        });
        assert_eq!(app.now_playing.art_bytes(), Some([3, 2, 1].as_slice()));
        assert_eq!(
            app.now_playing.presentation_art_bytes(),
            Some([9, 8, 7].as_slice())
        );

        app.handle_api_response(crate::api::ApiResponse::StreamUrl {
            track_id: 2,
            url: "url-2".into(),
            delivered: Default::default(),
        });

        assert_eq!(app.now_playing.art_bytes(), Some([3, 2, 1].as_slice()));
        assert_eq!(
            app.now_playing.presentation_art_bytes(),
            Some([9, 8, 7].as_slice())
        );
        while let Ok(req) = api_rx.try_recv() {
            assert!(
                !matches!(req, ApiRequest::FetchPresentationArt { .. }),
                "StreamUrl must not refetch presentation art for a track the queue already made current"
            );
        }
    }

    /// The reported bug: v2 track details carry the artwork as a URL and leave
    /// `album.cover` empty, and replacing `now_playing.track` with them made the
    /// next position tick strip `mpris:artUrl` from the metadata.
    #[test]
    fn track_details_without_album_cover_keep_the_mpris_art_url() {
        let (mut app, mpris_rx, mut api_rx, mut player_rx) = make_app_watching_mpris();
        let mut mpv = FakeMpv::default();
        app.play_tracks(vec![track(1)], 0);
        settle(&mut app, &mut mpv, &mut api_rx, &mut player_rx);

        let art = "https://resources.tidal.com/images/a/b/320x320.jpg";
        let presentation = "https://resources.tidal.com/images/a/b/640x640.jpg";
        app.handle_api_response(crate::api::ApiResponse::TrackDetails {
            track_id: 1,
            track: track(1), // album.cover is None, as in real v2 details
            cover_url: Some(art.to_string()),
        });
        assert_eq!(app.now_playing.art_source.as_deref(), Some(art));
        assert_eq!(mpris_rx.borrow().art_url, presentation);

        app.handle_player_event(PlayerEvent::Position(3.0));
        assert_eq!(
            mpris_rx.borrow().art_url,
            presentation,
            "a position tick must not strip the art URL"
        );
    }

    #[test]
    fn mpris_art_url_uses_the_presentation_size() {
        let (mut app, mpris_rx, _api_rx, _player_rx) = make_app_watching_mpris();
        app.now_playing.track = Some(track(1));
        app.now_playing.art_source = Some("33fd4c9b-5673-4c1e-bbd4-5346d397b8e0".into());
        app.push_mpris_state();
        assert_eq!(
            mpris_rx.borrow().art_url,
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/640x640.jpg"
        );
    }
}
