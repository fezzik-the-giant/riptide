// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Playlist detail view.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
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
    let frame = art_frame(sidebar_art_bounds(area));
    let left_col_w = frame.width.max(DETAIL_SIDEBAR_MIN_W);
    let cols = Layout::horizontal([Constraint::Length(left_col_w), Constraint::Min(0)]).split(area);
    let left_rows =
        Layout::vertical([Constraint::Length(frame.height), Constraint::Min(0)]).split(cols[0]);
    let header_cols = [left_rows[0], left_rows[1]];

    render_framed_art(
        f,
        frame,
        detail.art_bytes.as_deref(),
        detail.art_loading,
        app.tick,
        palette().accent,
    );

    // ── Playlist metadata (title + description merged) ────────────────────────
    let focused = detail.focus == PlaylistDetailFocus::Description;
    let meta_area = header_cols[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::default().fg(palette().accent)
        } else {
            Style::default().fg(palette().border)
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
            .style(
                Style::default()
                    .fg(palette().accent)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center),
        sections[0],
    );

    let kind = detail.playlist.kind(&app.owned_playlist_ids).label();
    let header = match (kind, detail.playlist.track_count_label()) {
        ("", None) => None,
        ("", Some(count)) => Some(count),
        (kind, None) => Some(kind.to_string()),
        (kind, Some(count)) => Some(format!("{kind} · {count}")),
    };
    if let Some(header) = header {
        f.render_widget(
            Paragraph::new(header)
                .style(Style::default().fg(palette().text))
                .alignment(Alignment::Center),
            sections[1],
        );
    }

    // Description with scrolling (if it exists)
    if let Some(desc) = &detail.playlist.description
        && !desc.is_empty()
    {
        f.render_widget(
            Paragraph::new(desc.as_str())
                .style(Style::default().fg(palette().dim))
                .wrap(Wrap { trim: true })
                .scroll((detail.description_scroll, 0)),
            sections[2],
        );
    }

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.visible_len())
    };
    let tracks_focused = detail.focus == PlaylistDetailFocus::Tracks;
    render_track_list(
        f,
        app,
        &detail.tracks,
        tracks_focused,
        app.playlist_move.as_ref().map(|m| m.item_id.as_str()),
        cols[1],
        &title,
    );
}
