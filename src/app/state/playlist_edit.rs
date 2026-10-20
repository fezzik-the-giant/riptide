// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Overlay state for filing a track into a playlist and naming one.

use crate::api::models::{Playlist, Track};

/// Row 0 is always "+ New playlist…"; row `i + 1` is `targets[i]`. The targets
/// are snapshotted on open so a library refresh cannot shift the row under the
/// cursor.
#[derive(Default)]
pub struct PlaylistPicker {
    pub active: bool,
    pub track: Option<Track>,
    pub targets: Vec<Playlist>,
    pub selected: usize,
}

impl PlaylistPicker {
    pub fn rows(&self) -> usize {
        self.targets.len() + 1
    }
}

pub enum NameTarget {
    /// `then_add` is set when the prompt was opened from the picker, so the
    /// new playlist receives the track it was made for.
    New {
        then_add: Option<Track>,
    },
    Rename(Playlist),
}

impl Default for NameTarget {
    fn default() -> Self {
        NameTarget::New { then_add: None }
    }
}

/// Positions are kept as entry ids, never indices: a remove or add response
/// landing mid-move shifts every index but no entry.
pub struct PlaylistMove {
    pub item_id: String,
    /// The entry it started in front of; `None` when it started last.
    pub origin_next: Option<String>,
}

#[derive(Default)]
pub struct NamePrompt {
    pub active: bool,
    pub input: String,
    pub target: NameTarget,
}
