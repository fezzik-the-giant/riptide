// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Every theme the user can pick: the two modes, the built-ins, and their files.

use std::path::{Path, PathBuf};

use super::Palette;
use super::theme_file::{self, ThemeFile};

#[derive(Debug, Clone, PartialEq)]
pub enum ThemeKind {
    Default,
    Album,
    Fixed(Palette),
}

/// Which group the picker lists a theme under. A user file that replaces a
/// built-in keeps the built-in's place and group: it is a tweak of that theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    User,
}

#[derive(Debug, Clone)]
pub struct ThemeEntry {
    pub name: String,
    pub kind: ThemeKind,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    pub entries: Vec<ThemeEntry>,
    /// User files left out, each already reported in the log; the picker
    /// counts them so a theme that never appears is not a mystery.
    pub skipped: usize,
}

impl Default for ThemeCatalog {
    fn default() -> Self {
        Self::builtin_only()
    }
}

pub fn themes_dir() -> PathBuf {
    crate::api::auth::config_path().with_file_name("themes")
}

/// Names are compared folded, which is what lets the `"default"` and
/// `"album"` already saved in configs resolve to "Default" and "Album".
fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Default and Album are modes, not palettes, so no file may stand in for
/// them; and of two files with one name, the first filename keeps it.
fn admit(theme: ThemeFile, seen: &[String]) -> anyhow::Result<ThemeFile> {
    if same_name(&theme.name, "Default") || same_name(&theme.name, "Album") {
        anyhow::bail!("{:?} is reserved for a built-in mode", theme.name);
    }
    if seen.contains(&theme.name.to_lowercase()) {
        anyhow::bail!("another file already uses the name {:?}", theme.name);
    }
    Ok(theme)
}

impl ThemeCatalog {
    pub fn builtin_only() -> Self {
        let modes = [("Default", ThemeKind::Default), ("Album", ThemeKind::Album)];
        let mut entries: Vec<ThemeEntry> = modes
            .into_iter()
            .map(|(name, kind)| ThemeEntry {
                name: name.to_string(),
                kind,
                origin: Origin::Builtin,
            })
            .collect();
        entries.extend(theme_file::builtins().into_iter().map(|t| ThemeEntry {
            name: t.name,
            kind: ThemeKind::Fixed(t.palette),
            origin: Origin::Builtin,
        }));
        Self {
            entries,
            skipped: 0,
        }
    }

    /// Built-ins, then the `.json` files in `user_dir`. A file named like a
    /// built-in theme takes its place in the list, which is how someone
    /// tweaks Mocha; the rest follow alphabetically. A missing directory is
    /// the common case and not worth a word in the log.
    pub fn load(user_dir: &Path) -> Self {
        let mut catalog = Self::builtin_only();
        let Ok(dir) = std::fs::read_dir(user_dir) else {
            return catalog;
        };
        let mut files: Vec<PathBuf> = dir
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();

        let builtin_count = catalog.entries.len();
        let mut seen: Vec<String> = Vec::new();
        let mut added: Vec<ThemeEntry> = Vec::new();
        for path in files {
            let theme = match std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|json| theme_file::parse(&json))
                .and_then(|theme| admit(theme, &seen))
            {
                Ok(theme) => theme,
                Err(reason) => {
                    tracing::warn!("theme: skipping {}: {reason}", path.display());
                    catalog.skipped += 1;
                    continue;
                }
            };
            seen.push(theme.name.to_lowercase());
            let entry = ThemeEntry {
                name: theme.name,
                kind: ThemeKind::Fixed(theme.palette),
                origin: Origin::User,
            };
            match catalog.entries[..builtin_count]
                .iter()
                .position(|e| same_name(&e.name, &entry.name))
            {
                Some(at) => {
                    catalog.entries[at] = ThemeEntry {
                        origin: Origin::Builtin,
                        ..entry
                    }
                }
                None => added.push(entry),
            }
        }
        added.sort_by_key(|e| e.name.to_lowercase());
        catalog.entries.extend(added);
        catalog
    }

    pub fn find(&self, name: &str) -> Option<&ThemeEntry> {
        self.entries.iter().find(|e| same_name(&e.name, name))
    }

    /// The name one step along the list, wrapping at both ends. An unknown
    /// name steps as if from Default, which is what it draws as.
    pub fn step(&self, name: &str, forward: bool) -> &str {
        let len = self.entries.len();
        let at = self
            .entries
            .iter()
            .position(|e| same_name(&e.name, name))
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % len
        } else {
            (at + len - 1) % len
        };
        &self.entries[next].name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    const MINE: &str = include_str!("../../assets/themes/cyberpunk.json");

    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("riptide-themes-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Dir(path)
        }

        fn write(&self, file: &str, name: &str) {
            let json = MINE.replace(r#""name": "Cyberpunk""#, &format!(r#""name": "{name}""#));
            std::fs::write(self.0.join(file), json).unwrap();
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(catalog: &ThemeCatalog) -> Vec<&str> {
        catalog.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn builtin_order_is_default_album_then_the_ten() {
        assert_eq!(
            names(&ThemeCatalog::builtin_only()),
            [
                "Default",
                "Album",
                "Catppuccin Latte",
                "Catppuccin Frappé",
                "Catppuccin Macchiato",
                "Catppuccin Mocha",
                "Nord",
                "Gruvbox Dark",
                "Tokyo Night",
                "Dracula",
                "Rosé Pine",
                "Cyberpunk",
            ]
        );
    }

    #[test]
    fn find_ignores_case() {
        let catalog = ThemeCatalog::builtin_only();
        assert_eq!(
            catalog.find("CATPPUCCIN mocha").unwrap().name,
            "Catppuccin Mocha"
        );
        assert_eq!(catalog.find("album").unwrap().kind, ThemeKind::Album);
        assert!(catalog.find("nope").is_none());
    }

    #[test]
    fn user_file_replaces_a_builtin_in_place() {
        let dir = Dir::new("replace");
        dir.write("nord.json", "Nord");
        let catalog = ThemeCatalog::load(&dir.0);
        let at = names(&catalog).iter().position(|n| *n == "Nord").unwrap();
        assert_eq!(at, 6);
        let nord = &catalog.entries[at];
        assert_eq!(nord.origin, Origin::Builtin);
        let ThemeKind::Fixed(palette) = &nord.kind else {
            panic!("not fixed")
        };
        assert_eq!(palette.background, Some(Color::Rgb(0x10, 0x10, 0x14)));
        assert_eq!(catalog.entries.len(), 12);
    }

    /// The picker draws a heading wherever the origin changes, so a tweaked
    /// built-in marked as the user's would split "built in" in two.
    #[test]
    fn a_replaced_builtin_stays_in_the_builtin_group() {
        let dir = Dir::new("group");
        dir.write("nord.json", "Nord");
        dir.write("mine.json", "Mine");
        let catalog = ThemeCatalog::load(&dir.0);
        let origins: Vec<Origin> = catalog.entries.iter().map(|e| e.origin).collect();
        assert!(
            origins[..12].iter().all(|o| *o == Origin::Builtin),
            "{origins:?}"
        );
        assert_eq!(origins[12..], [Origin::User]);
    }

    #[test]
    fn user_file_named_default_or_album_is_skipped() {
        let dir = Dir::new("reserved");
        dir.write("a.json", "default");
        dir.write("b.json", "ALBUM");
        let catalog = ThemeCatalog::load(&dir.0);
        assert_eq!(catalog.skipped, 2);
        assert_eq!(catalog.find("default").unwrap().kind, ThemeKind::Default);
        assert_eq!(catalog.find("album").unwrap().kind, ThemeKind::Album);
    }

    #[test]
    fn duplicate_user_names_keep_the_first_filename() {
        let dir = Dir::new("dupes");
        dir.write("a.json", "Mine");
        let second = MINE
            .replace(r#""name": "Cyberpunk""#, r#""name": "mine""#)
            .replace("#101014", "#202020");
        std::fs::write(dir.0.join("b.json"), second).unwrap();
        let catalog = ThemeCatalog::load(&dir.0);
        assert_eq!(catalog.skipped, 1);
        let ThemeKind::Fixed(palette) = &catalog.find("mine").unwrap().kind else {
            panic!()
        };
        assert_eq!(palette.background, Some(Color::Rgb(0x10, 0x10, 0x14)));
    }

    #[test]
    fn user_themes_follow_the_builtins_alphabetically() {
        let dir = Dir::new("order");
        dir.write("z.json", "Zest");
        dir.write("a.json", "Mine");
        let catalog = ThemeCatalog::load(&dir.0);
        assert_eq!(names(&catalog)[12..], ["Mine", "Zest"]);
        assert_eq!(catalog.entries[12].origin, Origin::User);
    }

    #[test]
    fn invalid_user_file_is_counted_not_listed() {
        let dir = Dir::new("invalid");
        std::fs::write(dir.0.join("broken.json"), &MINE[..MINE.len() / 2]).unwrap();
        std::fs::write(dir.0.join("notes.txt"), "not a theme").unwrap();
        let catalog = ThemeCatalog::load(&dir.0);
        assert_eq!(catalog.skipped, 1);
        assert_eq!(catalog.entries.len(), 12);
    }

    #[test]
    fn missing_dir_is_builtins_only() {
        let catalog = ThemeCatalog::load(&std::env::temp_dir().join("riptide-no-themes-here"));
        assert_eq!(names(&catalog), names(&ThemeCatalog::builtin_only()));
        assert_eq!(catalog.skipped, 0);
    }

    #[test]
    fn step_wraps_both_ways() {
        let catalog = ThemeCatalog::builtin_only();
        assert_eq!(catalog.step("Default", false), "Cyberpunk");
        assert_eq!(catalog.step("cyberpunk", true), "Default");
        assert_eq!(catalog.step("Default", true), "Album");
        assert_eq!(catalog.step("gone", true), "Album");
    }
}
