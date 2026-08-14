// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Command palette and the artist-picker modal.


// ── Artist selection modal ────────────────────────────────────────────────────

pub struct ArtistSelection {
    pub active: bool,
    pub artist_names: Vec<String>,
    pub selected: usize,
    pub searching_for: Option<String>,
}

impl Default for ArtistSelection {
    fn default() -> Self {
        Self { active: false, artist_names: Vec::new(), selected: 0, searching_for: None }
    }
}

// ── Command palette ───────────────────────────────────────────────────────────

pub struct CommandState {
    pub active: bool,
    pub input: String,
    pub selected: usize,
}

impl Default for CommandState {
    fn default() -> Self {
        Self { active: false, input: String::new(), selected: 0 }
    }
}

impl CommandState {
    /// Destinations and presentation modes offered in the palette.
    pub const COMMANDS: &'static [&'static str] =
        &["home", "tracks", "artists", "albums", "playlists", "search", "art"];

    pub fn matches(&self) -> Vec<&'static str> {
        let q = self.input.to_lowercase();
        Self::COMMANDS.iter()
            .filter(|&&c| c.starts_with(q.as_str()))
            .copied()
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_state_matches_prefix() {
        let mut cmd = CommandState::default();
        cmd.input = "tra".to_string();
        let matches = cmd.matches();
        assert!(matches.contains(&"tracks"));
        assert!(!matches.contains(&"artists"));
    }

    #[test]
    fn command_state_finds_art_mode() {
        let cmd = CommandState {
            input: "ar".to_string(),
            ..CommandState::default()
        };
        assert_eq!(cmd.matches(), vec!["artists", "art"]);
    }

    #[test]
    fn command_state_empty_input_matches_all() {
        let cmd = CommandState::default();
        let matches = cmd.matches();
        assert_eq!(matches.len(), CommandState::COMMANDS.len());
    }

    #[test]
    fn command_state_no_match_returns_empty() {
        let mut cmd = CommandState::default();
        cmd.input = "zzz".to_string();
        assert!(cmd.matches().is_empty());
    }
}
