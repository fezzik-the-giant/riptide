// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::{picker::Picker, Image, Resize};

use crate::app::{App, ArtistDetailFocus, StatusLevel, Tab, View, KeybindGroup};
use crate::search::SearchPane;
use crate::playlist::PlaylistDetailFocus;
use crate::api::models::Track;

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

fn fmt_sample_rate(hz: u32) -> String {
    match hz {
        44100  => "44.1 kHz".into(),
        88200  => "88.2 kHz".into(),
        176400 => "176.4 kHz".into(),
        _      => {
            let khz = hz / 1000;
            format!("{khz} kHz")
        }
    }
}
const HIGHLIGHT_BG: Color = Color::Rgb(40, 40, 55);
const SELECT_BG: Color = Color::Rgb(30, 100, 200);
const SIDEBAR_W: u16 = 20;
const QUEUE_W: u16 = 26;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let rows = Layout::vertical([
        Constraint::Min(0),    // sidebar + content + queue
        Constraint::Length(9), // now-playing bar
        Constraint::Length(1), // help hint
    ])
    .split(area);

    let cols = Layout::horizontal([
        Constraint::Length(SIDEBAR_W),
        Constraint::Min(0),
        Constraint::Length(QUEUE_W),
    ])
    .split(rows[0]);

    render_sidebar(f, app, cols[0]);
    render_content(f, app, cols[1]);
    render_queue(f, app, cols[2]);
    render_now_playing(f, app, rows[1]);
    render_footer(f, app, rows[2]);

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

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(40, 40, 40)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height < 4 {
        return;
    }

    let art_h = (inner.width / 2).min(inner.height.saturating_sub(5));

    let layout = Layout::vertical([
        Constraint::Length(art_h),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    render_sidebar_art(f, app, layout[0]);

    let div = "─".repeat(inner.width as usize);
    f.render_widget(
        Paragraph::new(div).style(Style::default().fg(Color::Rgb(40, 40, 40))),
        layout[1],
    );

    render_sidebar_nav(f, app, layout[2]);
}

fn render_sidebar_art(f: &mut Frame, app: &App, area: Rect) {
    let np = &app.now_playing;
    let w = area.width;
    let h = area.height;
    if w == 0 || h == 0 {
        return;
    }

    if let Some(bytes) = &np.art_bytes {
        render_image(f, bytes, area);
    } else if np.art_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(spinner.to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            area,
        );
    }
}

fn render_sidebar_nav(f: &mut Frame, app: &App, area: Rect) {
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let y = area.y + i as u16;
        if y >= area.y + area.height {
            break;
        }
        let selected = app.current_tab == *tab;
        let (prefix, style) = if selected {
            ("│", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        } else {
            (" ", Style::default().fg(DIM))
        };
        f.render_widget(
            Paragraph::new(format!("{}{}", prefix, tab.title())).style(style),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

// ── Queue panel ───────────────────────────────────────────────────────────────

fn render_queue(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.queue_focused;
    let border_style = if focused {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(Color::Rgb(40, 40, 40))
    };
    // No title on the block — ratatui doesn't reserve a row for titles on
    // Borders::LEFT-only blocks, so the title would be overdrawn by content.
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Title row rendered manually at the top of the inner area.
    let queue_title = if app.now_playing.shuffle { " Queue ⇄ " } else { " Queue " };
    f.render_widget(
        Paragraph::new(Span::styled(queue_title, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Content area starts one row below the title.
    let content_y = inner.y + 1;
    let content_h = inner.height.saturating_sub(1);

    let queue = &app.now_playing.queue;
    if queue.is_empty() {
        if content_h > 0 {
            f.render_widget(
                Paragraph::new("no queue")
                    .style(Style::default().fg(DIM))
                    .alignment(Alignment::Center),
                Rect::new(inner.x, content_y, inner.width, content_h),
            );
        }
        return;
    }

    let current = app.now_playing.queue_index;
    let cursor = app.queue_cursor;
    let item_h = 2usize;
    let visible = (content_h as usize).saturating_div(item_h).max(1);
    let offset = app.queue_scroll_offset(visible);

    let mut y = content_y;
    for (i, track) in queue.iter().enumerate().skip(offset) {
        if y + 1 >= content_y + content_h {
            break;
        }
        let is_cur = i == current;
        let is_cursor = focused && i == cursor && !app.help_active;
        let heart = if app.favorite_track_ids.contains(&track.id) { " ❤" } else { "" };
        let (title_line, line_style) = if is_cur {
            (format!("♪ {}{}", track.title, heart), Style::default().fg(Color::Rgb(180, 200, 255)).add_modifier(Modifier::BOLD))
        } else if is_cursor {
            (format!("▶ {}{}", track.title, heart), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        } else {
            (format!("{}{}", track.title, heart), Style::default().fg(Color::White))
        };

        f.render_widget(
            Paragraph::new(title_line).style(line_style),
            Rect::new(inner.x, y, inner.width, 1),
        );
        f.render_widget(
            Paragraph::new(format!("  {}", track.all_artist_names())).style(line_style),
            Rect::new(inner.x, y + 1, inner.width, 1),
        );
        y += item_h as u16;
    }
}

// ── Command overlay ───────────────────────────────────────────────────────────

fn render_command_overlay(f: &mut Frame, app: &App, area: Rect) {
    let matches = app.command.matches();

    let box_w: u16 = 34;
    // border(2) + input(1) + divider(1) + items (at least 1)
    let box_h: u16 = 4 + matches.len().max(1) as u16;

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(" command ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    if inner.height == 0 {
        return;
    }

    // Input line: "/ <typed><ghost>█"
    let q_lower = app.command.input.to_lowercase();
    let ghost = matches.first().map(|m| &m[q_lower.len()..]).unwrap_or("");
    let cursor = if (app.tick / 30) % 2 == 0 { "█" } else { " " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(app.command.input.clone(), Style::default().fg(Color::White)),
            Span::styled(ghost, Style::default().fg(DIM)),
            Span::styled(cursor, Style::default().fg(Color::White)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height < 2 {
        return;
    }

    // Thin divider between input and list
    f.render_widget(
        Paragraph::new("─".repeat(inner.width as usize)).style(Style::default().fg(DIM)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );

    // Command rows
    if matches.is_empty() {
        f.render_widget(
            Paragraph::new(" no match").style(Style::default().fg(DIM)),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    } else {
        for (i, cmd) in matches.iter().enumerate() {
            let row_y = inner.y + 2 + i as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let selected = i == app.command.selected;
            let style = if selected {
                Style::default().bg(SELECT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            f.render_widget(
                Paragraph::new(format!(" {cmd}")).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }
}

// ── Sort overlay ──────────────────────────────────────────────────────────────

fn render_sort_overlay(f: &mut Frame, app: &App, area: Rect) {
    use crate::app::SortPalette;

    let options = SortPalette::get_options(app.current_tab);
    let box_w: u16 = 26;
    let box_h: u16 = 2 + options.len() as u16; // border top/bottom + one row per option

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(" sort by ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    for (i, (label, _)) in options.iter().enumerate() {
        let row_y = inner.y + i as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let selected = i == app.sort_palette.selected;
        let style = if selected {
            Style::default().bg(SELECT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        let prefix = if selected { " ► " } else { "   " };
        f.render_widget(
            Paragraph::new(format!("{prefix}{label}")).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }
}

// ── Artist selection modal ────────────────────────────────────────────────────

fn render_artist_selection_modal(f: &mut Frame, app: &App, area: Rect) {
    let box_w = 40u16.min(area.width.saturating_sub(4));
    let box_h = (4 + app.artist_selection.artist_names.len() as u16).min(area.height.saturating_sub(6));

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(" select artist ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let start_y = inner.y;
    for (i, name) in app.artist_selection.artist_names.iter().enumerate() {
        let row_y = start_y + i as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let selected = i == app.artist_selection.selected;
        let style = if selected {
            Style::default().bg(SELECT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if selected { " ► " } else { "   " };
        let line = format!("{prefix}{}", name);
        f.render_widget(
            Paragraph::new(line).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }
}

// ── Help modal ────────────────────────────────────────────────────────────────

fn render_help_modal(f: &mut Frame, app: &App, area: Rect) {
    // Fixed size modal, well clear of the now-playing bar (9 lines at bottom)
    let box_w = 50u16.min(area.width.saturating_sub(4));
    let box_h = 24u16.min(area.height.saturating_sub(12)); // Leave 9 for now-playing + margin

    // Center horizontally and vertically within the safe area
    let safe_bottom = area.bottom().saturating_sub(10);
    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + (safe_bottom - area.y).saturating_sub(box_h) / 2;

    let overlay = Rect::new(x, y, box_w, box_h);

    let block = Block::default()
        .title(Span::styled(" help ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);

    // Clear only the inner content area
    f.render_widget(Clear, inner);
    f.render_widget(block, overlay);

    if inner.height == 0 || inner.width < 20 {
        return;
    }

    // Collect all keybind groups
    let groups = vec![
        KeybindGroup::global(),
        KeybindGroup::navigation(),
        KeybindGroup::queue(),
        KeybindGroup::search(),
        KeybindGroup::command(),
    ];

    // Build lines for rendering
    let mut lines: Vec<Line> = Vec::new();
    for group in groups {
        // Group header
        lines.push(Line::from(Span::styled(
            group.title,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));

        // Keybinds
        for keybind in group.binds {
            let line = Line::from(vec![
                Span::styled(
                    format!("  {:<12}", keybind.key),
                    Style::default().fg(ACCENT),
                ),
                Span::raw(keybind.action),
            ]);
            lines.push(line);
        }

        // Space between groups
        lines.push(Line::from(""));
    }

    // Render with scrolling
    let start = app.help_scroll as usize;
    let mut y = inner.y;

    for line in lines.iter().skip(start) {
        if y >= inner.y + inner.height {
            break;
        }
        f.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    // Render scroll hint
    if lines.len() as u16 > app.help_scroll + inner.height {
        let hint = " ↓ more ";
        f.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Right),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
    }
}

// ── Main content area ─────────────────────────────────────────────────────────

fn render_content(f: &mut Frame, app: &App, area: Rect) {
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
            let title = format!(" Tracks ({}) ", app.favorites.items.len());
            render_track_list(f, app, &app.favorites, true, area, &title);
        }
        Tab::Search => render_search_results(f, app, area),
    }
}

// ── Home tab ──────────────────────────────────────────────────────────────────

fn render_home(f: &mut Frame, app: &App, area: Rect) {
    use crate::app::HomeSectionFocus;

    let any_loading = app.home_new_releases.loading
        || app.home_daily_mixes.loading
        || app.home_discovery_mixes.loading;

    let title = if any_loading {
        let spinner = spinner_char(app.tick);
        format!(" Loading {} ", spinner)
    } else {
        " Home ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Only show sections once all are loaded
    if any_loading {
        let loading_text = "Fetching mixes...";
        let paragraph = Paragraph::new(loading_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(paragraph, inner);
        return;
    }

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(inner);

    render_home_section(f, &app.home_new_releases, chunks[0], "New Releases",
        app.home_section_focus == HomeSectionFocus::NewReleases);
    render_home_section(f, &app.home_daily_mixes, chunks[1], "Daily Mixes",
        app.home_section_focus == HomeSectionFocus::DailyMixes);
    render_home_section(f, &app.home_discovery_mixes, chunks[2], "Daily Discovery",
        app.home_section_focus == HomeSectionFocus::DiscoveryMixes);
}

fn render_home_section(
    f: &mut Frame,
    section: &crate::app::HomeSection<crate::api::models::Playlist>,
    area: Rect,
    title: &str,
    focused: bool,
) {
    let border_color = if focused { ACCENT } else { Color::Gray };

    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if section.loading {
        let spinner = spinner_char(0);
        let text = format!("{} Loading...", spinner);
        let paragraph = Paragraph::new(text).style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, inner);
        return;
    }

    if let Some(ref error) = section.error {
        let paragraph = Paragraph::new(format!("Error: {}", error))
            .style(Style::default().fg(Color::Red));
        f.render_widget(paragraph, inner);
        return;
    }

    if section.items.is_empty() {
        let paragraph = Paragraph::new("No items")
            .style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, inner);
        return;
    }

    let height = inner.height as usize;
    let start = if section.selected < height {
        0
    } else {
        section.selected - height + 1
    };

    let visible_items: Vec<ListItem> = section.items[start..]
        .iter()
        .take(height)
        .enumerate()
        .map(|(i, item)| {
            let idx = start + i;
            let is_selected = idx == section.selected && focused;

            let title_style = if is_selected {
                Style::default().fg(Color::White).bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
            } else if focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let line = Line::from(Span::styled(format!("{}{}", prefix, item.title), title_style));
            ListItem::new(line)
        })
        .collect();

    let list = List::new(visible_items);
    f.render_widget(list, inner);
}

// ── Artists list ──────────────────────────────────────────────────────────────

fn render_artist_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.artists.loading && app.artists.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Artists {spinner} ")
        } else {
            format!(" Artists ({}) ", app.artists.total)
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let items: Vec<ListItem> = visible_artist_items(&app.artists, height)
        .iter()
        .map(|(abs_idx, artist)| {
            let selected = *abs_idx == app.artists.selected;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{}", artist.name)).style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new("No followed artists found.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Saved albums list ─────────────────────────────────────────────────────────

fn render_fav_albums_list(f: &mut Frame, app: &App, area: Rect) {
    let loading = app.fav_albums.loading && app.fav_albums.items.is_empty();
    let spinner = spinner_char(app.tick);

    let block = Block::default()
        .title(if loading {
            format!(" Albums {spinner} ")
        } else {
            format!(" Albums ({}) ", app.fav_albums.total)
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.fav_albums.items.is_empty() && !loading {
        f.render_widget(
            Paragraph::new("No saved albums found.")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let selected = app.fav_albums.selected;
    let offset = app.fav_albums.scroll_offset(height);

    let items: Vec<ListItem> = app.fav_albums.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(idx, album)| {
            let is_sel = idx == selected;
            let bg = if is_sel { HIGHLIGHT_BG } else { Color::Reset };
            let prefix = if is_sel { "▶ " } else { "  " };
            let artist = album.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();

            let title_style = Style::default()
                .bg(bg)
                .fg(Color::White)
                .add_modifier(if is_sel { Modifier::BOLD } else { Modifier::empty() });
            let sub_style = Style::default().bg(bg).fg(DIM);
            let badge_style = Style::default().bg(bg).fg(ACCENT).add_modifier(Modifier::BOLD);

            let line = Line::from(vec![
                Span::styled(format!("{prefix}{}", album.title), title_style),
                Span::styled(if artist.is_empty() { String::new() } else { format!("  {artist}") }, sub_style),
                Span::styled(badge, badge_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

// ── Artist detail (tracks + albums split) ─────────────────────────────────────

fn render_artist_detail(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    let art_col_w: u16 = 22;
    let art_inner_w = art_col_w.saturating_sub(2);
    let art_h = art_inner_w / 2;
    let art_box_h = art_h + 2;

    let cols = Layout::horizontal([
        Constraint::Length(art_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

    render_artist_art(f, app, detail, left_rows[0]);

    render_artist_bio(f, app, detail, left_rows[1]);
    //use Render carousel tabs to render 
    render_carousel_tabs(f, app, detail, cols[1]);
}

fn render_artist_art(f: &mut Frame, app: &App, detail: &crate::app::ArtistDetail, area: Rect) {
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

fn render_artist_bio(f: &mut Frame, app: &App, detail: &crate::app::ArtistDetail, area: Rect) {
    let focused = detail.focus == ArtistDetailFocus::Bio;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) });
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

fn render_carousel_tabs(
    f: &mut Frame,
    app: &App,
    detail: &crate::app::ArtistDetail,
    area: Rect,
) {
    if area.height < 2 {
        return;
    }

    let tabs = vec![
        (format!(" Top Tracks ({})", detail.tracks.items.len()), ArtistDetailFocus::Tracks),
        (format!("Albums ({})", detail.albums.items.len()), ArtistDetailFocus::Albums),
        (format!("EPs ({})", detail.eps.items.len()), ArtistDetailFocus::EPs),
        (format!("Singles ({})", detail.singles.items.len()), ArtistDetailFocus::Singles),
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

fn render_artist_tracks_full(
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
    let items: Vec<ListItem> = detail.tracks.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, track)| {
            let selected = i == detail.tracks.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = app.now_playing.track.as_ref().map(|t| t.id == track.id).unwrap_or(false);
            let indicator = if playing { "♪ " } else { "" };

            let title_span = Span::styled(
                format!("{prefix}{indicator}{i:>2}. {} ({})", track.title, track.duration_display()),
                style
            );

            let badge = track.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

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

fn render_artist_albums(
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
    let items: Vec<ListItem> = detail.albums.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.albums.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style
            );

            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

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

fn render_artist_eps(
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
    let items: Vec<ListItem> = detail.eps.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.eps.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style
            );

            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

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

fn render_artist_singles(
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
    let items: Vec<ListItem> = detail.singles.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, album)| {
            let selected = i == detail.singles.selected && focused;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let year = album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
            let n = album.number_of_tracks.unwrap_or(0);

            let title_span = Span::styled(
                format!("{}{} ({}, {} tracks)", prefix, album.title, year, n),
                style
            );

            let badge = album.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

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
// ── Playlists ─────────────────────────────────────────────────────────────────

fn render_playlist_list(f: &mut Frame, app: &App, area: Rect) {
    let spinner = spinner_char(app.tick);
    let loading = app.playlists.loading && app.playlists.items.is_empty();

    let block = Block::default()
        .title(if loading {
            format!(" Playlists {spinner} ")
        } else {
            format!(" Playlists ({}) ", app.playlists.total)
        })
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let offset = app.playlists.scroll_offset(height);
    let items: Vec<ListItem> = app.playlists.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, pl)| {
            let selected = i == app.playlists.selected;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{} ({} tracks)", pl.title, pl.number_of_tracks.unwrap_or(0)))
                .style(style)
        })
        .collect();

    if items.is_empty() && !loading {
        let p = Paragraph::new("No playlists found.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Generic track list ────────────────────────────────────────────────────────

fn render_track_list(
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
    let offset = tracks.scroll_offset(height);

    let items: Vec<ListItem> = tracks.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, track)| {
            let is_selected = i == selected && focused && !app.help_active;
            let is_playing = app.now_playing.track.as_ref().map(|t| t.id == track.id).unwrap_or(false);
            let style = if is_selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };

            let title_span = Span::styled(
                format!(
                    "{prefix}{playing}{i:>3}. {} — {} ({})",
                    track.title,
                    track.all_artist_names(),
                    track.duration_display()
                ),
                style
            );

            let badge = track.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_track_ids.contains(&track.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    if items.is_empty() {
        let p = Paragraph::new("No tracks.")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        f.render_widget(p, inner);
        return;
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

// ── Album detail ──────────────────────────────────────────────────────────────

fn render_album_detail(f: &mut Frame, app: &App, detail: &crate::app::AlbumDetail, area: Rect) {
    // Left column: art (top) + metadata (below).  Right column: full-height track list.
    let art_cols = (area.width / 4).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7)); // cap so metadata fits
    let art_box_h = art_rows + 2; // +2 borders
    let left_col_w = art_cols + 2;

    // Horizontal split: left sidebar | tracks
    let cols = Layout::horizontal([
        Constraint::Length(left_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    // Left sidebar: art (fixed) + metadata (remainder)
    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

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
    let year = detail.album.release_date.as_deref().and_then(|d| d.get(..4)).unwrap_or("----");
    let n_tracks = detail.album.number_of_tracks.unwrap_or(0);
    let artist_name = detail.album.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("");

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

    meta_lines.push(Line::from(Span::styled(artist_name, Style::default().fg(Color::White))));

    let mut counts_spans = vec![
        Span::styled(format!("{year}  •  {n_tracks} tracks"), Style::default().fg(DIM)),
    ];
    if let Some(badge) = quality_badge {
        counts_spans.push(Span::styled("  ", Style::default()));
        counts_spans.push(Span::styled(
            badge,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    meta_lines.push(Line::from(counts_spans));

    let info = Paragraph::new(meta_lines)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(DIM)));
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

fn render_playlist_detail(f: &mut Frame, app: &App, detail: &crate::app::PlaylistDetail, area: Rect) {
    // Layout: left sidebar (art + metadata) | right (track list)
    let art_cols = (area.width / 4).max(10);
    let art_rows = (art_cols / 2).max(5).min(area.height.saturating_sub(7));
    let art_box_h = art_rows + 2;
    let left_col_w = art_cols + 2;

    let cols = Layout::horizontal([
        Constraint::Length(left_col_w),
        Constraint::Min(0),
    ])
    .split(area);

    let left_rows = Layout::vertical([
        Constraint::Length(art_box_h),
        Constraint::Min(0),
    ])
    .split(cols[0]);

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
        .border_style(if focused { Style::default().fg(ACCENT) } else { Style::default().fg(DIM) });
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

// ── Search results (three-pane layout) ───────────────────────────────────────

fn render_search_input_line(app: &App) -> Line<'static> {
    let cursor = if (app.tick / 30) % 2 == 0 { "█" } else { " " };
    if app.search.query.is_empty() {
        Line::from(vec![
            Span::styled("Search  ", Style::default().fg(DIM)),
            Span::styled(cursor.to_owned(), Style::default().fg(ACCENT)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Search  ", Style::default().fg(DIM)),
            Span::styled(app.search.query.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(cursor.to_owned(), Style::default().fg(ACCENT)),
        ])
    }
}

fn render_search_modal(f: &mut Frame, app: &App, area: Rect) {
    // Center a search input modal
    let modal_width = 60.min(area.width - 4);
    let modal_height = 3;

    let h_centered = Layout::horizontal([
        Constraint::Length((area.width.saturating_sub(modal_width)) / 2),
        Constraint::Length(modal_width),
        Constraint::Min(0),
    ])
    .split(area);

    let v_centered = Layout::vertical([
        Constraint::Length((area.height.saturating_sub(modal_height)) / 2),
        Constraint::Length(modal_height),
        Constraint::Min(0),
    ])
    .split(h_centered[1]);

    let modal_area = v_centered[1];

    // Render modal background/border
    let block = Block::default()
        .title(" Search ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    // Render search input
    let cursor = if (app.tick / 30) % 2 == 0 { "█" } else { " " };
    let input_text = format!("{}{}", app.search.query, cursor);
    f.render_widget(
        Paragraph::new(input_text)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Left),
        inner,
    );
}

fn render_search_results(f: &mut Frame, app: &App, area: Rect) {
    // Show modal if open
    if app.search.modal_open {
        render_search_modal(f, app, area);
        return;
    }

    // Empty state — no results and not loading
    if app.search.total_results() == 0 && !app.search.loading {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(ACCENT));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let rows = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(inner);

        let content: Line = if app.search.active {
            render_search_input_line(app)
        } else if app.search.query.is_empty() {
            Line::from(Span::styled("Start typing to search", Style::default().fg(DIM)))
        } else {
            Line::from(Span::styled("No results", Style::default().fg(DIM)))
        };
        f.render_widget(
            Paragraph::new(content).alignment(Alignment::Center),
            rows[1],
        );
        return;
    }

    // Loading state
    if app.search.loading {
        let spinner = spinner_char(app.tick);
        let block = Block::default()
            .title(format!(" Searching {spinner} "))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(DIM));
        f.render_widget(block, area);
        return;
    }

    // Results — optionally show live input above results when user is re-searching
    let (input_area, results_area) = if app.search.active {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        (Some(rows[0]), rows[1])
    } else {
        (None, area)
    };

    if let Some(ia) = input_area {
        f.render_widget(
            Paragraph::new(render_search_input_line(app)).alignment(Alignment::Center),
            ia,
        );
    }

    render_search_carousel_tabs(f, app, results_area);
}

fn render_search_carousel_tabs(f: &mut Frame, app: &App, area: Rect) {
    if area.height < 2 {
        return;
    }

    let tabs = vec![
        (format!(" Tracks ({}) ", app.search.tracks.len()), SearchPane::Tracks),
        (format!("Artists ({})", app.search.artists.len()), SearchPane::Artists),
        (format!("Playlists ({}) ", app.search.playlists.len()), SearchPane::Playlists),
    ];

    let mut line_spans = Vec::new();
    for (i, (name, pane)) in tabs.iter().enumerate() {
        if i > 0 {
            line_spans.push(Span::styled(" - ", Style::default().fg(DIM)));
        }

        let selected = app.search.pane == *pane;
        let style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        line_spans.push(Span::styled(name.clone(), style));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Line::from(line_spans));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height > 0 {
        match app.search.pane {
            SearchPane::Tracks => render_search_pane_tracks(f, app, inner),
            SearchPane::Artists => render_search_pane_artists(f, app, inner),
            SearchPane::Playlists => render_search_pane_playlists(f, app, inner),
        }
    }
}

fn render_search_pane_tracks(f: &mut Frame, app: &App, area: Rect) {
    let sel = app.search.track_sel;
    let height = area.height as usize;
    let offset = app.search.track_scroll_offset(height);
    let items: Vec<ListItem> = app.search.tracks
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, t)| {
            let selected = i == sel;
            let is_playing = app.now_playing.track.as_ref().map(|np| np.id == t.id).unwrap_or(false);
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };

            let title_span = Span::styled(
                format!(
                    "{prefix}{playing}{} — {} ({})",
                    t.title, t.all_artist_names(), t.duration_display()
                ),
                style
            );

            let badge = t.quality_badge().map(|b| format!(" [{b}]")).unwrap_or_default();
            let badge_span = Span::styled(badge, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

            let heart = if app.favorite_track_ids.contains(&t.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No tracks").style(Style::default().fg(DIM)).alignment(Alignment::Center),
            area,
        );
    } else {
        f.render_widget(List::new(items), area);
    }
}

fn render_search_pane_artists(f: &mut Frame, app: &App, area: Rect) {
    let sel = app.search.artist_sel;
    let height = area.height as usize;
    let offset = app.search.artist_scroll_offset(height);
    let items: Vec<ListItem> = app.search.artists
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, a)| {
            let selected = i == sel;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let title_span = Span::styled(
                format!("{prefix}{}", a.name),
                style
            );
            let heart = if app.favorite_artist_ids.contains(&a.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };
            ListItem::new(Line::from(vec![title_span, heart]))
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No artists").style(Style::default().fg(DIM)).alignment(Alignment::Center),
            area,
        );
    } else {
        f.render_widget(List::new(items), area);
    }
}

fn render_search_pane_playlists(f: &mut Frame, app: &App, area: Rect) {
    let sel = app.search.playlist_sel;
    let height = area.height as usize;
    let offset = app.search.playlist_scroll_offset(height);
    let items: Vec<ListItem> = app.search.playlists
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, pl)| {
            let selected = i == sel;
            let style = if selected {
                Style::default().bg(HIGHLIGHT_BG).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!("{prefix}{} ({} tracks)", pl.title, pl.number_of_tracks.unwrap_or(0)))
                .style(style)
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No playlists").style(Style::default().fg(DIM)).alignment(Alignment::Center),
            area,
        );
    } else {
        f.render_widget(List::new(items), area);
    }
}


// ── Now playing bar ───────────────────────────────────────────────────────────

fn render_now_playing(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area); // height = 6 (7 - 1 border)
    f.render_widget(block, area);

    let sections = Layout::vertical([
        Constraint::Length(3), // lyrics
        Constraint::Length(1), // spacer
        Constraint::Min(0),    // track info / waveform / time and volume
    ])
    .split(inner);

    render_lyrics(f, app, sections[0]);

    let cols = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(30),
        Constraint::Percentage(35),
    ])
    .split(sections[2]);

    let track_info: Vec<Line> = match &app.now_playing.track {
        Some(t) => {
            let quality_label: Option<String> = {
                let rate_str = app.now_playing.sample_rate.map(fmt_sample_rate);
                // mpv may return "FLAC (Free Lossless Audio Codec)" — take first word only.
                let codec_str = app.now_playing.codec.as_deref().map(|c| {
                    c.split_whitespace().next().unwrap_or(c).to_uppercase()
                });
                match (codec_str, rate_str) {
                    (Some(c), Some(r)) => Some(format!("{c} · {r}")),
                    (Some(c), None)    => Some(c),
                    (None, Some(r))    => Some(r),
                    (None, None)       => {
                        let q = t.quality_display();
                        if q.is_empty() { None } else { Some(q.to_owned()) }
                    }
                }
            };
            let heart = if app.favorite_track_ids.contains(&t.id) { " ❤" } else { "" };
            let mut lines = vec![
                Line::from(Span::styled(format!("{}{}", t.title, heart), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(t.all_artist_names(), Style::default().fg(Color::White))),
                Line::from(Span::styled(t.album.title.as_str(), Style::default().fg(DIM))),
            ];
            if let Some(label) = quality_label {
                lines.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
            }
            lines
        }
        None => vec![
            Line::from(Span::styled("No track playing", Style::default().fg(DIM))),
        ],
    };
    f.render_widget(Paragraph::new(track_info), cols[0]);

    f.render_widget(render_squib(app, cols[1].width), cols[1]);

    let time_str = format!("{} / {}", app.now_playing.position_display(), app.now_playing.duration_display());
    let volume_str = format!("Volume: {}%", app.now_playing.volume);
    f.render_widget(
        Paragraph::new(vec![
                Line::from(time_str),
                Line::from(volume_str),
            ]).alignment(Alignment::Right).style(Style::default().fg(DIM)),
        cols[2],
    );

}

fn render_lyrics(f: &mut Frame, app: &App, area: Rect) {
    let np = &app.now_playing;

    if np.lyrics_loading {
        let spinner = spinner_char(app.tick);
        f.render_widget(
            Paragraph::new(spinner.to_string()).style(Style::default().fg(DIM)).alignment(Alignment::Center),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
        return;
    }

    let lines: &[(f64, String)];
    let plain_buf: Vec<(f64, String)>;

    if !np.lyrics_synced.is_empty() {
        lines = &np.lyrics_synced;
    } else if !np.lyrics_plain.is_empty() {
        // Distribute plain lines evenly across the track duration.
        let n = np.lyrics_plain.len() as f64;
        let dur = if np.duration > 0.0 { np.duration } else { n };
        plain_buf = np.lyrics_plain.iter().enumerate()
            .map(|(i, t)| (i as f64 / n * dur, t.clone()))
            .collect();
        lines = &plain_buf;
    } else {
        return;
    }

    // Find the current line: last one whose timestamp <= playback position.
    let pos = np.position;
    let cur = lines.partition_point(|(t, _)| *t <= pos).saturating_sub(1);

    let show: [Option<usize>; 3] = [
        cur.checked_sub(1),
        Some(cur),
        if cur + 1 < lines.len() { Some(cur + 1) } else { None },
    ];

    for (row, opt) in show.iter().enumerate() {
        if let Some(idx) = opt {
            let (_, text) = &lines[*idx];
            let is_cur = row == 1;
            let style = if is_cur {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            let y = area.y + row as u16;
            if y < area.y + area.height {
                f.render_widget(
                    Paragraph::new(text.as_str()).style(style).alignment(Alignment::Center),
                    Rect::new(area.x, y, area.width, 1),
                );
            }
        }
    }
}

// ── Help hint ─────────────────────────────────────────────────────────────────

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Length(20)])
        .split(area);

    let context_hint = get_context_hint(app);
    let context_span = Span::styled(context_hint, Style::default().fg(DIM));
    f.render_widget(
        Paragraph::new(Line::from(context_span)).alignment(Alignment::Left),
        cols[0],
    );

    let help_span = Span::styled("? show keybinds", Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
    f.render_widget(
        Paragraph::new(Line::from(help_span)).alignment(Alignment::Right),
        cols[1],
    );
}

fn get_context_hint(app: &App) -> String {
    if let Some(View::ArtistDetail(detail)) = app.view_stack.last() {
        match detail.focus {
            ArtistDetailFocus::Tracks => {
                "↑↓ Select | ← → Section | a Add | f Fav | r Radio".to_string()
            }
            ArtistDetailFocus::Albums | ArtistDetailFocus::EPs | ArtistDetailFocus::Singles => {
                "↑↓ Select | ← → Section | f Fav | c Copy".to_string()
            }
            ArtistDetailFocus::Bio => {
                "↑↓ Scroll | ← → Section".to_string()
            }
        }
    } else if let Some(View::AlbumDetail(_)) = app.view_stack.last() {
        "↑↓ Select | a Add | f Fav | r Radio | c Copy".to_string()
    } else if let Some(View::PlaylistDetail(detail)) = app.view_stack.last() {
        match detail.focus {
            PlaylistDetailFocus::Tracks => {
                "↑↓ Select | ← → Section | a Add | f Fav | r Radio | c Copy".to_string()
            }
            PlaylistDetailFocus::Description => {
                "↑↓ Scroll | ← → Section".to_string()
            }
        }
    } else {
        match app.current_tab {
            Tab::Home => "↑↓ Select | ← → Switch section | Enter Open".to_string(),
            Tab::Favorites => "↑↓ Select | a Add | f Fav | r Radio | c Copy".to_string(),
            Tab::Artists => "↑↓ Select | f Follow | Enter Open".to_string(),
            Tab::Albums => "↑↓ Select | f Fav | Enter Open".to_string(),
            Tab::Playlists => "↑↓ Select | Enter Open".to_string(),
            Tab::Search => "↑↓ Select | ← → Section | / Open Search".to_string(),
        }
    }
}

// ── Toast ─────────────────────────────────────────────────────────────────────

fn render_toast(f: &mut Frame, app: &App, area: Rect) {
    let Some((msg, level, set_at)) = &app.status else { return };

    let elapsed = set_at.elapsed().as_secs_f64();
    // Fade out over the last ~1 s of the 5 s lifetime.
    let fading = elapsed > 4.0;

    let (border_color, text_color) = match level {
        StatusLevel::Error => (Color::Red,  if fading { Color::DarkGray } else { Color::White }),
        StatusLevel::Info  => (ACCENT,      if fading { Color::DarkGray } else { Color::White }),
    };

    // Size the card to the message, clamped to the terminal width.
    let inner_w = msg.len() as u16 + 4; // 2 padding each side
    let toast_w = inner_w.min(area.width.saturating_sub(4));
    let toast_h = 3u16;
    let x = area.x + area.width.saturating_sub(toast_w) / 2;
    // Float just above the now-playing bar (last 10 rows).
    let y = area.y + area.height.saturating_sub(toast_h + 10);
    let toast_rect = Rect::new(x, y, toast_w, toast_h);

    f.render_widget(Clear, toast_rect);
    f.render_widget(
        Paragraph::new(msg.as_str())
            .alignment(Alignment::Center)
            .style(Style::default().fg(text_color))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color)),
            ),
        toast_rect,
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────


fn visible_artist_items(
    list: &crate::app::StatefulList<crate::api::models::Artist>,
    height: usize,
) -> Vec<(usize, &crate::api::models::Artist)> {
    let offset = list.scroll_offset(height);
    list.items
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .collect()
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner_char(tick: u64) -> char {
    SPINNER[(tick / 3) as usize % SPINNER.len()]
}

/// Animated waveform squib: undulates while playing, flat line while paused.
/// The played portion is highlighted in ACCENT, the remainder in DIM.
fn render_squib(app: &App, width: u16) -> Paragraph<'static> {
    const WAVE: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

    let ratio = app.now_playing.progress_ratio();
    let played_w = ((width as f64 * ratio) as u16).min(width);
    let playing = app.now_playing.active && !app.now_playing.paused;

    let spans: Vec<Span<'static>> = (0..width)
        .map(|i| {
            let color = if i < played_w { ACCENT } else { DIM };
            let ch: &'static str = if playing {
                // Sine wave: spatial frequency ~1 cycle per 8 cols, phase advances with tick
                let phase = i as f64 * 0.8 + app.tick as f64 * 0.35;
                let t = (phase.sin() + 1.0) / 2.0; // 0.0 – 1.0
                WAVE[(t * 7.99) as usize]
            } else {
                "▄" // flat mid-height line when paused or idle
            };
            Span::styled(ch, Style::default().fg(color))
        })
        .collect();

    Paragraph::new(Line::from(spans))
}

// Initialize picker once at startup to avoid blocking on every frame
fn get_picker() -> &'static Picker {
    static PICKER: std::sync::OnceLock<Picker> = std::sync::OnceLock::new();
    PICKER.get_or_init(|| {
        let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".to_string());
        let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        tracing::info!(
            "Terminal: TERM={}, COLORTERM={} → Image protocol: {:?}",
            term,
            colorterm,
            picker.protocol_type()
        );
        picker
    })
}

fn render_image(f: &mut Frame, bytes: &[u8], area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if let Ok(img) = image::load_from_memory(bytes) {
        let picker = get_picker();
        if let Ok(protocol) = picker.new_protocol(img, area.into(), Resize::Fit(None)) {
            f.render_widget(Image::new(&protocol), area);
        }
    }
}
