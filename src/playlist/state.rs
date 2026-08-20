// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use crate::api::models::{Playlist, Track};
use crate::app::StatefulList;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistDetailFocus {
    Tracks,
    Description,
}

pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub tracks: StatefulList<Track>,
    pub focus: PlaylistDetailFocus,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub description_scroll: u16,
}
