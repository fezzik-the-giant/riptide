// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The now-playing bar: art, lyrics, track info and playback visualization.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::*;
use crate::app::App;
use crate::visualizer::VisualizerMode;

/// Below this many columns beside the cover, lyrics take the full width instead.
const LYRICS_MIN_W: u16 = 20;
const LYRICS_GAP: u16 = 3;

pub(super) fn render_now_playing(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let sections = Layout::vertical([
        Constraint::Min(0),    // art, with the lyrics beside it
        Constraint::Length(1), // gap
        Constraint::Length(4), // track info / visualization / time and volume
    ])
    .split(inner);

    let cols = Layout::horizontal([
        Constraint::Percentage(35),
        Constraint::Percentage(30),
        Constraint::Percentage(35),
    ])
    .split(sections[2]);

    // Width is twice the height because terminal cells are about half as wide as
    // tall, which keeps the cover square.
    let top = sections[0];
    let art_w = (top.height * 2).min(cols[0].width);
    render_now_playing_art(f, app, Rect::new(top.x, top.y, art_w, top.height));

    // Narrow terminals fall back to the full width — overlap is better than a
    // two-word column.
    let inset = art_w + LYRICS_GAP;
    let lyrics_area = if top.width > inset + LYRICS_MIN_W {
        Rect::new(top.x + inset, top.y, top.width - inset, top.height)
    } else {
        top
    };
    render_lyrics(f, app, lyrics_area);

    let track_info: Vec<Line> = match &app.now_playing.track {
        Some(t) => {
            let quality_label: Option<String> = {
                // mpv reports the rate a moment after playback starts; Tidal states
                // it up front, so the label is populated immediately either way.
                let rate_str = app
                    .now_playing
                    .sample_rate
                    .or_else(|| {
                        app.now_playing
                            .delivered
                            .sample_rate
                            .and_then(|r| u32::try_from(r).ok())
                    })
                    .map(fmt_sample_rate);
                // mpv may return "FLAC (Free Lossless Audio Codec)" — take first word only.
                let codec_str = app
                    .now_playing
                    .codec
                    .as_deref()
                    .map(|c| c.split_whitespace().next().unwrap_or(c).to_uppercase());
                // Bit depth comes from Tidal rather than mpv: mpv reports the
                // decoder's output format (24-bit FLAC decodes to s32), which is not
                // the depth of the stream. This is the one figure that distinguishes
                // a MAX release actually served in hi-res from the same release
                // served as 16-bit lossless.
                let depth_str = app
                    .now_playing
                    .delivered
                    .bit_depth
                    .map(|d| format!("{d}-bit"));
                let parts: Vec<String> = [codec_str, depth_str, rate_str]
                    .into_iter()
                    .flatten()
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(" · "))
                }
            };
            let heart = if app.favorite_track_ids.contains(&t.id) {
                " ❤"
            } else {
                ""
            };
            let mut lines = vec![
                Line::from(Span::styled(
                    format!("{}{}", t.title, heart),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    t.all_artist_names(),
                    Style::default().fg(Color::White),
                )),
                Line::from(Span::styled(
                    t.album.title.as_str(),
                    Style::default().fg(DIM),
                )),
            ];
            if let Some(label) = quality_label {
                lines.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "No track playing",
            Style::default().fg(DIM),
        ))],
    };
    f.render_widget(Paragraph::new(track_info), cols[0]);

    render_progress(f, app, cols[1]);

    let time_str = format!(
        "{} / {}",
        app.now_playing.position_display(),
        app.now_playing.duration_display()
    );
    let volume_str = format!("Volume: {}%", app.now_playing.volume);
    f.render_widget(
        Paragraph::new(vec![Line::from(time_str), Line::from(volume_str)])
            .alignment(Alignment::Right)
            .style(Style::default().fg(DIM)),
        cols[2],
    );
}

pub(super) fn render_now_playing_art(f: &mut Frame, app: &App, area: Rect) {
    let np = &app.now_playing;
    if area.width == 0 || area.height == 0 {
        return;
    }

    if let Some(bytes) = np.art_bytes() {
        if !render_image(f, bytes, area) {
            f.render_widget(
                Paragraph::new("No artwork")
                    .style(Style::default().fg(DIM))
                    .alignment(Alignment::Center),
                area,
            );
        }
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
            Paragraph::new(spinner.to_string()).style(Style::default().fg(DIM)),
            Rect::new(area.x, area.y + area.height / 2, area.width, 1),
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
        plain_buf = np
            .lyrics_plain
            .iter()
            .enumerate()
            .map(|(i, t)| (i as f64 / n * dur, t.clone()))
            .collect();
        lines = &plain_buf;
    } else {
        return;
    }

    // Find the current line: last one whose timestamp <= playback position.
    let pos = np.position;
    let cur = lines.partition_point(|(t, _)| *t <= pos).saturating_sub(1);

    let current = wrap_in_two(&lines[cur].1, area.width);
    let cur_top = area.y + area.height.saturating_sub(current.len() as u16) / 2;
    let cur_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let context_style = Style::default().fg(DIM);
    let row = |y: u16| Rect::new(area.x, y, area.width, 1);

    for (i, text) in current.iter().enumerate() {
        let y = cur_top + i as u16;
        if y < area.bottom() {
            f.render_widget(lyric_line(text, cur_style), row(y));
        }
    }
    for (y, (_, text)) in (area.y..cur_top).rev().zip(lines[..cur].iter().rev()) {
        f.render_widget(
            lyric_line(&ellipsize(text, area.width), context_style),
            row(y),
        );
    }
    let below = cur_top + current.len() as u16;
    for (y, (_, text)) in (below..area.bottom()).zip(&lines[cur + 1..]) {
        f.render_widget(
            lyric_line(&ellipsize(text, area.width), context_style),
            row(y),
        );
    }
}

fn lyric_line(text: &str, style: Style) -> Paragraph<'_> {
    Paragraph::new(text).style(style)
}

/// Fit the current lyric line in at most two rows of `columns`, breaking at the
/// last space that fits. Context lines are only ellipsized, but this is the one
/// being sung, so it is worth a second row before anything is cut.
fn wrap_in_two(text: &str, columns: u16) -> Vec<String> {
    let columns = columns as usize;
    if text.width() <= columns {
        return vec![text.to_string()];
    }
    let mut used = 0usize;
    let mut fit_end = 0usize;
    let mut last_space = None;
    for (i, ch) in text.char_indices() {
        used += ch.width().unwrap_or(0);
        if used > columns {
            break;
        }
        if ch == ' ' {
            last_space = Some(i);
        }
        fit_end = i + ch.len_utf8();
    }
    let (first, rest) = match last_space {
        Some(i) => (&text[..i], &text[i + 1..]),
        None => text.split_at(fit_end),
    };
    vec![
        first.to_string(),
        ellipsize(rest.trim_start(), columns as u16),
    ]
}

/// The progress rail, with the spectrum stacked above it when a visualizer
/// mode is on. With it off the rail is centred, so the row does not read as
/// an empty band waiting for something to appear.
fn render_progress(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let ratio = (app.now_playing.active && app.now_playing.duration > 0.0)
        .then(|| app.now_playing.progress_ratio());
    let rail_y = if app.visualizer_mode == VisualizerMode::Off {
        area.y + area.height.saturating_sub(1) / 2
    } else {
        f.render_widget(
            Spectrum {
                mode: app.visualizer_mode,
                state: &app.visualizer.spectrum_rx.borrow(),
                tick: app.tick,
            },
            Rect::new(area.x, area.y, area.width, area.height - 1),
        );
        area.bottom() - 1
    };
    f.render_widget(
        ProgressRail { ratio },
        Rect::new(area.x, rail_y, area.width, 1),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_line_wraps_at_a_word_then_ellipsizes() {
        assert_eq!(wrap_in_two("short", 10), ["short"]);
        assert_eq!(
            wrap_in_two("killing myself again and again", 12),
            ["killing", "myself agai…"]
        );
        assert_eq!(wrap_in_two("無限無限無限", 4), ["無限", "無…"]);
    }
}
