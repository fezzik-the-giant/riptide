// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use super::{App, StatusLevel};
use crate::api::ApiRequest;
use crate::api::models::Track;
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
                let _ = self
                    .api_tx
                    .send(ApiRequest::ResolveStreamUrl { track_id: track.id });
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
    }

    pub fn push_mpris_state(&self) {
        let state = match &self.now_playing.track {
            Some(t) => MprisState {
                title: t.title.clone(),
                artist: t.artist_name().to_owned(),
                album: t.album.title.clone(),
                art_url: t
                    .album
                    .cover
                    .as_deref()
                    .map(|id| {
                        format!(
                            "https://resources.tidal.com/images/{}/320x320.jpg",
                            id.replace('-', "/")
                        )
                    })
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
    use super::App;
    use crate::api::ApiRequest;
    use crate::api::models::{Album, ArtistRef, Track};
    use crate::mpris::MprisState;
    use crate::player::{PlayerCmd, PlayerEvent};
    use tokio::sync::mpsc;

    fn track(id: u64) -> Track {
        Track {
            id,
            title: format!("Track {id}"),
            duration: 180,
            artist: Some(ArtistRef {
                name: "Artist".to_string(),
            }),
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
}
