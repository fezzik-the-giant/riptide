// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Playlist detail view.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::*;
use crate::app::App;

pub(super) fn render_playlist_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::PlaylistDetail,
    area: Rect,
) {
    // Layout: left sidebar (art + metadata) | right (track list)
    let art_cols = (area.width / 4).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7));
    let art_box_h = art_rows + 2;
    let left_col_w = art_cols + 2;

    let cols = Layout::horizontal([Constraint::Length(left_col_w), Constraint::Min(0)]).split(area);

    let left_rows =
        Layout::vertical([Constraint::Length(art_box_h), Constraint::Min(0)]).split(cols[0]);

    let header_cols = [left_rows[0], left_rows[1]];

    // ── Playlist cover art ────────────────────────────────────────────────────
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let art_inner = art_block.inner(header_cols[0]);
    f.render_widget(art_block, header_cols[0]);

    if let Some(bytes) = &detail.art_bytes {
        render_image(f, bytes, art_inner);
    } else if detail.art_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(format!("{spinner}"))
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            art_inner,
        );
    }

    // ── Playlist metadata (title + description merged) ────────────────────────
    let n_tracks = detail.playlist.number_of_tracks.unwrap_or(0);
    let focused = detail.focus == PlaylistDetailFocus::Description;
    let meta_area = header_cols[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(DIM)
        });
    let inner = block.inner(meta_area);
    f.render_widget(block, meta_area);

    if inner.height < 3 {
        return;
    }

    // Split inner area into sections: title, track count, description
    let sections = Layout::vertical([
        Constraint::Max(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    // Playlist title (wrapped)
    f.render_widget(
        Paragraph::new(detail.playlist.title.as_str())
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center),
        sections[0],
    );

    // Track count
    f.render_widget(
        Paragraph::new(format!("{} tracks", n_tracks))
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center),
        sections[1],
    );

    // Description with scrolling (if it exists)
    if let Some(desc) = &detail.playlist.description {
        if !desc.is_empty() {
            f.render_widget(
                Paragraph::new(desc.as_str())
                    .style(Style::default().fg(DIM))
                    .wrap(Wrap { trim: true })
                    .scroll((detail.description_scroll, 0)),
                sections[2],
            );
        }
    }

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.items.len())
    };
    let tracks_focused = detail.focus == PlaylistDetailFocus::Tracks;
    render_track_list(f, app, &detail.tracks, tracks_focused, cols[1], &title);
}
