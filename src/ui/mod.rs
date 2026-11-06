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
use crate::app::{
    App, ArtistDetailFocus, KeybindGroup, QUEUE_W, StatusLevel, Tab, View, queue_fits,
};
use crate::playlist::PlaylistDetailFocus;
use crate::search::SearchPane;

mod album_detail;
mod art;
mod artist_detail;
mod carousel;
mod footer;
mod framed_art;
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
mod visualizer;

use album_detail::*;
use art::*;
use artist_detail::*;
use carousel::*;
use footer::*;
use framed_art::*;
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
use visualizer::*;

pub fn draw(f: &mut Frame, app: &App) {
    set_palette(app.active_palette());
    paint_background(f, f.area());
    let area = f.area();
    let overlays = Overlays::none()
        .with(Overlays::COMMAND, app.command.active)
        .with(Overlays::SORT, app.sort_palette.active)
        .with(Overlays::ARTIST_PICKER, app.artist_selection.active)
        .with(Overlays::PLAYLIST_PICKER, app.playlist_picker.active)
        .with(Overlays::NAME_PROMPT, app.name_prompt.active)
        .with(Overlays::DELETE_CONFIRM, app.delete_confirm.is_some())
        .with(Overlays::HELP, app.help_active)
        .with(Overlays::SETTINGS, app.settings.active)
        .with(Overlays::THEME_PICKER, app.theme_picker.active)
        .with(Overlays::STATUS, app.status.is_some());
    prepare_image_frame(app.art_fullscreen, overlays);

    if app.art_fullscreen {
        render_art_view(f, app, area);
        render_overlays(f, app, area);
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3), // tab bar (boxed active tab needs 3 rows)
        Constraint::Min(0),    // content + queue
        Constraint::Length(now_playing_height(area.height)),
        Constraint::Length(1), // help hint
    ])
    .split(area);

    render_tab_bar(f, app, rows[0]);

    if app.queue_visible && queue_fits(area.width) {
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

    if app.playlist_picker.active {
        render_playlist_picker(f, app, area);
    }

    if app.name_prompt.active {
        render_name_prompt(f, app, area);
    }

    if let Some(playlist) = &app.delete_confirm {
        render_delete_confirm(f, &playlist.title, area);
    }

    if app.help_active {
        render_help_modal(f, app, area);
    }

    if app.settings.active {
        render_settings_modal(f, app, area);
    }

    if app.theme_picker.active {
        render_theme_picker(f, app, area);
    }

    if app.update.active {
        render_update_modal(f, app, area);
    }

    render_toast(f, app, area);
}

// ── Tab bar ───────────────────────────────────────────────────────────────────

// ── Queue panel ───────────────────────────────────────────────────────────────

// ── Command overlay ───────────────────────────────────────────────────────────

// ── Sort overlay ──────────────────────────────────────────────────────────────

// ── Artist selection modal ────────────────────────────────────────────────────

// ── Help modal ────────────────────────────────────────────────────────────────

// ── Settings modal ────────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::test_app;
    use crate::colors::Palette;
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    fn backgrounds(app: &App) -> Vec<Color> {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.bg)
            .collect()
    }

    fn assert_only(bgs: &[Color], allowed: &[Color]) {
        let stray: Vec<_> = bgs.iter().filter(|bg| !allowed.contains(bg)).collect();
        assert!(
            stray.is_empty(),
            "{} cells with stray backgrounds, e.g. {:?}",
            stray.len(),
            stray[0]
        );
    }

    #[test]
    fn fixed_theme_paints_every_cell() {
        let mut t = test_app();
        t.app.set_theme("Cyberpunk");
        let p = t.app.active_palette();
        assert_only(
            &backgrounds(&t.app),
            &[p.background.unwrap(), p.highlight_bg, p.select_bg],
        );
    }

    #[test]
    fn fixed_theme_covers_overlays() {
        let mut t = test_app();
        t.app.set_theme("Cyberpunk");
        t.app.command.active = true;
        let p = t.app.active_palette();
        assert_only(
            &backgrounds(&t.app),
            &[p.background.unwrap(), p.highlight_bg, p.select_bg],
        );
    }

    fn marker_colours(app: &App) -> Vec<Color> {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .iter()
            .filter(|c| c.symbol() == "♪")
            .map(|c| c.fg)
            .collect()
    }

    /// The marker must read as the accent whether or not the cursor bar is on
    /// its row, in the library and in the queue alike.
    #[test]
    fn now_playing_marker_is_the_accent_everywhere() {
        use crate::app::Tab;
        use crate::app::test_support::track;
        let p = Palette::DEFAULT;
        for queue_focused in [false, true] {
            let mut t = test_app();
            t.app.current_tab = Tab::Favorites;
            t.app.favorites.append_page(vec![track(1), track(2)], None);
            t.app.now_playing.queue = vec![track(1), track(2)];
            t.app.now_playing.queue_index = 0;
            t.app.now_playing.track = Some(track(1));
            t.app.queue_visible = true;
            t.app.queue_focused = queue_focused;
            t.app.queue_cursor = 0;

            let colours = marker_colours(&t.app);
            assert_eq!(
                colours.len(),
                2,
                "expected a marker in the library and the queue"
            );
            assert!(
                colours
                    .iter()
                    .all(|c| *c == p.accent || *c == p.highlight_accent),
                "queue_focused={queue_focused}: marker drawn in {colours:?}"
            );
        }
    }

    #[test]
    fn default_theme_paints_no_background() {
        let p = Palette::DEFAULT;
        let t = test_app();
        assert_only(
            &backgrounds(&t.app),
            &[Color::Reset, p.highlight_bg, p.select_bg],
        );
    }
}
