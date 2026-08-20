// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Terminal rendering.
//!
//! [`draw`] lays out the frame — tab strip, content, queue, now-playing bar and
//! footer — and hands each region to the module that owns it. Overlays are
//! painted last so they sit above everything else.

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::api::models::Track;
use crate::app::{App, ArtistDetailFocus, KeybindGroup, StatusLevel, Tab, View};
use crate::playlist::PlaylistDetailFocus;
use crate::search::SearchPane;

mod album_detail;
mod art;
mod artist_detail;
mod carousel;
mod footer;
mod home;
mod image;
mod lists;
mod now_playing;
mod overlays;
mod playlist_detail;
mod queue;
mod row;
mod search;
mod tabs;
mod theme;

use album_detail::*;
use art::*;
use artist_detail::*;
use carousel::*;
use footer::*;
use home::*;
use image::*;
use lists::*;
use now_playing::*;
use overlays::*;
use playlist_detail::*;
use queue::*;
use row::*;
use search::*;
use tabs::*;
use theme::*;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let overlays = Overlays::none()
        .with(Overlays::COMMAND, app.command.active)
        .with(Overlays::SORT, app.sort_palette.active)
        .with(Overlays::ARTIST_PICKER, app.artist_selection.active)
        .with(Overlays::HELP, app.help_active)
        .with(Overlays::STATUS, app.status.is_some());
    prepare_image_frame(app.art_fullscreen, overlays);

    if app.art_fullscreen {
        render_art_view(f, app, area);
        render_overlays(f, app, area);
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3),  // tab bar (boxed active tab needs 3 rows)
        Constraint::Min(0),     // content + queue
        Constraint::Length(16), // now-playing bar (art stacked above track info)
        Constraint::Length(1),  // help hint
    ])
    .split(area);

    render_tab_bar(f, app, rows[0]);

    if app.queue_visible {
        let cols =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(QUEUE_W)]).split(rows[1]);
        render_content(f, app, cols[0]);
        render_queue(f, app, cols[1]);
    } else {
        render_content(f, app, rows[1]);
    }

    render_now_playing(f, app, rows[2]);
    render_footer(f, app, rows[3]);

    render_overlays(f, app, area);
}

fn render_overlays(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.command.active {
        render_command_overlay(f, app, area);
    }

    if app.sort_palette.active {
        render_sort_overlay(f, app, area);
    }

    if app.artist_selection.active {
        render_artist_selection_modal(f, app, area);
    }

    if app.help_active {
        render_help_modal(f, app, area);
    }

    render_toast(f, app, area);
}

// ── Tab bar ───────────────────────────────────────────────────────────────────

// ── Queue panel ───────────────────────────────────────────────────────────────

// ── Command overlay ───────────────────────────────────────────────────────────

// ── Sort overlay ──────────────────────────────────────────────────────────────

// ── Artist selection modal ────────────────────────────────────────────────────

// ── Help modal ────────────────────────────────────────────────────────────────

// ── Main content area ─────────────────────────────────────────────────────────

// ── Home tab ──────────────────────────────────────────────────────────────────

// ── Artists list ──────────────────────────────────────────────────────────────

// ── Saved albums list ─────────────────────────────────────────────────────────

// ── Artist detail (tracks + albums split) ─────────────────────────────────────

// ── Playlists ─────────────────────────────────────────────────────────────────

// ── Generic track list ────────────────────────────────────────────────────────

// ── Album detail ──────────────────────────────────────────────────────────────

// ── Search results (three-pane layout) ───────────────────────────────────────

// ── Now playing bar ───────────────────────────────────────────────────────────

// ── Help hint ─────────────────────────────────────────────────────────────────

// ── Toast ─────────────────────────────────────────────────────────────────────

// ── Helpers ───────────────────────────────────────────────────────────────────
