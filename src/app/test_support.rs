// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Shared scaffolding for tests that need a live [`App`].

use super::*;

/// An [`App`] with its channels held open.
///
/// The receivers have to outlive the app — dropping one makes every send on its
/// sender fail — so they are kept here even where a test ignores them.
pub(crate) struct TestApp {
    pub app: App,
    pub api_rx: mpsc::UnboundedReceiver<ApiRequest>,
    _player_rx: mpsc::UnboundedReceiver<PlayerCmd>,
    _mpris_rx: watch::Receiver<MprisState>,
    _lastfm_rx: mpsc::UnboundedReceiver<LastfmCmd>,
}

impl TestApp {
    /// Drop the requests `App::new` queues at startup, so a test sees only what
    /// the code under test asked for.
    pub fn drain_api(&mut self) {
        while self.api_rx.try_recv().is_ok() {}
    }

    pub fn api_requests(&mut self) -> Vec<ApiRequest> {
        let mut out = Vec::new();
        while let Ok(req) = self.api_rx.try_recv() {
            out.push(req);
        }
        out
    }
}

pub(crate) fn test_app() -> TestApp {
    let (api_tx, api_rx) = mpsc::unbounded_channel();
    let (player_tx, player_rx) = mpsc::unbounded_channel();
    let (mpris_tx, mpris_rx) = watch::channel(MprisState::default());
    let (lastfm_tx, lastfm_rx) = mpsc::unbounded_channel();
    let app = App::new(
        api_tx,
        player_tx,
        mpris_tx,
        lastfm_tx,
        crate::app::Preferences::default(),
    );
    TestApp {
        app,
        api_rx,
        _player_rx: player_rx,
        _mpris_rx: mpris_rx,
        _lastfm_rx: lastfm_rx,
    }
}

pub(crate) fn track(id: u64) -> crate::api::models::Track {
    use crate::api::models::{Album, ArtistRef, Track};
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
            media_metadata: None,
            added_at: None,
            album_type: None,
        },
        media_metadata: None,
        added_at: None,
    }
}
