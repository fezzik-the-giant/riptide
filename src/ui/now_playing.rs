// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! The now-playing bar: art, lyrics, track info and playback visualization.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;
use super::*;

pub(super) fn render_now_playing(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area); // height = 6 (7 - 1 border)
    f.render_widget(block, area);

    let sections = Layout::vertical([
        Constraint::Min(0),    // art (see below — it also spans the lyrics rows)
        Constraint::Length(1), // gap
        Constraint::Length(3), // lyrics — sits directly above the waveform row
        Constraint::Length(4), // track info / waveform / time and volume
    ])
    .split(inner);

    let cols = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(30),
        Constraint::Percentage(35),
    ])
    .split(sections[3]);

    // The art fills everything above the track info bar one blank line. It
    // deliberately runs past its own section and down alongside the lyrics:
    // lyrics are centred while the art is left-aligned, so a tall cover sits
    // beside them rather than under them. Stopping the art at its section
    // instead would leave the lyrics' full height as dead space in this column.
    // Width is twice the height because terminal cells are about half as wide as
    // tall, which keeps the cover square.
    let art_h = sections[3].y.saturating_sub(inner.y).saturating_sub(1);
    let art_w = (art_h * 2).min(cols[0].width);
    render_now_playing_art(f, app, Rect::new(inner.x, inner.y, art_w, art_h));

    // Lyrics are centred across whatever rect they're given, so a long line
    // would run left into the cover. Inset both sides by the art's width to keep
    // them clear of it while staying centred on the bar. Narrow terminals fall
    // back to the full width — overlap is better than a two-word column.
    let inset = art_w + 1;
    let lyrics_area = if inner.width > inset * 2 + 20 {
        Rect::new(
            inner.x + inset,
            sections[2].y,
            inner.width - inset * 2,
            sections[2].height,
        )
    } else {
        sections[2]
    };
    render_lyrics(f, app, lyrics_area);

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

    render_center(f, app, cols[1]);

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

pub(super) fn render_now_playing_art(f: &mut Frame, app: &App, area: Rect) {
    let np = &app.now_playing;
    if area.width == 0 || area.height == 0 {
        return;
    }

    if let Some(bytes) = &np.art_bytes {
        render_image(f, bytes, area);
    } else if np.art_loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            area,
        );
    }
}

pub(super) fn render_lyrics(f: &mut Frame, app: &App, area: Rect) {
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

fn render_center(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let ratio = (app.now_playing.active && app.now_playing.duration > 0.0)
        .then(|| app.now_playing.progress_ratio());
    if app.visualizer_mode == crate::visualizer::VisualizerMode::Off {
        let rail = Rect::new(
            area.x,
            area.y + area.height.saturating_sub(1) / 2,
            area.width,
            1,
        );
        f.render_widget(ProgressRail { ratio }, rail);
        return;
    }

    let rail = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let spectrum_area = Rect::new(area.x, area.y, area.width, area.height - 1);
    let spectrum_state = app.spectrum_rx.borrow();
    f.render_widget(
        Spectrum {
            mode: app.visualizer_mode,
            state: &spectrum_state,
        },
        spectrum_area,
    );
    f.render_widget(ProgressRail { ratio }, rail);
}
