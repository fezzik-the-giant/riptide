// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Album detail view.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
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
    // Left column: art (top) + metadata (below).  Right column: full-height track list.
    let art_cols = (area.width / 4).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7)); // cap so metadata fits
    let art_box_h = art_rows + 2; // +2 borders
    let left_col_w = art_cols + 2;

    // Horizontal split: left sidebar | tracks
    let cols = Layout::horizontal([Constraint::Length(left_col_w), Constraint::Min(0)]).split(area);

    // Left sidebar: art (fixed) + metadata (remainder)
    let left_rows =
        Layout::vertical([Constraint::Length(art_box_h), Constraint::Min(0)]).split(cols[0]);

    // Alias for clarity — art area is left_rows[0], metadata is left_rows[1]
    let header_cols = [left_rows[0], left_rows[1]];

    // ── Album art ─────────────────────────────────────────────────────────────
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

    let quality_badge = detail.album.quality_badge();

    let mut meta_lines = Vec::new();

    // Wrap title across multiple lines if needed
    let max_width = (header_cols[1].width as usize).saturating_sub(2); // account for borders
    let mut title_line = String::new();
    for word in detail.album.title.split_whitespace() {
        if title_line.len() + word.len() + 1 <= max_width {
            if !title_line.is_empty() {
                title_line.push(' ');
            }
            title_line.push_str(word);
        } else {
            if !title_line.is_empty() {
                meta_lines.push(Line::from(Span::styled(
                    title_line.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
                title_line.clear();
            }
            title_line.push_str(word);
        }
    }
    if !title_line.is_empty() {
        meta_lines.push(Line::from(Span::styled(
            title_line,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
    }

    meta_lines.push(Line::from(Span::styled(
        artist_name,
        Style::default().fg(Color::White),
    )));

    let mut counts_spans = vec![Span::styled(
        format!("{year}  •  {n_tracks} tracks"),
        Style::default().fg(DIM),
    )];
    if let Some(badge) = quality_badge {
        counts_spans.push(Span::styled("  ", Style::default()));
        counts_spans.push(Span::styled(
            badge,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    meta_lines.push(Line::from(counts_spans));

    let info = Paragraph::new(meta_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(DIM)),
    );
    f.render_widget(info, header_cols[1]);

    // ── Track list (full right column) ────────────────────────────────────────
    let spinner = spinner_char(app.tick);
    let title = if detail.tracks.loading {
        format!(" Tracks {spinner} ")
    } else {
        format!(" Tracks ({}) ", detail.tracks.items.len())
    };
    render_track_list(f, app, &detail.tracks, true, cols[1], &title);
}
