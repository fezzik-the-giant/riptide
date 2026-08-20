// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Colours, spacing constants and small formatting helpers.

use ratatui::style::Color;

pub(super) const ACCENT: Color = Color::Cyan;

pub(super) const DIM: Color = Color::DarkGray;

pub(super) fn fmt_sample_rate(hz: u32) -> String {
    match hz {
        44100 => "44.1 kHz".into(),
        88200 => "88.2 kHz".into(),
        176400 => "176.4 kHz".into(),
        _ => {
            let khz = hz / 1000;
            format!("{khz} kHz")
        }
    }
}

pub(super) const HIGHLIGHT_BG: Color = Color::Rgb(40, 40, 55);

pub(super) const SELECT_BG: Color = Color::Rgb(30, 100, 200);

pub(super) const QUEUE_W: u16 = 26;

pub(super) const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub(super) fn spinner_char(tick: u64) -> char {
    SPINNER[(tick / 3) as usize % SPINNER.len()]
}

/// Blinking block for text inputs. Shared so every input box blinks in step.
pub(super) fn cursor_char(tick: u64) -> &'static str {
    if (tick / 30) % 2 == 0 { "█" } else { " " }
}
