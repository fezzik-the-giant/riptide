// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use crate::api::models::*;
use std::cell::Cell;

#[derive(Default)]
pub(super) struct ListViewport {
    offset: Cell<usize>,
    capacity: Cell<usize>,
}

impl ListViewport {
    pub fn offset(&self, selected: usize, len: usize, height: usize) -> usize {
        self.capacity.set(height);
        if height == 0 || len <= height {
            self.offset.set(0);
            return 0;
        }

        let selected = selected.min(len - 1);
        let mut offset = self.offset.get().min(len - height);
        if selected < offset {
            offset = selected;
        } else if selected >= offset + height {
            offset = selected + 1 - height;
        }
        offset = offset.min(len - height);
        self.offset.set(offset);
        offset
    }

    pub fn reset(&self) {
        self.offset.set(0);
    }

    pub(super) fn page_size(&self) -> usize {
        self.capacity.get().saturating_sub(1).max(1)
    }

    pub(super) fn previous_page(&self, selected: usize, len: usize) -> usize {
        selected
            .saturating_sub(self.page_size())
            .min(len.saturating_sub(1))
    }

    pub(super) fn next_page(&self, selected: usize, len: usize) -> usize {
        (selected + self.page_size()).min(len.saturating_sub(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchPane {
    #[default]
    Tracks,
    Albums,
    Artists,
    Playlists,
}

pub struct SearchState {
    pub query: String,
    pub modal_open: bool,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub pane: SearchPane,
    pub track_sel: usize,
    pub album_sel: usize,
    pub artist_sel: usize,
    pub playlist_sel: usize,
    pub tracks_next_url: Option<String>,
    pub albums_next_url: Option<String>,
    pub artists_next_url: Option<String>,
    pub playlists_next_url: Option<String>,
    pub tracks_awaiting_page2: bool,
    pub albums_awaiting_page2: bool,
    pub artists_awaiting_page2: bool,
    pub playlists_awaiting_page2: bool,
    track_viewport: ListViewport,
    album_viewport: ListViewport,
    artist_viewport: ListViewport,
    playlist_viewport: ListViewport,
    pub loading: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            modal_open: false,
            tracks: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            playlists: Vec::new(),
            pane: SearchPane::Tracks,
            track_sel: 0,
            album_sel: 0,
            artist_sel: 0,
            playlist_sel: 0,
            tracks_next_url: None,
            albums_next_url: None,
            artists_next_url: None,
            playlists_next_url: None,
            tracks_awaiting_page2: false,
            albums_awaiting_page2: false,
            artists_awaiting_page2: false,
            playlists_awaiting_page2: false,
            track_viewport: ListViewport::default(),
            album_viewport: ListViewport::default(),
            artist_viewport: ListViewport::default(),
            playlist_viewport: ListViewport::default(),
            loading: false,
        }
    }
}

impl SearchState {
    /// True when any pane has something to browse.
    pub fn has_results(&self) -> bool {
        !self.tracks.is_empty()
            || !self.albums.is_empty()
            || !self.artists.is_empty()
            || !self.playlists.is_empty()
    }

    /// True once every pane has drained its last page.
    pub fn all_settled(&self) -> bool {
        !self.tracks_awaiting_page2
            && !self.albums_awaiting_page2
            && !self.artists_awaiting_page2
            && !self.playlists_awaiting_page2
    }

    pub fn track_scroll_offset(&self, height: usize) -> usize {
        self.track_viewport
            .offset(self.track_sel, self.tracks.len(), height)
    }

    pub fn album_scroll_offset(&self, height: usize) -> usize {
        self.album_viewport
            .offset(self.album_sel, self.albums.len(), height)
    }

    pub fn artist_scroll_offset(&self, height: usize) -> usize {
        self.artist_viewport
            .offset(self.artist_sel, self.artists.len(), height)
    }

    pub fn playlist_scroll_offset(&self, height: usize) -> usize {
        self.playlist_viewport
            .offset(self.playlist_sel, self.playlists.len(), height)
    }

    pub fn reset_viewports(&self) {
        self.track_viewport.reset();
        self.album_viewport.reset();
        self.artist_viewport.reset();
        self.playlist_viewport.reset();
    }

    pub fn total_results(&self) -> usize {
        self.tracks.len() + self.albums.len() + self.artists.len() + self.playlists.len()
    }

    pub fn pane_next(&mut self) {
        let len = self.pane_len();
        if len == 0 {
            return;
        }
        match self.pane {
            SearchPane::Tracks => self.track_sel = (self.track_sel + 1).min(len - 1),
            SearchPane::Albums => self.album_sel = (self.album_sel + 1).min(len - 1),
            SearchPane::Artists => self.artist_sel = (self.artist_sel + 1).min(len - 1),
            SearchPane::Playlists => self.playlist_sel = (self.playlist_sel + 1).min(len - 1),
        }
    }

    pub fn pane_prev(&mut self) {
        match self.pane {
            SearchPane::Tracks => {
                if self.track_sel > 0 {
                    self.track_sel -= 1;
                }
            }
            SearchPane::Albums => {
                if self.album_sel > 0 {
                    self.album_sel -= 1;
                }
            }
            SearchPane::Artists => {
                if self.artist_sel > 0 {
                    self.artist_sel -= 1;
                }
            }
            SearchPane::Playlists => {
                if self.playlist_sel > 0 {
                    self.playlist_sel -= 1;
                }
            }
        }
    }

    pub fn pane_page_up(&mut self) {
        match self.pane {
            SearchPane::Tracks => {
                self.track_sel = self
                    .track_viewport
                    .previous_page(self.track_sel, self.tracks.len())
            }
            SearchPane::Albums => {
                self.album_sel = self
                    .album_viewport
                    .previous_page(self.album_sel, self.albums.len())
            }
            SearchPane::Artists => {
                self.artist_sel = self
                    .artist_viewport
                    .previous_page(self.artist_sel, self.artists.len())
            }
            SearchPane::Playlists => {
                self.playlist_sel = self
                    .playlist_viewport
                    .previous_page(self.playlist_sel, self.playlists.len())
            }
        }
    }

    pub fn pane_page_down(&mut self) {
        match self.pane {
            SearchPane::Tracks => {
                self.track_sel = self
                    .track_viewport
                    .next_page(self.track_sel, self.tracks.len())
            }
            SearchPane::Albums => {
                self.album_sel = self
                    .album_viewport
                    .next_page(self.album_sel, self.albums.len())
            }
            SearchPane::Artists => {
                self.artist_sel = self
                    .artist_viewport
                    .next_page(self.artist_sel, self.artists.len())
            }
            SearchPane::Playlists => {
                self.playlist_sel = self
                    .playlist_viewport
                    .next_page(self.playlist_sel, self.playlists.len())
            }
        }
    }

    pub fn pane_len(&self) -> usize {
        match self.pane {
            SearchPane::Tracks => self.tracks.len(),
            SearchPane::Albums => self.albums.len(),
            SearchPane::Artists => self.artists.len(),
            SearchPane::Playlists => self.playlists.len(),
        }
    }

    pub fn next_pane(&mut self) {
        self.pane = match self.pane {
            SearchPane::Tracks => SearchPane::Albums,
            SearchPane::Albums => SearchPane::Artists,
            SearchPane::Artists => SearchPane::Playlists,
            SearchPane::Playlists => SearchPane::Tracks,
        };
    }

    pub fn prev_pane(&mut self) {
        self.pane = match self.pane {
            SearchPane::Tracks => SearchPane::Playlists,
            SearchPane::Albums => SearchPane::Tracks,
            SearchPane::Artists => SearchPane::Albums,
            SearchPane::Playlists => SearchPane::Artists,
        };
    }
}
