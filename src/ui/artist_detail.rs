// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Artist detail view: art, biography and the carousel of catalogue tabs.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::*;
use crate::app::App;

/// Narrowest the sidebar gets on a small window, so the bio stays readable once
/// the art has shrunk.
const ARTIST_SIDEBAR_MIN_W: u16 = 22;

pub(super) fn render_artist_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let frame = art_frame(sidebar_art_bounds(area));
    let sidebar_w = frame.width.max(ARTIST_SIDEBAR_MIN_W);
    let cols = Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Min(0)]).split(area);
    let left_rows =
        Layout::vertical([Constraint::Length(frame.height), Constraint::Min(0)]).split(cols[0]);

    render_framed_art(
        f,
        frame,
        detail.art_bytes.as_deref(),
        detail.art_loading,
        app.tick,
        palette().border,
    );
    render_artist_bio(f, app, detail, left_rows[1]);
    render_carousel_tabs(f, app, detail, cols[1]);
}

pub(super) fn render_artist_bio(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let focused = detail.focus == ArtistDetailFocus::Bio;
    // The name rides in the border: under a cover limited by the window's
    // height the box has five rows, and a name line plus a spacer left the bio
    // just one of them.
    let name = Span::styled(
        format!(" {} ", detail.artist.name),
        Style::default()
            .fg(palette().accent)
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(name))
        .title_alignment(Alignment::Center)
        .border_style(if focused {
            Style::default().fg(palette().accent)
        } else {
            Style::default().fg(palette().border)
        });
    let inner = block.inner(area);
    f.render_widget(block, area);

    if detail.bio_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(palette().dim))
                .alignment(Alignment::Center),
            inner,
        );
    } else if let Some(bio) = &detail.bio {
        // Strip HTML tags that Tidal sometimes includes.
        let clean: String = {
            let mut out = String::with_capacity(bio.len());
            let mut in_tag = false;
            for ch in bio.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => out.push(ch),
                    _ => {}
                }
            }
            out
        };
        f.render_widget(
            Paragraph::new(clean)
                .style(Style::default().fg(palette().bio))
                .wrap(Wrap { trim: true })
                .scroll((detail.bio_scroll, 0)),
            inner,
        );
    } else {
        f.render_widget(
            Paragraph::new("No biography available.")
                .style(Style::default().fg(palette().dim))
                .alignment(Alignment::Center),
            inner,
        );
    }
}

pub(super) fn render_carousel_tabs(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let labels = [
        (
            format!("Top Tracks ({})", detail.tracks.visible_len()),
            detail.focus == ArtistDetailFocus::Tracks,
        ),
        (
            format!("Albums ({})", detail.albums.visible_len()),
            detail.focus == ArtistDetailFocus::Albums,
        ),
        (
            format!("EPs ({})", detail.eps.visible_len()),
            detail.focus == ArtistDetailFocus::EPs,
        ),
        (
            format!("Singles ({})", detail.singles.visible_len()),
            detail.focus == ArtistDetailFocus::Singles,
        ),
    ];

    let Some(inner) = render_carousel(f, area, &labels) else {
        return;
    };

    match detail.focus {
        ArtistDetailFocus::Tracks => render_artist_tracks_full(f, app, detail, inner),
        ArtistDetailFocus::Albums => render_artist_albums(f, app, detail, inner),
        ArtistDetailFocus::EPs => render_artist_eps(f, app, detail, inner),
        ArtistDetailFocus::Singles => render_artist_singles(f, app, detail, inner),
        ArtistDetailFocus::Bio => {}
    }
}

pub(super) fn render_artist_tracks_full(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.tracks.loading;
    let focused = app.content_focused();

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(palette().dim)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let items: Vec<ListItem> = detail
        .tracks
        .visible_window(height)
        .into_iter()
        .map(|(i, track)| {
            let selected = i == detail.tracks.selected && focused;
            let style = row_style(selected);
            let playing = app
                .now_playing
                .track
                .as_ref()
                .map(|t| t.id == track.id)
                .unwrap_or(false);
            // `i` stays 0-based for selection; only the displayed ordinal is 1-based.
            let ordinal = format!("{:>3}. ", i + 1);
            ListItem::new(track_row(
                app,
                track,
                area.width,
                Some(ordinal),
                selected,
                playing,
                style,
            ))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

pub(super) fn render_artist_albums(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.albums.loading;
    let focused = app.content_focused();

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(palette().dim)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let items: Vec<ListItem> = detail
        .albums
        .visible_window(height)
        .into_iter()
        .map(|(i, album)| {
            let selected = i == detail.albums.selected && focused;
            let style = row_style(selected);
            ListItem::new(album_row(app, album, area.width, selected, false, style))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

pub(super) fn render_artist_eps(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.eps.loading;
    let focused = app.content_focused();

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(palette().dim)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let items: Vec<ListItem> = detail
        .eps
        .visible_window(height)
        .into_iter()
        .map(|(i, album)| {
            let selected = i == detail.eps.selected && focused;
            let style = row_style(selected);
            ListItem::new(album_row(app, album, area.width, selected, false, style))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

pub(super) fn render_artist_singles(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let spinner = spinner_char(app.tick);
    let loading = detail.singles.loading;
    let focused = app.content_focused();

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(palette().dim)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let items: Vec<ListItem> = detail
        .singles
        .visible_window(height)
        .into_iter()
        .map(|(i, album)| {
            let selected = i == detail.singles.selected && focused;
            let style = row_style(selected);
            ListItem::new(album_row(app, album, area.width, selected, false, style))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}
