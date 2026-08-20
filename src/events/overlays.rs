// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Input for the modal overlays: command palette, sort picker, help, artist picker.

use crossterm::event::{KeyCode, KeyEvent};

use super::*;
use crate::app::{App, SortPalette, Tab};

pub(super) fn handle_command_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.command.active = false;
        }
        KeyCode::Enter => {
            let matches = app.command.matches();
            let cmd = matches
                .get(app.command.selected)
                .or_else(|| matches.first())
                .copied();
            if let Some(cmd) = cmd {
                execute_command(app, cmd);
            } else {
                app.command.active = false;
            }
        }
        KeyCode::Tab => {
            // Accept ghost-text completion.
            if let Some(&first) = app.command.matches().first() {
                app.command.input = first.to_string();
                app.command.selected = 0;
            }
        }
        KeyCode::Up => {
            if app.command.selected > 0 {
                app.command.selected -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.command.matches().len();
            if app.command.selected + 1 < len {
                app.command.selected += 1;
            }
        }
        KeyCode::Backspace => {
            app.command.input.pop();
            app.command.selected = 0;
        }
        KeyCode::Char(c) => {
            app.command.input.push(c);
            app.command.selected = 0;
        }
        _ => {}
    }
}

pub(super) fn execute_command(app: &mut App, cmd: &str) {
    app.command.active = false;
    app.command.input.clear();
    let cleanup = |app: &App| {
        if leaving_album(app) {
            kitty_delete_album_art();
        }
        if leaving_artist(app) {
            kitty_delete_artist_art();
        }
    };
    match cmd {
        "home" => {
            cleanup(app);
            app.set_tab(Tab::Home);
        }
        "tracks" => {
            cleanup(app);
            app.set_tab(Tab::Favorites);
        }
        "artists" => {
            cleanup(app);
            app.set_tab(Tab::Artists);
        }
        "albums" => {
            cleanup(app);
            app.set_tab(Tab::Albums);
        }
        "playlists" => {
            cleanup(app);
            app.set_tab(Tab::Playlists);
        }
        "search" => {
            cleanup(app);
            app.set_tab(Tab::Search);
            app.search.modal_open = true;
            app.search.query.clear();
        }
        "art" => {
            app.enter_art_fullscreen();
        }
        _ => {}
    }
}

pub(super) fn handle_sort_palette_input(app: &mut App, key: KeyEvent) {
    let options = SortPalette::get_options(app.current_tab);
    let count = options.len();
    match key.code {
        KeyCode::Esc => {
            app.sort_palette.active = false;
        }
        KeyCode::Up => {
            if app.sort_palette.selected > 0 {
                app.sort_palette.selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.sort_palette.selected + 1 < count {
                app.sort_palette.selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some((_, field)) = options.get(app.sort_palette.selected) {
                app.apply_sort(*field);
            }
        }
        _ => {}
    }
}

pub(super) fn handle_help_input(app: &mut App, key: KeyEvent) {
    use crate::app::KeybindGroup;

    let max_scroll = {
        let total_lines = KeybindGroup::total_help_lines() as i16;
        let visible_lines = 22i16; // Modal inner height (24 - 2 for borders) in render_help_modal
        (total_lines - visible_lines).max(0) as u16
    };

    match key.code {
        KeyCode::Esc => {
            app.help_active = false;
            app.help_scroll = 0;
        }
        KeyCode::Up => {
            app.help_scroll = app.help_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            app.help_scroll = (app.help_scroll + 1).min(max_scroll);
        }
        KeyCode::PageUp => {
            app.help_scroll = app.help_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.help_scroll = (app.help_scroll + 10).min(max_scroll);
        }
        _ => {}
    }
}

pub(super) fn handle_artist_selection_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.artist_selection.active = false;
        }
        KeyCode::Up => {
            if app.artist_selection.selected > 0 {
                app.artist_selection.selected -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.artist_selection.artist_names.len();
            if app.artist_selection.selected + 1 < len {
                app.artist_selection.selected += 1;
            }
        }
        KeyCode::Enter => {
            app.open_selected_artist_from_selection();
        }
        _ => {}
    }
}
