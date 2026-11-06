// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Album detail view.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::*;
use crate::app::App;

pub(super) fn render_album_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::AlbumDetail,
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

    // ── Album metadata ────────────────────────────────────────────────────────
    let year = detail
        .album
        .release_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .unwrap_or("----");
    let n_tracks = detail.album.number_of_tracks.unwrap_or(0);
    let artist_name = detail
        .album
        .artist
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("");

    let quality_badge = detail.album.quality_badge(app.atmos);

    let mut meta_lines = Vec::new();

    // Wrap title across multiple lines if needed
    let max_width = (header_cols[1].width as usize).saturating_sub(2); // account for borders
    let mut title_line = String::new();
    for word in detail.album.title.split_whitespace() {
        if title_line.len() + word.len() < max_width {
            if !title_line.is_empty() {
                title_line.push(' ');
            }
            title_line.push_str(word);
        } else {
            if !title_line.is_empty() {
                meta_lines.push(Line::from(Span::styled(
                    title_line.clone(),
                    Style::default()
                        .fg(palette().accent)
                        .add_modifier(Modifier::BOLD),
                )));
                title_line.clear();
            }
            title_line.push_str(word);
        }
    }
    if !title_line.is_empty() {
        meta_lines.push(Line::from(Span::styled(
            title_line,
            Style::default()
                .fg(palette().accent)
                .add_modifier(Modifier::BOLD),
        )));
    }

    meta_lines.push(Line::from(Span::styled(
        artist_name,
        Style::default().fg(palette().text),
    )));

    let mut counts_spans = vec![Span::styled(
        format!("{year}  •  {n_tracks} tracks"),
        Style::default().fg(palette().dim),
    )];
    if let Some(badge) = quality_badge {
        counts_spans.push(Span::styled("  ", Style::default()));
        counts_spans.push(Span::styled(
            badge,
            Style::default()
                .fg(palette().accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    meta_lines.push(Line::from(counts_spans));

    let info = Paragraph::new(meta_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette().border)),
    );
    f.render_widget(info, header_cols[1]);

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.visible_len())
    };
    render_track_list(f, app, &detail.tracks, true, None, cols[1], &title);
}
