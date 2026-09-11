// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Input for the search box.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

pub(super) fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        // Esc dismisses the box, matching Esc everywhere else in the app. It
        // used to jump to the previous tab, which was the only way out.
        KeyCode::Esc => {
            app.search.modal_open = false;
        }
        // Tab must keep working while the box is open — it is otherwise
        // swallowed by the catch-all below and the user is stuck.
        KeyCode::Tab => {
            app.search.modal_open = false;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_tab();
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            app.search.modal_open = false;
            app.prev_tab();
        }
        KeyCode::Enter => {
            app.search.modal_open = false;
            app.submit_search();
        }
        KeyCode::Backspace => {
            app.search.query.pop();
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
        }
        _ => {}
    }
}
