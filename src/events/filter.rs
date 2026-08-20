// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Input for the list filter box.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

pub(super) fn handle_filter_input(app: &mut App, key: KeyEvent) {
    match key.code {
        // Esc backs all the way out: close the box and restore the full list.
        // Enter keeps the query so the narrowed list can be navigated.
        KeyCode::Esc => {
            app.filter_active = false;
            app.clear_active_filter();
        }
        KeyCode::Enter => {
            app.filter_active = false;
        }
        // Tab must keep switching tabs or the box is a trap, matching the
        // search box. The query stays applied to the tab being left.
        KeyCode::Tab => {
            app.filter_active = false;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_tab();
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            app.filter_active = false;
            app.prev_tab();
        }
        KeyCode::Backspace => app.edit_active_filter(|f| {
            f.pop();
        }),
        KeyCode::Char(c) => app.edit_active_filter(|f| f.push(c)),
        _ => {}
    }
}
