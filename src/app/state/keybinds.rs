// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The keybind reference shown in the help modal.

// ── Keybinds ──────────────────────────────────────────────────────────────────

pub struct Keybind {
    pub key: &'static str,
    pub action: &'static str,
}

pub struct KeybindGroup {
    pub title: &'static str,
    pub binds: &'static [Keybind],
}

/// Filtered view of a keybind group for help search.
pub struct FilteredKeybindGroup {
    pub title: &'static str,
    pub binds: Vec<&'static Keybind>,
}

impl KeybindGroup {
    /// All groups in display order.
    pub fn all() -> Vec<Self> {
        vec![
            Self::global(),
            Self::navigation(),
            Self::queue(),
            Self::search(),
            Self::command(),
        ]
    }

    /// Case-insensitive substring filter on title/key/action. Empty query = all.
    pub fn filtered_groups(query: &str) -> Vec<FilteredKeybindGroup> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Self::all()
                .into_iter()
                .map(|g| FilteredKeybindGroup {
                    title: g.title,
                    binds: g.binds.iter().collect(),
                })
                .collect();
        }
        let mut out = Vec::new();
        for g in Self::all() {
            let title_lower = g.title.to_lowercase();
            if title_lower.contains(&q) {
                out.push(FilteredKeybindGroup {
                    title: g.title,
                    binds: g.binds.iter().collect(),
                });
                continue;
            }
            let binds: Vec<&'static Keybind> = g
                .binds
                .iter()
                .filter(|b| {
                    b.key.to_lowercase().contains(&q) || b.action.to_lowercase().contains(&q)
                })
                .collect();
            if !binds.is_empty() {
                out.push(FilteredKeybindGroup {
                    title: g.title,
                    binds,
                });
            }
        }
        out
    }

    /// Total lines needed to display filtered groups (for scroll bounds).
    pub fn total_help_lines_filtered(query: &str) -> u16 {
        let groups = Self::filtered_groups(query);
        if groups.is_empty() {
            return 1;
        }
        let mut total = 0u16;
        for g in groups {
            total += 1; // group header
            total += g.binds.len() as u16; // keybinds
            total += 1; // blank line
        }
        total
    }

    /// Count of binds matching the query across all groups.
    #[allow(dead_code)]
    pub fn filtered_bind_count(query: &str) -> usize {
        Self::filtered_groups(query)
            .iter()
            .map(|g| g.binds.len())
            .sum()
    }

    /// Total bind count across all groups.
    pub fn total_bind_count() -> usize {
        Self::all().iter().map(|g| g.binds.len()).sum()
    }

    /// Calculate total lines needed to display all keybind groups (for scrolling bounds)
    #[allow(dead_code)]
    pub fn total_help_lines() -> u16 {
        Self::total_help_lines_filtered("")
    }

    pub fn global() -> Self {
        KeybindGroup {
            title: "Global",
            binds: &[
                Keybind {
                    key: "?",
                    action: "Show this help",
                },
                Keybind {
                    key: "q",
                    action: "Quit",
                },
                Keybind {
                    key: ":",
                    action: "Command palette",
                },
                Keybind {
                    key: "/",
                    action: "Filter list",
                },
                Keybind {
                    key: "Tab",
                    action: "Next tab",
                },
                Keybind {
                    key: "Shift+Tab",
                    action: "Previous tab",
                },
                Keybind {
                    key: "Shift+A",
                    action: "Toggle fullscreen art",
                },
                Keybind {
                    key: "Space",
                    action: "Play/Pause",
                },
                Keybind {
                    key: "n",
                    action: "Next track",
                },
                Keybind {
                    key: "p",
                    action: "Previous track",
                },
                Keybind {
                    key: "z",
                    action: "Toggle shuffle",
                },
                Keybind {
                    key: "t",
                    action: "Show/hide queue",
                },
                Keybind {
                    key: "U",
                    action: "Update to latest release",
                },
                Keybind {
                    key: "+ or =",
                    action: "Volume Up",
                },
                Keybind {
                    key: "-",
                    action: "Volume Down",
                },
                Keybind {
                    key: "Esc",
                    action: "Back/Go up",
                },
            ],
        }
    }

    pub fn navigation() -> Self {
        KeybindGroup {
            title: "Navigation",
            binds: &[
                Keybind {
                    key: "↑ or k",
                    action: "Up",
                },
                Keybind {
                    key: "↓ or j",
                    action: "Down",
                },
                Keybind {
                    key: "PgUp/PgDn",
                    action: "Move one page",
                },
                Keybind {
                    key: "Enter",
                    action: "Select/Open",
                },
                Keybind {
                    key: "a",
                    action: "Add to queue",
                },
                Keybind {
                    key: "f",
                    action: "Favorite/follow/save",
                },
                Keybind {
                    key: "d",
                    action: "Remove from library",
                },
                Keybind {
                    key: "u",
                    action: "Undo the last removal",
                },
                Keybind {
                    key: "g",
                    action: "Go to artist",
                },
                Keybind {
                    key: "s",
                    action: "Sort",
                },
                Keybind {
                    key: "r",
                    action: "Start radio",
                },
                Keybind {
                    key: "c",
                    action: "Copy share link (song)",
                },
                Keybind {
                    key: "C",
                    action: "Copy share link (album/playlist)",
                },
                Keybind {
                    key: "→ or l",
                    action: "Focus queue",
                },
            ],
        }
    }

    pub fn queue() -> Self {
        KeybindGroup {
            title: "Queue",
            binds: &[
                Keybind {
                    key: "↑ or k",
                    action: "Up",
                },
                Keybind {
                    key: "↓ or j",
                    action: "Down",
                },
                Keybind {
                    key: "d",
                    action: "Remove track",
                },
                Keybind {
                    key: "c",
                    action: "Copy share link (song)",
                },
                Keybind {
                    key: "C",
                    action: "Copy share link (album)",
                },
                Keybind {
                    key: "Enter",
                    action: "Play track",
                },
                Keybind {
                    key: "t",
                    action: "Show/hide queue",
                },
                Keybind {
                    key: "Esc",
                    action: "Close queue",
                },
            ],
        }
    }

    pub fn search() -> Self {
        KeybindGroup {
            title: "Search",
            binds: &[
                Keybind {
                    key: "↑ or k",
                    action: "Up",
                },
                Keybind {
                    key: "↓ or j",
                    action: "Down",
                },
                Keybind {
                    key: "Tab",
                    action: "Next pane",
                },
                Keybind {
                    key: "Shift+Tab",
                    action: "Prev pane",
                },
                Keybind {
                    key: "Enter",
                    action: "Select/Open",
                },
                Keybind {
                    key: "Esc",
                    action: "Close search",
                },
            ],
        }
    }

    pub fn command() -> Self {
        KeybindGroup {
            title: "Command",
            binds: &[
                Keybind {
                    key: "↑",
                    action: "Up",
                },
                Keybind {
                    key: "↓",
                    action: "Down",
                },
                Keybind {
                    key: "Enter",
                    action: "Execute",
                },
                Keybind {
                    key: "Esc",
                    action: "Close",
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Help filtering ────────────────────────────────────────────────────

    #[test]
    fn help_filter_empty_returns_all() {
        let all = KeybindGroup::filtered_groups("");
        assert_eq!(all.len(), 5);
        assert_eq!(
            KeybindGroup::filtered_bind_count(""),
            KeybindGroup::total_bind_count()
        );
    }

    #[test]
    fn help_filter_matches_action_case_insensitive() {
        let groups = KeybindGroup::filtered_groups("shuffle");
        let count: usize = groups.iter().map(|g| g.binds.len()).sum();
        assert_eq!(count, 1);
        assert!(groups.iter().any(|g| {
            g.binds
                .iter()
                .any(|b| b.action.to_lowercase().contains("shuffle"))
        }));
        // Case insensitive
        let groups2 = KeybindGroup::filtered_groups("SHUFFLE");
        assert_eq!(groups2.iter().map(|g| g.binds.len()).sum::<usize>(), count);
    }

    #[test]
    fn help_filter_matches_key() {
        let groups = KeybindGroup::filtered_groups("Tab");
        // Should match Global Tab, Search Tab, etc.
        assert!(!groups.is_empty());
        let has_tab = groups
            .iter()
            .flat_map(|g| &g.binds)
            .any(|b| b.key.contains("Tab"));
        assert!(has_tab);
    }

    #[test]
    fn help_filter_matches_group_title() {
        let groups = KeybindGroup::filtered_groups("global");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].title, "Global");
        assert_eq!(groups[0].binds.len(), KeybindGroup::global().binds.len());
    }

    #[test]
    fn help_filter_no_match_returns_empty() {
        let groups = KeybindGroup::filtered_groups("xyznotfound");
        assert!(groups.is_empty());
        assert_eq!(KeybindGroup::filtered_bind_count("xyznotfound"), 0);
        assert_eq!(KeybindGroup::total_help_lines_filtered("xyznotfound"), 1);
    }

    #[test]
    fn help_filter_trims_whitespace() {
        let a = KeybindGroup::filtered_groups("  shuffle  ");
        let b = KeybindGroup::filtered_groups("shuffle");
        assert_eq!(a.len(), b.len());
        assert_eq!(
            KeybindGroup::filtered_bind_count("  shuffle  "),
            KeybindGroup::filtered_bind_count("shuffle")
        );
    }
}
