// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

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

impl KeybindGroup {
    /// Calculate total lines needed to display all keybind groups (for scrolling bounds)
    pub fn total_help_lines() -> u16 {
        let groups = vec![
            Self::global(),
            Self::navigation(),
            Self::queue(),
            Self::search(),
            Self::command(),
        ];
        let mut total = 0u16;
        for group in groups {
            total += 1; // group header
            total += group.binds.len() as u16; // keybinds
            total += 1; // blank line
        }
        total
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
                    key: "↑",
                    action: "Up",
                },
                Keybind {
                    key: "↓",
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
                    key: "→",
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
                    key: "↑",
                    action: "Up",
                },
                Keybind {
                    key: "↓",
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
                    key: "↑",
                    action: "Up",
                },
                Keybind {
                    key: "↓",
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
