// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The settings modal's rows and the account facts shown beneath them.

// ── Rows ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Atmos,
    Shuffle,
    QueuePanel,
    Volume,
    Scrobbling,
}

impl Setting {
    pub const ALL: [Setting; 5] = [
        Setting::Atmos,
        Setting::Shuffle,
        Setting::QueuePanel,
        Setting::Volume,
        Setting::Scrobbling,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Setting::Atmos => "Dolby Atmos",
            Setting::Shuffle => "Shuffle",
            Setting::QueuePanel => "Queue panel",
            Setting::Volume => "Volume",
            Setting::Scrobbling => "Last.fm scrobbling",
        }
    }

    /// Shown under the list for the selected row. Atmos needs it most: the
    /// choice is spatial-but-lossy against stereo-but-lossless, which no
    /// on/off label conveys.
    pub fn help(self) -> &'static str {
        match self {
            Setting::Atmos => "Atmos mixes stream as E-AC-3; off plays the stereo master as FLAC",
            Setting::Shuffle => "Shuffles the current queue, keeping the playing track first",
            Setting::QueuePanel => "Show the queue alongside the library",
            Setting::Volume => "← → adjusts by 5%",
            Setting::Scrobbling => "Submit plays to Last.fm",
        }
    }
}

/// What a row currently reads.
pub enum SettingValue {
    Toggle(bool),
    Percent(u8),
    /// The setting has no value to show and cannot be changed here, with the
    /// reason in place of on/off.
    Unavailable(&'static str),
}

#[derive(Debug, Default)]
pub struct SettingsState {
    pub active: bool,
    pub selected: usize,
}

impl SettingsState {
    pub fn next(&mut self) {
        if self.selected + 1 < Setting::ALL.len() {
            self.selected += 1;
        }
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_setting(&self) -> Setting {
        Setting::ALL[self.selected.min(Setting::ALL.len() - 1)]
    }
}

// ── Account ───────────────────────────────────────────────────────────────────

/// The parts of `Config` worth showing but never worth editing in a TUI —
/// enough to paste into a bug report without opening the file and redacting
/// tokens by hand.
#[derive(Debug, Clone, Default)]
pub struct AccountInfo {
    pub country_code: String,
    pub user_id: Option<u64>,
    /// RFC 3339, as stored. Rendered as a date and time, or "expired".
    pub expires_at: Option<String>,
    pub auth_generation: u32,
}

impl AccountInfo {
    pub fn from_config(config: &crate::api::models::Config) -> Self {
        Self {
            country_code: config.country_code.clone(),
            user_id: config.user_id,
            expires_at: config.expires_at.clone(),
            auth_generation: config.auth_generation,
        }
    }

    /// Human-readable token expiry: local time, or "expired" once it has passed.
    pub fn token_expiry(&self) -> String {
        let Some(raw) = self.expires_at.as_deref() else {
            return "not signed in".to_string();
        };
        let Ok(expiry) = chrono::DateTime::parse_from_rfc3339(raw) else {
            return raw.to_string();
        };
        if expiry <= chrono::Utc::now() {
            return "expired".to_string();
        }
        expiry
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_stays_in_range() {
        let mut state = SettingsState::default();
        for _ in 0..Setting::ALL.len() + 3 {
            state.next();
        }
        assert_eq!(state.selected, Setting::ALL.len() - 1);
        assert_eq!(state.selected_setting(), Setting::Scrobbling);

        for _ in 0..Setting::ALL.len() + 3 {
            state.previous();
        }
        assert_eq!(state.selected, 0);
        assert_eq!(state.selected_setting(), Setting::Atmos);
    }

    #[test]
    fn token_expiry_reports_a_past_timestamp_as_expired() {
        let past = AccountInfo {
            expires_at: Some("2020-01-01T00:00:00+00:00".to_string()),
            ..AccountInfo::default()
        };
        assert_eq!(past.token_expiry(), "expired");

        let none = AccountInfo::default();
        assert_eq!(none.token_expiry(), "not signed in");
    }
}
