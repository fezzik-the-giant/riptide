// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! State for the pushed detail views and the Home tab sections.

use super::*;

// ── Artist detail ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistDetailFocus {
    Tracks,
    Albums,
    EPs,
    Singles,
    Bio,
}

pub struct ArtistDetail {
    pub artist: Artist,
    pub tracks: StatefulList<Track>,
    pub albums: StatefulList<Album>,
    pub eps: StatefulList<Album>,
    pub singles: StatefulList<Album>,
    pub focus: ArtistDetailFocus,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub bio: Option<String>,
    pub bio_loading: bool,
    pub bio_scroll: u16,
}

// ── Playlist detail ───────────────────────────────────────────────────────────

// ── Home tab ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSectionFocus {
    NewReleases,
    DailyMixes,
    DiscoveryMixes,
}

impl Default for HomeSectionFocus {
    fn default() -> Self {
        Self::NewReleases
    }
}

pub struct HomeSection<T> {
    pub items: Vec<T>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T> Default for HomeSection<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            loading: false,
            error: None,
        }
    }
}

impl<T> HomeSection<T> {
    pub fn next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.items.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.items.get(self.selected)
    }
}

// ── Album detail / art payload ────────────────────────────────────────────────

pub struct AlbumDetail {
    pub album: Album,
    pub tracks: StatefulList<Track>,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
}
