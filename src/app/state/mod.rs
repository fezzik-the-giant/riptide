// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::models::*;
use crate::playlist::PlaylistDetail;
use std::cell::Cell;

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

// ── Status level ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Error,
}
