// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Top-level library lists and the shared track-list renderer.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::*;
use crate::app::{App, StatefulList};

pub(super) fn render_content(f: &mut Frame, app: &App, area: Rect) {
    // The filter box takes a slice off the top rather than floating over the
    // list — the point is to watch the list narrow while typing.
    let area = if app.filter_active && app.filterable_tab() {
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
        render_filter_box(f, app, rows[0]);
        rows[1]
    } else {
        area
    };

    // If there's a view on the stack, render it
    if let Some(view) = app.view_stack.last() {
        match view {
            View::ArtistDetail(detail) => {
                render_artist_detail(f, app, detail, area);
                return;
            }
            View::PlaylistDetail(detail) => {
                render_playlist_detail(f, app, detail, area);
                return;
            }
            View::AlbumDetail(detail) => {
                render_album_detail(f, app, detail, area);
                return;
            }
        }
    }

    match app.current_tab {
        Tab::Home => render_home(f, app, area),
        Tab::Artists => render_artist_list(f, app, area),
        Tab::Albums => render_fav_albums_list(f, app, area),
        Tab::Playlists => render_playlist_list(f, app, area),
        Tab::Favorites => {
            let title = list_title("Tracks", &app.favorites, app.favorites.items.len(), app);
            render_track_list(f, app, &app.favorites, true, area, &title);
        }
        Tab::Search => render_search_results(f, app, area),
    }
}

/// Trailing " · A-Z" for a list title, showing how the list is ordered. Empty on
/// tabs that don't sort, so it can be appended unconditionally.
pub(super) fn sort_suffix(app: &App) -> String {
    app.active_sort()
        .map(|f| format!(" · {}", f.label()))
        .unwrap_or_default()
}

/// " Tracks (3 of 214) · A-Z · /ts " — an active filter is always named in the
/// title, so a narrowed list can never look like the whole library.
fn list_title<T>(name: &str, list: &StatefulList<T>, total: usize, app: &App) -> String {
    if list.is_filtered() {
        format!(
            " {name} ({} of {total}){} · /{} ",
            list.visible_len(),
            sort_suffix(app),
            list.filter()
        )
    } else {
        format!(" {name} ({total}){} ", sort_suffix(app))
    }
}

/// What to show in place of an empty list, distinguishing "you have none" from
/// "none match what you typed".
fn empty_text<T>(list: &StatefulList<T>, when_empty: &str) -> String {
    if list.is_filtered() {
        format!("No matches for \"{}\".", list.filter())
    } else {
        when_empty.to_string()
    }
}

fn render_filter_box(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    f.render_widget(block, area);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "/ ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(app.active_filter(), Style::default().fg(Color::White)),
            Span::styled(cursor_char(app.tick), Style::default().fg(ACCENT)),
        ])),
        inner,
    );
}

pub(super) fn render_artist_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.artists.loading && app.artists.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Artists {spinner} ")
        } else {
            list_title("Artists", &app.artists, app.artists.total as usize, app)
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let items: Vec<ListItem> = app
        .artists
        .visible_window(height)
        .iter()
        .map(|(idx, artist)| {
            let selected = *idx == app.artists.selected;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{}", artist.name)).style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new(empty_text(&app.artists, "No followed artists found."))
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

pub(super) fn render_fav_albums_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.fav_albums.loading && app.fav_albums.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Albums {spinner} ")
        } else {
            list_title(
                "Albums",
                &app.fav_albums,
                app.fav_albums.total as usize,
                app,
            )
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.fav_albums.visible_len() == 0 && !loading {
        f.render_widget(
            Paragraph::new(empty_text(&app.fav_albums, "No saved albums found."))
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let selected = app.fav_albums.selected;

    let items: Vec<ListItem> = app
        .fav_albums
        .visible_window(height)
        .iter()
        .map(|(idx, album)| {
            let is_sel = *idx == selected;
            let bg = if is_sel { HIGHLIGHT_BG } else { Color::Reset };
            let prefix = if is_sel { "▶ " } else { "  " };
            let artist = album.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            let badge = album
                .quality_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();

            let title_style = Style::default()
                .bg(bg)
                .fg(Color::White)
                .add_modifier(if is_sel {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            let sub_style = Style::default().bg(bg).fg(DIM);
            let badge_style = Style::default()
                .bg(bg)
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD);

            let line = Line::from(vec![
                Span::styled(format!("{prefix}{}", album.title), title_style),
                Span::styled(
                    if artist.is_empty() {
                        String::new()
                    } else {
                        format!("  {artist}")
                    },
                    sub_style,
                ),
                Span::styled(badge, badge_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

pub(super) fn render_playlist_list(f: &mut Frame, app: &App, area: Rect) {
    let spinner = spinner_char(app.tick);
    let loading = app.playlists.loading && app.playlists.items.is_empty();

    let block = Block::default()
        .title(if loading {
            format!(" Playlists {spinner} ")
        } else {
            list_title(
                "Playlists",
                &app.playlists,
                app.playlists.total as usize,
                app,
            )
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let items: Vec<ListItem> = app
        .playlists
        .visible_window(height)
        .iter()
        .map(|(i, pl)| {
            let selected = *i == app.playlists.selected;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!(
                "{prefix}{} ({} tracks)",
                pl.title,
                pl.number_of_tracks.unwrap_or(0)
            ))
            .style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new(empty_text(&app.playlists, "No playlists found."))
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

pub(super) fn render_track_list(
    f: &mut Frame,
    app: &App,
    tracks: &crate::app::StatefulList<Track>,
    focused: bool,
    area: Rect,
    title: &str,
) {
    let selected = tracks.selected;
    let block = Block::default()
        .title(title)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;

    let items: Vec<ListItem> = tracks
        .visible_window(height)
        .iter()
        .map(|&(i, track)| {
            let is_selected = i == selected && focused && !app.help_active;
            let is_playing = app
                .now_playing
                .track
                .as_ref()
                .map(|t| t.id == track.id)
                .unwrap_or(false);
            let style = if is_selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };
            // `i` stays 0-based for selection; only the displayed ordinal is 1-based.
            let n = i + 1;

            let title_span = Span::styled(
                format!(
                    "{prefix}{playing}{n:>3}. {} — {} ({})",
                    track.title,
                    track.all_artist_names(),
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

    if items.is_empty() {
        let p = Paragraph::new(empty_text(tracks, "No tracks."))
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}
