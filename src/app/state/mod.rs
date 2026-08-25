// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use crate::api::models::*;
use crate::playlist::PlaylistDetail;
use std::cell::Cell;
use std::collections::HashMap;

mod detail;
mod keybinds;
mod list;
mod now_playing;
mod palette;
mod sort;

pub use detail::*;
pub use keybinds::*;
pub use list::*;
pub use now_playing::*;
pub use palette::*;
pub use sort::*;

// ── Tab ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Favorites,
    Artists,
    Albums,
    Playlists,
    Search,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Home,
        Tab::Favorites,
        Tab::Artists,
        Tab::Albums,
        Tab::Playlists,
        Tab::Search,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Favorites => "Tracks",
            Tab::Artists => "Artists",
            Tab::Albums => "Albums",
            Tab::Playlists => "Playlists",
            Tab::Search => "Search",
        }
    }
}

// ── View stack ────────────────────────────────────────────────────────────────

pub enum View {
    ArtistDetail(ArtistDetail),
    PlaylistDetail(PlaylistDetail),
    AlbumDetail(AlbumDetail),
}

// ── Undo ──────────────────────────────────────────────────────────────────────

/// The last thing removed from the library, kept so `u` can put it back.
///
/// Deliberately a single slot that outlives its toast: the point is to rescue an
/// accidental keypress, and the user may not look up until well after the message
/// has gone. Taking the slot on undo prevents a second `u` re-adding something the
/// user has since removed on purpose.
pub enum Removal {
    Track(Box<Track>),
    Artist(Box<Artist>),
    Album(Box<Album>),
    Playlist(Box<Playlist>),
}

impl Removal {
    pub fn title(&self) -> &str {
        match self {
            Removal::Track(t) => &t.title,
            Removal::Artist(a) => &a.name,
            Removal::Album(a) => &a.title,
            Removal::Playlist(p) => &p.title,
        }
    }
}

// ── Status level ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Error,
}

// ── Self-update ───────────────────────────────────────────────────────────────

/// Status of a TUI-triggered self-update (the modal dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Release is newer; waiting for the user to press Enter to confirm.
    Confirming,
    /// Downloading, verifying, and installing.
    Working,
    /// Installed successfully; restart required.
    Done,
    /// Update failed; the binary is untouched.
    Failed,
    /// Re-check found nothing newer. Mutually reachable with an earlier-found
    /// `available` (release retracted between check and install).
    UpToDate,
}

/// What the update actor has learned so far, sent as it learns it. The actor
/// cannot report either until it has resolved `install_method`, which is
/// deferred off the first frame and can take seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePhase {
    /// This install is package-managed; no check will ever run.
    NotSelfUpdatable,
    /// An availability check is in flight.
    Checking,
}

/// Command sent to the self-update actor thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCmd {
    /// (Re)run the availability check.
    Check,
    /// Download and install the newer release.
    Install,
}

pub struct UpdateState {
    /// Newer tag detected (e.g. "v0.14.0"). Drives the footer hint.
    pub available: Option<String>,
    /// True while the background check (3 s after startup) has not yet completed.
    pub checking: bool,
    /// True once the first background check produced any outcome (found,
    /// none, or failed). Prevents pressing `U` from reporting "up to date"
    /// before a check has actually happened for this install type.
    pub check_done: bool,
    /// Update dialog open.
    pub active: bool,
    pub status: UpdateStatus,
    /// Error text when status == Failed.
    pub error: Option<String>,
    /// Last update-check failure (offline, rate-limited, …), if the most
    /// recent check errored. Lets `U` say "check failed" instead of
    /// misreporting "up to date".
    pub check_error: Option<String>,
    /// Whether self-update applies to this install, once the actor has
    /// resolved it. `None` until then — pressing `U` in that window must not
    /// claim either answer.
    pub self_updatable: Option<bool>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            available: None,
            // `checking` starts false so non-self-update installs never show
            // a stale "Checking…" state; main() flips it to true just before
            // a background check is actually spawned (Script installs only).
            checking: false,
            check_done: false,
            active: false,
            status: UpdateStatus::Confirming,
            error: None,
            check_error: None,
            self_updatable: None,
        }
    }
}
