// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The Theme picker opened from Settings.

use crate::colors::catalog::{ThemeCatalog, ThemeEntry};

#[derive(Debug, Default)]
pub struct ThemePicker {
    pub active: bool,
    pub query: String,
    /// Indexes [`Self::matches`], not the catalogue.
    pub selected: usize,
    /// The theme active when the picker opened, which Esc puts back.
    pub original: String,
}

impl ThemePicker {
    pub fn matches<'a>(&self, catalog: &'a ThemeCatalog) -> Vec<&'a ThemeEntry> {
        let query = self.query.to_lowercase();
        catalog
            .entries
            .iter()
            .filter(|entry| entry.name.to_lowercase().contains(&query))
            .collect()
    }

    pub fn selected_name(&self, catalog: &ThemeCatalog) -> Option<String> {
        self.matches(catalog)
            .get(self.selected)
            .map(|entry| entry.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_filter_by_substring_ignoring_case() {
        let catalog = ThemeCatalog::builtin_only();
        let picker = ThemePicker {
            query: "MOC".into(),
            ..ThemePicker::default()
        };
        let names: Vec<&str> = picker
            .matches(&catalog)
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["Catppuccin Mocha"]);
    }

    #[test]
    fn an_empty_query_matches_everything_in_order() {
        let catalog = ThemeCatalog::builtin_only();
        let all = ThemePicker::default().matches(&catalog);
        assert_eq!(all.len(), catalog.entries.len());
        assert_eq!(all[0].name, "Default");
    }
}
