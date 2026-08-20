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
