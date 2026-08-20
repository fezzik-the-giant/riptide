// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Top-level library lists and the shared track-list renderer.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::*;
use crate::api::models::{Album, Playlist};
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
            ListItem::new(simple_row(
                app,
                &artist.name,
                inner.width,
                selected,
                style,
                "",
            ))
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
            let title_style = Style::default()
                .bg(bg)
                .fg(Color::White)
                .add_modifier(if is_sel {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            let sub_style = Style::default().bg(bg).fg(DIM);
            ListItem::new(album_row(
                app,
                album,
                inner.width,
                is_sel,
                true,
                title_style,
                sub_style,
            ))
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
            ListItem::new(playlist_row(
                app,
                pl,
                inner.width,
                selected,
                style,
                Style::default().fg(DIM),
            ))
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

/// The marquee only runs on the row under the cursor.
fn marquee_phase(app: &App, is_selected: bool) -> Option<std::time::Duration> {
    is_selected.then(|| app.marquee_phase())
}

/// One album row. `show_artist` is off inside an artist's own page, where the
/// column would repeat the artist on every line.
pub(super) fn album_row(
    app: &App,
    album: &Album,
    width: u16,
    is_selected: bool,
    show_artist: bool,
    style: Style,
    dim: Style,
) -> Line<'static> {
    let phase = marquee_phase(app, is_selected);
    let mut cells = vec![
        Cell::fixed(if is_selected { "▶ " } else { "  " }, 2, style),
        Cell::flex(album.title.clone(), 3, 0, style),
    ];
    if show_artist {
        cells.push(Cell::flex(
            album
                .artist
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            2,
            12,
            dim,
        ));
    }
    cells.push(
        Cell::fixed(
            album
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .unwrap_or("")
                .to_string(),
            6,
            dim,
        )
        .right(),
    );
    cells.push(
        Cell::fixed(
            album
                .number_of_tracks
                .map(|n| format!("{n} tracks"))
                .unwrap_or_default(),
            11,
            dim,
        )
        .right(),
    );
    cells.push(Cell::fixed(
        album
            .quality_badge()
            .map(|b| format!(" [{b}]"))
            .unwrap_or_default(),
        8,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
    cells.push(Cell::fixed(
        if app.favorite_album_ids.contains(&album.id) {
            " ❤"
        } else {
            ""
        },
        2,
        style,
    ));
    layout_row(width, cells, phase)
}

pub(super) fn playlist_row(
    app: &App,
    playlist: &Playlist,
    width: u16,
    is_selected: bool,
    style: Style,
    dim: Style,
) -> Line<'static> {
    layout_row(
        width,
        vec![
            Cell::fixed(if is_selected { "▶ " } else { "  " }, 2, style),
            Cell::flex(playlist.title.clone(), 1, 0, style),
            Cell::fixed(playlist.track_count_label().unwrap_or_default(), 11, dim).right(),
            Cell::fixed(
                if app.favorite_playlist_ids.contains(&playlist.uuid) {
                    " ❤"
                } else {
                    ""
                },
                2,
                style,
            ),
        ],
        marquee_phase(app, is_selected),
    )
}

/// A single-field row: no columns to align, just a cursor and text that stops at
/// the edge instead of being chopped mid-character.
pub(super) fn simple_row(
    app: &App,
    text: &str,
    width: u16,
    is_selected: bool,
    style: Style,
    trailing: &str,
) -> Line<'static> {
    layout_row(
        width,
        vec![
            Cell::fixed(if is_selected { "▶ " } else { "  " }, 2, style),
            Cell::flex(text.to_string(), 1, 0, style),
            Cell::fixed(trailing.to_string(), 2, style),
        ],
        marquee_phase(app, is_selected),
    )
}

/// One track row, laid out in columns so the duration, badge and favourite
/// marker stay put whatever the title and artists do.
pub(super) fn track_row(
    app: &App,
    track: &Track,
    width: u16,
    ordinal: Option<String>,
    is_selected: bool,
    is_playing: bool,
    style: Style,
) -> Line<'static> {
    let phase = marquee_phase(app, is_selected);
    let mut cells = vec![
        Cell::fixed(if is_selected { "▶ " } else { "  " }, 2, style),
        Cell::fixed(if is_playing { "♪ " } else { "  " }, 2, style),
    ];
    if let Some(ordinal) = ordinal {
        cells.push(Cell::fixed(ordinal, 5, style));
    }
    cells.push(Cell::flex(track.title.clone(), 3, 0, style));
    cells.push(Cell::flex(track.all_artist_names(), 2, 12, style));
    cells.push(Cell::fixed(track.duration_display(), 6, style).right());
    cells.push(Cell::fixed(
        track
            .quality_badge()
            .map(|b| format!(" [{b}]"))
            .unwrap_or_default(),
        8,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
    cells.push(Cell::fixed(
        if app.favorite_track_ids.contains(&track.id) {
            " ❤"
        } else {
            ""
        },
        2,
        style,
    ));
    layout_row(width, cells, phase)
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
            // `i` stays 0-based for selection; only the displayed ordinal is 1-based.
            let ordinal = format!("{:>3}. ", i + 1);
            ListItem::new(track_row(
                app,
                track,
                inner.width,
                Some(ordinal),
                is_selected,
                is_playing,
                style,
            ))
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Playlist;
    use crate::app::test_support::test_app;
    use ratatui::{Terminal, backend::TestBackend};

    fn playlist(title: &str, number_of_tracks: Option<u32>) -> Playlist {
        Playlist {
            uuid: title.to_string(),
            title: title.to_string(),
            number_of_tracks,
            description: None,
            cover: None,
            added_at: None,
        }
    }

    /// Search results mix saved and unsaved playlists, and the marker is the only
    /// thing that tells them apart.
    #[test]
    fn the_heart_marks_only_saved_playlists() {
        let mut t = test_app();
        t.app
            .favorite_playlist_ids
            .insert("Metal Classics".to_string());
        std::mem::forget(t.api_rx);
        let style = Style::default();

        let row = |title: &str| {
            let line = playlist_row(&t.app, &playlist(title, Some(12)), 60, false, style, style);
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        };

        assert!(row("Metal Classics").contains('❤'));
        assert!(!row("Some Other Playlist").contains('❤'));
    }

    /// A `MIX` playlist reports no `numberOfItems`, and the row used to render
    /// that `None` as `(0 tracks)`.
    #[test]
    fn a_playlist_with_no_count_shows_no_count() {
        let mut t = test_app();
        t.app.playlists.append_page(
            vec![
                playlist("Metal Classics", Some(181)),
                playlist("Run The Jewels", None),
            ],
            None,
        );
        std::mem::forget(t.api_rx);

        let (w, h) = (60u16, 8u16);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_playlist_list(f, &t.app, Rect::new(0, 0, w, h)))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let screen: String = (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(screen.contains("Metal Classics"), "{screen}");
        assert!(screen.contains("181 tracks"), "{screen}");
        assert!(screen.contains("Run The Jewels"), "{screen}");
        assert!(!screen.contains("0 tracks"), "{screen}");
    }
}
