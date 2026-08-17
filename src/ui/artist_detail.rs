// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Artist detail view: art, biography and the carousel of catalogue tabs.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::*;
use crate::app::App;

pub(super) fn render_artist_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let art_col_w: u16 = 22;
    let art_inner_w = art_col_w.saturating_sub(2);
    let art_h = art_inner_w / 2;
    let art_box_h = art_h + 2;

    let cols = Layout::horizontal([Constraint::Length(art_col_w), Constraint::Min(0)]).split(area);

    let left_rows =
        Layout::vertical([Constraint::Length(art_box_h), Constraint::Min(0)]).split(cols[0]);

    render_artist_art(f, app, detail, left_rows[0]);

    render_artist_bio(f, app, detail, left_rows[1]);
    //use Render carousel tabs to render
    render_carousel_tabs(f, app, detail, cols[1]);
}

pub(super) fn render_artist_art(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let art_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));
    let inner = art_block.inner(area);
    f.render_widget(art_block, area);

    let w = inner.width;
    let h = inner.height;
    if w == 0 || h == 0 {
        return;
    }

    if let Some(bytes) = &detail.art_bytes {
        render_image(f, bytes, inner);
    } else if detail.art_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
    }
}

pub(super) fn render_artist_bio(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let focused = detail.focus == ArtistDetailFocus::Bio;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(DIM)
        });
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Artist name always at the top.
    f.render_widget(
        Paragraph::new(detail.artist.name.as_str())
            .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height < 3 {
        return;
    }

    let bio_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 2);

    if detail.bio_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            bio_area,
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
                .style(Style::default().fg(Color::Rgb(180, 180, 180)))
                .wrap(Wrap { trim: true })
                .scroll((detail.bio_scroll, 0)),
            bio_area,
        );
    } else {
        f.render_widget(
            Paragraph::new("No biography available.")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            bio_area,
        );
    }
}

pub(super) fn render_carousel_tabs(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    if area.height < 2 {
        return;
    }

    let tabs = vec![
        (
            format!(" Top Tracks ({})", detail.tracks.items.len()),
            ArtistDetailFocus::Tracks,
        ),
        (
            format!("Albums ({})", detail.albums.items.len()),
            ArtistDetailFocus::Albums,
        ),
        (
            format!("EPs ({})", detail.eps.items.len()),
            ArtistDetailFocus::EPs,
        ),
        (
            format!("Singles ({})", detail.singles.items.len()),
            ArtistDetailFocus::Singles,
        ),
    ];

    // Spans + Line seperators. can be changed or removed completely.
    let mut line_spans = Vec::new();
    for (i, (name, focus)) in tabs.iter().enumerate() {
        if i > 0 {
            line_spans.push(Span::styled(" - ", Style::default().fg(DIM)));
        }

        let selected = detail.focus == *focus;
        let style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        line_spans.push(Span::styled(name.clone(), style));
    }
    //block styling (made it dim cuz that fit better with the surrounding UI)
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Line::from(line_spans));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height > 0 {
        match detail.focus {
            ArtistDetailFocus::Tracks => render_artist_tracks_full(f, app, detail, inner),
            ArtistDetailFocus::Albums => render_artist_albums(f, app, detail, inner),
            ArtistDetailFocus::EPs => render_artist_eps(f, app, detail, inner),
            ArtistDetailFocus::Singles => render_artist_singles(f, app, detail, inner),
            ArtistDetailFocus::Bio => {}
        }
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
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.tracks.scroll_offset(height);
    let items: Vec<ListItem> = detail
        .tracks
        .items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, track)| {
            let selected = i == detail.tracks.selected && focused;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = app
                .now_playing
                .track
                .as_ref()
                .map(|t| t.id == track.id)
                .unwrap_or(false);
            let indicator = if playing { "♪ " } else { "" };
            // `i` stays 0-based for selection; only the displayed ordinal is 1-based.
            let n = i + 1;

            let title_span = Span::styled(
                format!(
                    "{prefix}{indicator}{n:>2}. {} ({})",
                    track.title,
                    track.duration_display()
                ),
                style,
            );

            let badge = track
                .quality_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            let badge_span = Span::styled(
                badge,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            );

            let heart = if app.favorite_track_ids.contains(&track.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
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
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.albums.scroll_offset(height);
    let items: Vec<ListItem> = detail
        .albums
        .items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.albums.selected && focused;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style,
            );

            let badge = album
                .quality_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            let badge_span = Span::styled(
                badge,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            );

            let heart = if app.favorite_album_ids.contains(&album.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
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
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.eps.scroll_offset(height);
    let items: Vec<ListItem> = detail
        .eps
        .items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.eps.selected && focused;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style,
            );

            let badge = album
                .quality_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            let badge_span = Span::styled(
                badge,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            );

            let heart = if app.favorite_album_ids.contains(&album.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
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
    let focused = true;

    if loading {
        let msg = format!("Loading {spinner}");
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }

    let inner = area;

    let height = inner.height as usize;
    let offset = detail.singles.scroll_offset(height);
    let items: Vec<ListItem> = detail
        .singles
        .items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.singles.selected && focused;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style,
            );

            let badge = album
                .quality_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            let badge_span = Span::styled(
                badge,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            );

            let heart = if app.favorite_album_ids.contains(&album.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}
