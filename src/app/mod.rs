// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

mod state;
mod loading;
mod navigation;
mod playback;
mod library;
mod responses;

pub use state::*;
pub use crate::playlist::{PlaylistDetail, PlaylistDetailFocus};

use std::collections::HashSet;
use tokio::sync::{mpsc, watch};
use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist, Track};
use crate::lastfm::LastfmCmd;
use crate::mpris::MprisState;
use crate::player::PlayerCmd;
use crate::search::SearchState;

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub should_quit: bool,
    pub current_tab: Tab,
    pub view_stack: Vec<View>,
    pub art_fullscreen: bool,

    pub home_new_releases: HomeSection<Playlist>,
    pub home_daily_mixes: HomeSection<Playlist>,
    pub home_discovery_mixes: HomeSection<Playlist>,
    pub home_section_focus: HomeSectionFocus,

    pub artists:   StatefulList<Artist>,
    pub fav_albums: StatefulList<Album>,
    pub playlists: StatefulList<Playlist>,
    pub favorites: StatefulList<Track>,
    pub favorite_track_ids: HashSet<u64>,
    pub favorite_album_ids: HashSet<u64>,
    pub favorite_artist_ids: HashSet<u64>,
    pub search:    SearchState,
    pub command:   CommandState,
    pub sort_palette:  SortPalette,
    pub artist_selection: ArtistSelection,
    pub tracks_sort: Option<SortField>,
    pub artists_sort:   Option<SortField>,
    pub fav_albums_sort: Option<SortField>,
    pub playlists_sort: Option<SortField>,
    pub now_playing: NowPlaying,

    pub queue_focused: bool,
    pub queue_visible: bool,
    pub queue_cursor:  usize,
    queue_viewport: ListViewport,

    pub help_active: bool,
    pub help_scroll: u16,

    pub tick: u64,
    /// (message, level, Instant when set) — cleared automatically after ~5 s
    pub status: Option<(String, StatusLevel, std::time::Instant)>,

    pub api_tx:    mpsc::UnboundedSender<ApiRequest>,
    pub player_tx: mpsc::UnboundedSender<PlayerCmd>,
    pub mpris_tx:  watch::Sender<MprisState>,
    pub lastfm_tx: mpsc::UnboundedSender<LastfmCmd>,
}

impl App {
    pub fn new(
        api_tx:    mpsc::UnboundedSender<ApiRequest>,
        player_tx: mpsc::UnboundedSender<PlayerCmd>,
        mpris_tx:  watch::Sender<MprisState>,
        lastfm_tx: mpsc::UnboundedSender<LastfmCmd>,
        prefs:     Preferences,
    ) -> Self {
        let mut app = Self {
            should_quit: false,
            current_tab: Tab::Home,
            view_stack:  Vec::new(),
            art_fullscreen: false,
            home_new_releases: HomeSection::default(),
            home_daily_mixes: HomeSection::default(),
            home_discovery_mixes: HomeSection::default(),
            home_section_focus: HomeSectionFocus::default(),
            artists:     StatefulList::default(),
            fav_albums:  StatefulList::default(),
            playlists:   StatefulList::default(),
            favorites:   StatefulList::default(),
            favorite_track_ids: HashSet::new(),
            favorite_album_ids: HashSet::new(),
            favorite_artist_ids: HashSet::new(),
            search:      SearchState::default(),
            command:     CommandState::default(),
            sort_palette:    SortPalette::default(),
            artist_selection: ArtistSelection::default(),
            tracks_sort:  prefs.tracks_sort,
            artists_sort:    prefs.artists_sort,
            fav_albums_sort: prefs.fav_albums_sort,
            playlists_sort:  prefs.playlists_sort,
            now_playing: {
                let mut np = NowPlaying::default();
                // Load logo as default art
                if let Ok(logo_bytes) = std::fs::read("assets/wave-logo-320-transparent.png") {
                    np.set_art_bytes(Some(logo_bytes));
                }
                np.volume = prefs.volume;
                np.shuffle = prefs.shuffle;
                np
            },
            queue_focused: false,
            queue_visible: prefs.queue_visible,
            queue_cursor:  0,
            queue_viewport: ListViewport::default(),
            help_active: false,
            help_scroll: 0,
            tick:   0,
            status: None,
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
        };
        // mpv starts at its own default, so the restored level has to be pushed
        // across rather than just held in state.
        let _ = app.player_tx.send(PlayerCmd::SetVolume(prefs.volume));

        app.load_home();
        app.load_artists();
        app.load_fav_albums();
        app.load_playlists();
        app.load_favorites();
        app
    }

    /// Snapshot the persistable UI choices for writing back to `Config`.
    pub fn preferences(&self) -> Preferences {
        Preferences {
            tracks_sort: self.tracks_sort,
            artists_sort: self.artists_sort,
            fav_albums_sort: self.fav_albums_sort,
            playlists_sort: self.playlists_sort,
            volume: self.now_playing.volume,
            shuffle: self.now_playing.shuffle,
            queue_visible: self.queue_visible,
        }
    }

    pub fn queue_scroll_offset(&self, height: usize) -> usize {
        let selected = if self.queue_focused {
            self.queue_cursor
        } else {
            self.now_playing.queue_index
        };
        self.queue_viewport
            .offset(selected, self.now_playing.queue.len(), height)
    }

    pub fn queue_page_up(&mut self) {
        self.queue_cursor = self.queue_viewport
            .previous_page(self.queue_cursor, self.now_playing.queue.len());
    }

    pub fn queue_page_down(&mut self) {
        self.queue_cursor = self.queue_viewport
            .next_page(self.queue_cursor, self.now_playing.queue.len());
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if let Some((msg, _, set_at)) = &self.status {
            if set_at.elapsed() > std::time::Duration::from_secs(5) {
                tracing::debug!("Clearing status after {:.1}s: {}", set_at.elapsed().as_secs_f64(), msg);
                self.status = None;
            }
        }
    }

    pub(crate) fn set_status(&mut self, msg: String, level: StatusLevel) {
        self.status = Some((msg, level, std::time::Instant::now()));
    }

    pub(crate) fn rebuild_favorite_track_ids(&mut self) {
        self.favorite_track_ids = self.favorites.items.iter().map(|t| t.id).collect();
    }

    pub(crate) fn rebuild_favorite_album_ids(&mut self) {
        self.favorite_album_ids = self.fav_albums.items.iter().map(|a| a.id).collect();
    }

    /// Show/hide the queue panel. Hiding it while it holds focus would strand
    /// the cursor in an invisible pane, so focus returns to the content list.
    pub(crate) fn toggle_queue_visible(&mut self) {
        self.queue_visible = !self.queue_visible;
        if !self.queue_visible {
            self.queue_focused = false;
        }
    }

    pub(crate) fn rebuild_favorite_artist_ids(&mut self) {
        self.favorite_artist_ids = self.artists.items.iter().map(|a| a.id).collect();
    }

    /// Copy a share URL to the system clipboard and confirm via the status toast.
    pub(crate) fn copy_url(&mut self, url: String) {
        copy_to_clipboard(&url);
        self.set_status(format!("Copied link: {url}"), StatusLevel::Info);
    }

    pub(crate) fn load_search_tracks_next(&mut self) {
        if let Some(next_url) = self.search.tracks_next_url.take() {
            let _ = self.api_tx.send(crate::api::ApiRequest::SearchTracksNext { next_url });
        }
    }

    pub(crate) fn load_search_artists_next(&mut self) {
        if let Some(next_url) = self.search.artists_next_url.take() {
            let _ = self.api_tx.send(crate::api::ApiRequest::SearchArtistsNext { next_url });
        }
    }

    pub(crate) fn load_search_playlists_next(&mut self) {
        if let Some(next_url) = self.search.playlists_next_url.take() {
            let _ = self.api_tx.send(crate::api::ApiRequest::SearchPlaylistsNext { next_url });
        }
    }
}

#[cfg(test)]
pub(crate) fn test_app() -> (App, mpsc::UnboundedReceiver<ApiRequest>) {
    let (api_tx, api_rx) = mpsc::unbounded_channel();
    let (player_tx, _) = mpsc::unbounded_channel();
    let (mpris_tx, _) = watch::channel(MprisState::default());
    let (lastfm_tx, _) = mpsc::unbounded_channel();
    (
        App::new(
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
            Preferences::default(),
        ),
        api_rx,
    )
}

/// Write text to the terminal's clipboard using an OSC 52 escape sequence.
///
/// Requires the terminal emulator to honour OSC 52 (kitty, WezTerm, foot, and
/// recent Alacritty do by default; tmux needs `set -g set-clipboard on`). This
/// keeps riptide dependency-free and works transparently over SSH, unlike a
/// direct Wayland/X11 clipboard crate.
fn copy_to_clipboard(text: &str) {
    use base64::Engine as _;
    use std::io::Write;
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    print!("\x1b]52;c;{b64}\x07");
    let _ = std::io::stdout().flush();
}
