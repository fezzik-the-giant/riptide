// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

mod library;
mod loading;
mod navigation;
mod playback;
mod responses;
mod state;
#[cfg(test)]
pub(crate) mod test_support;

pub use crate::playlist::{PlaylistDetail, PlaylistDetailFocus};
pub use state::*;

use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist, Track};
use crate::lastfm::LastfmCmd;
use crate::mpris::MprisState;
use crate::player::PlayerCmd;
use crate::search::SearchState;
use std::collections::HashSet;
use tokio::sync::{mpsc, watch};

/// Placeholder art shown before anything is playing.
///
/// Embedded rather than read from `assets/` at runtime: that path resolved
/// against the working directory, so it only ever loaded when the binary was
/// launched from the repo root, and silently showed nothing once installed.
const DEFAULT_ART: &[u8] = include_bytes!("../../assets/wave-logo-320-transparent.png");

// ── App ───────────────────────────────────────────────────────────────────────

/// Ends of the self-update channels owned by the update actor thread (main.rs).
/// `check_tx` carries `Ok(tag)`/`Ok(None)`/`Err(message)` from availability
/// checks; `cmd_rx` receives Check/Install commands from the UI.
pub struct UpdateActorHandles {
    pub check_tx: mpsc::UnboundedSender<Result<Option<String>, String>>,
    pub checking_tx: mpsc::UnboundedSender<UpdatePhase>,
    pub result_tx: mpsc::UnboundedSender<Result<crate::update::UpdateOutcome, String>>,
    pub cmd_rx: mpsc::UnboundedReceiver<UpdateCmd>,
}

pub struct App {
    pub should_quit: bool,
    pub current_tab: Tab,
    pub view_stack: Vec<View>,
    pub art_fullscreen: bool,

    pub home_new_releases: HomeSection<Playlist>,
    pub home_daily_mixes: HomeSection<Playlist>,
    pub home_discovery_mixes: HomeSection<Playlist>,
    pub home_section_focus: HomeSectionFocus,
    pub home_art: HomeArt,

    pub artists: StatefulList<Artist>,
    pub fav_albums: StatefulList<Album>,
    pub playlists: StatefulList<Playlist>,
    pub favorites: StatefulList<Track>,
    pub favorite_track_ids: HashSet<u64>,
    pub favorite_album_ids: HashSet<u64>,
    pub favorite_artist_ids: HashSet<u64>,
    /// Playlists are keyed by uuid, not the numeric id the other three use.
    /// Mirrors `playlists.items` exactly — the two are updated together, so a
    /// removal still in flight never reads as already gone.
    pub favorite_playlist_ids: HashSet<String>,
    pub search: SearchState,
    pub command: CommandState,
    /// Whether the filter box is open and capturing input. The query itself
    /// lives on each list, so every tab keeps its own.
    pub filter_active: bool,
    pub sort_palette: SortPalette,
    pub artist_selection: ArtistSelection,
    pub tracks_sort: Option<SortField>,
    pub artists_sort: Option<SortField>,
    pub fav_albums_sort: Option<SortField>,
    pub playlists_sort: Option<SortField>,
    pub now_playing: NowPlaying,

    pub queue_focused: bool,
    pub queue_visible: bool,
    pub queue_cursor: usize,
    queue_viewport: ListViewport,

    /// Most recent library removal, restorable with `u` until the next one.
    pub last_removal: Option<Removal>,

    pub help_active: bool,
    pub help_scroll: u16,

    /// Self-update availability + dialog state.
    pub update: UpdateState,
    /// Receives the update-check outcome (found tag / none / failed).
    pub update_rx: mpsc::UnboundedReceiver<Result<Option<String>, String>>,
    /// Receives the actor's progress through resolving the install method and
    /// running the background check.
    pub checking_rx: mpsc::UnboundedReceiver<UpdatePhase>,
    /// Receives the result of a TUI-triggered update install.
    pub update_result_rx: mpsc::UnboundedReceiver<Result<crate::update::UpdateOutcome, String>>,
    /// Sends Check/Install commands to the update actor thread.
    pub update_cmd_tx: mpsc::UnboundedSender<UpdateCmd>,
    /// Cancellation flag for an in-flight download/install; set when the
    /// user quits while Working so the actor aborts before replacing the binary.
    pub update_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Actor-thread ends of the update channels; taken by main() to spawn it.
    update_actor: Option<UpdateActorHandles>,

    pub tick: u64,
    /// When the marquee's cycle started. Reset on every keypress, so a row the
    /// cursor just landed on scrolls from its start rather than picking up
    /// wherever the clock happened to be.
    ///
    /// Wall-clock rather than a frame count: the draw loop's rate depends on the
    /// terminal and on what is being drawn — album art roughly halves it — so
    /// timing the cycle in frames made the same constants mean different things
    /// on different setups.
    pub marquee_epoch: std::time::Instant,
    /// (message, level, Instant when set) — cleared automatically after ~5 s
    pub status: Option<(String, StatusLevel, std::time::Instant)>,

    pub api_tx: mpsc::UnboundedSender<ApiRequest>,
    pub player_tx: mpsc::UnboundedSender<PlayerCmd>,
    pub mpris_tx: watch::Sender<MprisState>,
    pub lastfm_tx: mpsc::UnboundedSender<LastfmCmd>,
}

impl App {
    pub fn new(
        api_tx: mpsc::UnboundedSender<ApiRequest>,
        player_tx: mpsc::UnboundedSender<PlayerCmd>,
        mpris_tx: watch::Sender<MprisState>,
        lastfm_tx: mpsc::UnboundedSender<LastfmCmd>,
        prefs: Preferences,
    ) -> Self {
        // Self-update plumbing. The check/install thread is NOT started here
        // (App::new is also used in tests, which must stay offline) — main()
        // takes the senders back and spawns the actor.
        let (update_check_tx, update_rx) = mpsc::unbounded_channel();
        let (checking_tx, checking_rx) = mpsc::unbounded_channel();
        let (update_result_tx, update_result_rx) = mpsc::unbounded_channel();
        let (update_cmd_tx, update_cmd_rx) = mpsc::unbounded_channel();

        let mut app = Self {
            should_quit: false,
            current_tab: Tab::Home,
            view_stack: Vec::new(),
            art_fullscreen: false,
            home_new_releases: HomeSection::default(),
            home_daily_mixes: HomeSection::default(),
            home_discovery_mixes: HomeSection::default(),
            home_section_focus: HomeSectionFocus::default(),
            home_art: HomeArt::default(),
            artists: StatefulList::default(),
            fav_albums: StatefulList::default(),
            playlists: StatefulList::default(),
            favorites: StatefulList::default(),
            favorite_track_ids: HashSet::new(),
            favorite_album_ids: HashSet::new(),
            favorite_artist_ids: HashSet::new(),
            favorite_playlist_ids: HashSet::new(),
            search: SearchState::default(),
            command: CommandState::default(),
            filter_active: false,
            sort_palette: SortPalette::default(),
            artist_selection: ArtistSelection::default(),
            tracks_sort: prefs.tracks_sort,
            artists_sort: prefs.artists_sort,
            fav_albums_sort: prefs.fav_albums_sort,
            playlists_sort: prefs.playlists_sort,
            now_playing: {
                let mut np = NowPlaying::default();
                np.set_art_bytes(Some(DEFAULT_ART.to_vec()));
                np.volume = prefs.volume;
                np.shuffle = prefs.shuffle;
                np
            },
            queue_focused: false,
            queue_visible: prefs.queue_visible,
            queue_cursor: 0,
            queue_viewport: ListViewport::default(),
            last_removal: None,
            help_active: false,
            help_scroll: 0,
            update: UpdateState::default(),
            update_rx,
            checking_rx,
            update_result_rx,
            update_cmd_tx,
            update_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            update_actor: Some(UpdateActorHandles {
                check_tx: update_check_tx,
                checking_tx,
                result_tx: update_result_tx,
                cmd_rx: update_cmd_rx,
            }),
            tick: 0,
            marquee_epoch: std::time::Instant::now(),
            status: None,
            api_tx,
            player_tx,
            mpris_tx,
            lastfm_tx,
        };
        // mpv starts at its own default, so the restored level has to be pushed
        // across rather than just held in state.
        let _ = app.player_tx.send(PlayerCmd::SetVolume(prefs.volume));
        // MPRIS clients otherwise read volume 0 / shuffle off until the first
        // playback event pushes real state.
        app.push_mpris_state();

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
        self.queue_cursor = self
            .queue_viewport
            .previous_page(self.queue_cursor, self.now_playing.queue.len());
    }

    pub fn queue_page_down(&mut self) {
        self.queue_cursor = self
            .queue_viewport
            .next_page(self.queue_cursor, self.now_playing.queue.len());
    }

    /// Time since the last keypress, driving the marquee on the selected row.
    pub fn marquee_phase(&self) -> std::time::Duration {
        self.marquee_epoch.elapsed()
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if let Some((msg, _, set_at)) = &self.status {
            if set_at.elapsed() > std::time::Duration::from_secs(5) {
                tracing::debug!(
                    "Clearing status after {:.1}s: {}",
                    set_at.elapsed().as_secs_f64(),
                    msg
                );
                self.status = None;
            }
        }
    }

    pub(crate) fn set_status(&mut self, msg: String, level: StatusLevel) {
        self.status = Some((msg, level, std::time::Instant::now()));
    }

    /// Hand the update-actor channel ends to main() (once). None in tests.
    pub fn take_update_actor(&mut self) -> Option<UpdateActorHandles> {
        self.update_actor.take()
    }

    /// Record the background update-check outcome; sets the footer hint on a
    /// found release, or surfaces a check failure instead of "up to date".
    pub(crate) fn set_update_available(&mut self, result: Result<Option<String>, String>) {
        match result {
            Ok(tag) => {
                self.update.available = tag;
                self.update.check_error = None;
            }
            Err(e) => {
                self.update.available = None;
                self.update.check_error = Some(e);
            }
        }
        self.update.checking = false;
        self.update.check_done = true;
    }

    /// Open the update dialog in a specific state (Confirming for an available
    /// release, UpToDate to bare the result of a manual check).
    pub(crate) fn open_update_dialog_in_state(&mut self, status: UpdateStatus) {
        if !self.update.active {
            self.update.status = status;
            self.update.error = None;
            self.update.active = true;
        }
    }

    /// Send `Install` to the actor, clearing any cancellation left from an
    /// earlier attempt. Resetting the flag here rather than on receipt keeps a
    /// cancel raised *after* this send from being overwritten by the actor.
    pub(crate) fn send_install(&mut self) -> bool {
        self.update_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.update_cmd_tx.send(UpdateCmd::Install).is_ok()
    }

    /// Record the result of a background install.
    pub(crate) fn set_update_result(
        &mut self,
        result: Result<crate::update::UpdateOutcome, String>,
    ) {
        match result {
            Ok(crate::update::UpdateOutcome::Updated(tag)) => {
                self.update.status = UpdateStatus::Done;
                self.update.error = None;
                self.update.available = Some(tag.clone());
                self.update.checking = false;
                tracing::info!("self-update installed {tag}; restart required");
            }
            Ok(crate::update::UpdateOutcome::AlreadyCurrent) => {
                self.update.status = UpdateStatus::UpToDate;
                self.update.error = None;
                self.update.available = None;
                self.update.checking = false;
                tracing::info!("self-update: already up to date");
            }
            Err(err) => {
                tracing::warn!("self-update failed: {err}");
                self.update.status = UpdateStatus::Failed;
                self.update.error = Some(err);
                self.update.checking = false;
            }
        }
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

    pub(crate) fn rebuild_favorite_playlist_ids(&mut self) {
        self.favorite_playlist_ids = self
            .playlists
            .items
            .iter()
            .map(|p| p.uuid.clone())
            .collect();
    }

    /// Copy a share URL to the system clipboard and confirm via the status toast.
    pub(crate) fn copy_url(&mut self, url: String) {
        copy_to_clipboard(&url);
        self.set_status(format!("Copied link: {url}"), StatusLevel::Info);
    }
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
