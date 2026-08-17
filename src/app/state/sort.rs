// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Sort fields, the sort palette, and the preferences persisted to config.

use super::*;

// ── Sort palette ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SortField {
    #[default]
    Alphabetical,
    LastAdded,
    ByArtist,
}

impl SortField {
    /// Short label for the sort indicator shown in list titles.
    pub fn label(self) -> &'static str {
        match self {
            SortField::Alphabetical => "A-Z",
            SortField::LastAdded => "Recent",
            SortField::ByArtist => "Artist",
        }
    }
}

// ── Persisted preferences ────────────────────────────────────────────────────

/// User choices that survive restarts, stored inside `Config`.
///
/// Every field carries a serde default so configs written by older versions
/// still load — an absent key falls back to the value the app used before this
/// existed, rather than failing the whole parse and logging the user out.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub tracks_sort: Option<SortField>,
    #[serde(default)]
    pub artists_sort: Option<SortField>,
    #[serde(default)]
    pub fav_albums_sort: Option<SortField>,
    #[serde(default)]
    pub playlists_sort: Option<SortField>,
    #[serde(default = "default_volume")]
    pub volume: u8,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default = "default_queue_visible")]
    pub queue_visible: bool,
}

fn default_volume() -> u8 {
    100
}
fn default_queue_visible() -> bool {
    true
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            tracks_sort: None,
            artists_sort: None,
            fav_albums_sort: None,
            playlists_sort: None,
            volume: default_volume(),
            shuffle: false,
            queue_visible: default_queue_visible(),
        }
    }
}

pub struct SortPalette {
    pub active: bool,
    pub selected: usize,
}

impl Default for SortPalette {
    fn default() -> Self {
        Self {
            active: false,
            selected: 0,
        }
    }
}

impl SortPalette {
    pub fn get_options(current_tab: Tab) -> &'static [(&'static str, SortField)] {
        match current_tab {
            Tab::Home => &[],
            Tab::Artists | Tab::Playlists => &[
                ("Alphabetical", SortField::Alphabetical),
                ("Last Added", SortField::LastAdded),
            ],
            Tab::Albums | Tab::Favorites => &[
                ("Alphabetical", SortField::Alphabetical),
                ("By Artist", SortField::ByArtist),
                ("Last Added", SortField::LastAdded),
            ],
            Tab::Search => &[],
        }
    }
}
