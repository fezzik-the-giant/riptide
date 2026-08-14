// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Compact spectrum renderers and the independent playback progress rail.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Paragraph, Widget},
};
use std::time::Duration;

use super::{ACCENT, DIM};
use crate::visualizer::{SpectrumState, VisualizerMode};

const FRAME_STALE_AFTER: Duration = Duration::from_millis(650);
const BLOCKS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

pub(super) struct ProgressRail {
    pub ratio: Option<f64>,
}

impl Widget for ProgressRail {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let Some(ratio) = self.ratio.filter(|ratio| ratio.is_finite()) else {
            buffer.set_stringn(
                area.x,
                area.y,
                "─".repeat(area.width as usize),
                area.width as usize,
                Style::default().fg(DIM),
            );
            return;
        };

        let ratio = ratio.clamp(0.0, 1.0);
        let played = (ratio * f64::from(area.width)).floor() as u16;
        for offset in 0..area.width {
            let (symbol, color) = if offset < played || ratio == 1.0 {
                ("━", ACCENT)
            } else if offset == played && ratio > 0.0 {
                ("╸", ACCENT)
            } else {
                ("─", DIM)
            };
            if let Some(cell) = buffer.cell_mut((area.x + offset, area.y)) {
                cell.set_symbol(symbol)
                    .set_style(Style::default().fg(color));
            }
        }
    }
}

pub(super) struct Spectrum<'a> {
    pub mode: VisualizerMode,
    pub state: &'a SpectrumState,
    pub tick: u64,
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.mode == VisualizerMode::Off {
            return;
        }

        let bands = match self.state {
            SpectrumState::Active(frame) if frame.received_at.elapsed() <= FRAME_STALE_AFTER => {
                frame.bands.as_slice()
            }
            SpectrumState::Active(_) => &[] as &[f32],
            SpectrumState::Starting => {
                render_diagnostic("starting cava", area, buffer);
                return;
            }
            SpectrumState::Unavailable => {
                render_diagnostic("cava unavailable", area, buffer);
                return;
            }
            SpectrumState::Disabled => &[],
        };

        // Rendering concepts are adapted from CLIamp's MIT-licensed modes; see
        // THIRD_PARTY_NOTICES.md for the source and copyright notice.
        match self.mode {
            VisualizerMode::Bars => render_bars(area, bands, buffer),
            VisualizerMode::Outline => render_outline(area, bands, buffer),
            VisualizerMode::Columns => render_columns(area, bands, buffer),
            VisualizerMode::Bricks => render_bricks(area, bands, buffer),
            VisualizerMode::Dots => render_dots(area, bands, buffer),
            VisualizerMode::Butterfly => render_butterfly(area, bands, self.tick, buffer),
            VisualizerMode::Off => {}
        }
    }
}

fn render_diagnostic(message: &str, area: Rect, buffer: &mut Buffer) {
    let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
    Paragraph::new(message)
        .alignment(Alignment::Center)
        .style(Style::default().fg(DIM))
        .render(row, buffer);
}

fn render_bars(area: Rect, input: &[f32], buffer: &mut Buffer) {
    let band_count = preferred_band_count(area.width);
    let bands = resample_bands(input, band_count);

    for (index, level) in bands.into_iter().enumerate() {
        let (start, end) = band_columns(area, index, band_count);
        render_vertical_band(area, level, start, end, buffer);
    }
}

fn render_outline(area: Rect, input: &[f32], buffer: &mut Buffer) {
    let band_count = preferred_band_count(area.width);
    let bands = resample_bands(input, band_count);

    for (index, level) in bands.into_iter().enumerate() {
        let filled_eighths = (level * f32::from(area.height) * 8.0).round() as u16;
        if filled_eighths == 0 {
            continue;
        }
        let top_cell = (filled_eighths - 1) / 8;
        let cell_fill = (filled_eighths - 1) % 8 + 1;
        let y = area.y + area.height - 1 - top_cell.min(area.height - 1);
        let (start, end) = band_columns(area, index, band_count);
        for x in start..end {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(BLOCKS[cell_fill as usize])
                    .set_style(Style::default().fg(ACCENT));
            }
        }
    }
}

fn render_columns(area: Rect, input: &[f32], buffer: &mut Buffer) {
    let bands = resample_bands(input, usize::from(area.width));
    for (x_offset, level) in bands.into_iter().enumerate() {
        let x = area.x + x_offset as u16;
        render_vertical_band(area, level, x, x + 1, buffer);
    }
}

fn render_vertical_band(area: Rect, level: f32, start: u16, end: u16, buffer: &mut Buffer) {
    let filled_eighths = (level * f32::from(area.height) * 8.0).round() as u16;
    for y_offset in 0..area.height {
        let eighths_below = (area.height - y_offset - 1) * 8;
        let cell_fill = filled_eighths.saturating_sub(eighths_below).min(8);
        if cell_fill == 0 {
            continue;
        }
        for x in start..end {
            if let Some(cell) = buffer.cell_mut((x, area.y + y_offset)) {
                cell.set_symbol(BLOCKS[cell_fill as usize])
                    .set_style(Style::default().fg(ACCENT));
            }
        }
    }
}

fn render_bricks(area: Rect, input: &[f32], buffer: &mut Buffer) {
    let band_count = usize::from(area.width.div_ceil(2));
    let bands = resample_bands(input, band_count);
    for (index, level) in bands.into_iter().enumerate() {
        let filled_rows = (level * f32::from(area.height)).round() as u16;
        let x = area.x + (index * 2) as u16;
        for row in 0..filled_rows.min(area.height) {
            let y = area.bottom() - 1 - row;
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol("▄").set_style(Style::default().fg(ACCENT));
            }
        }
    }
}

fn render_dots(area: Rect, input: &[f32], buffer: &mut Buffer) {
    let dot_width = usize::from(area.width) * 2;
    let dot_height = usize::from(area.height) * 4;
    if dot_width == 0 || dot_height == 0 {
        return;
    }

    let band_count = if area.width < 4 {
        usize::from(area.width)
    } else {
        dot_width.div_ceil(3)
    };
    let bands = resample_bands(input, band_count);
    let mut dots = vec![false; dot_width * dot_height];
    for (index, level) in bands.into_iter().enumerate() {
        let columns = band_span(dot_width, index, band_count, area.width >= 4);
        let filled = (level * dot_height as f32).round() as usize;
        for y in dot_height.saturating_sub(filled.min(dot_height))..dot_height {
            for x in columns.clone() {
                dots[y * dot_width + x] = true;
            }
        }
    }
    render_dot_grid(area, &dots, dot_width, buffer);
}

fn render_butterfly(area: Rect, input: &[f32], tick: u64, buffer: &mut Buffer) {
    let (dots, dot_width) = butterfly_grid(area, input, tick);
    render_dot_grid(area, &dots, dot_width, buffer);
}

fn butterfly_grid(area: Rect, input: &[f32], tick: u64) -> (Vec<bool>, usize) {
    let dot_width = usize::from(area.width) * 2;
    let dot_height = usize::from(area.height) * 4;
    if dot_width == 0 || dot_height == 0 {
        return (Vec::new(), dot_width);
    }

    let levels = resample_bands(input, dot_height);
    let half_width = dot_width / 2;
    let mut dots = vec![false; dot_width * dot_height];
    for y in 0..dot_height {
        let level = levels.get(y).copied().unwrap_or(0.0);
        let base_width = (level * half_width as f32).round() as isize;
        let wobble = (deterministic_hash(y as u64, tick) % 3) as isize - 1;
        let wing_width = (base_width + wobble).clamp(1, half_width as isize) as usize;

        for distance in 0..wing_width {
            let boundary = distance == 0 || distance + 1 == wing_width;
            let visible = boundary
                || deterministic_hash(distance as u64, y as u64 ^ tick).trailing_zeros() < 2;
            if visible {
                dots[y * dot_width + half_width - 1 - distance] = true;
                dots[y * dot_width + half_width + distance] = true;
            }
        }
    }
    (dots, dot_width)
}

fn deterministic_hash(first: u64, second: u64) -> u64 {
    let mut value = first.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ second;
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value.wrapping_mul(0x94d0_49bb_1331_11eb) ^ (value >> 31)
}

fn render_dot_grid(area: Rect, dots: &[bool], dot_width: usize, buffer: &mut Buffer) {
    if dot_width == 0 {
        return;
    }
    for cell_y in 0..usize::from(area.height) {
        for cell_x in 0..usize::from(area.width) {
            let symbol = braille_char(cell_x * 2, cell_y * 4, |x, y| {
                dots.get(y * dot_width + x).copied().unwrap_or(false)
            });
            if symbol != '\u{2800}'
                && let Some(cell) =
                    buffer.cell_mut((area.x + cell_x as u16, area.y + cell_y as u16))
            {
                cell.set_char(symbol).set_style(Style::default().fg(ACCENT));
            }
        }
    }
}

fn braille_char<F>(cell_x: usize, cell_y: usize, is_set: F) -> char
where
    F: Fn(usize, usize) -> bool,
{
    const BITS: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
    let mut bits = 0;
    for (y, row) in BITS.iter().enumerate() {
        for (x, bit) in row.iter().enumerate() {
            if is_set(cell_x + x, cell_y + y) {
                bits |= bit;
            }
        }
    }
    char::from_u32(0x2800 + bits).expect("Braille bit mapping is a valid Unicode scalar")
}

fn preferred_band_count(width: u16) -> usize {
    if width == 0 {
        0
    } else {
        usize::from(width.div_ceil(3)).clamp(1, 12)
    }
}

fn band_columns(area: Rect, index: usize, band_count: usize) -> (u16, u16) {
    let columns = band_span(usize::from(area.width), index, band_count, true);
    (area.x + columns.start as u16, area.x + columns.end as u16)
}

fn band_span(
    width: usize,
    index: usize,
    band_count: usize,
    leave_trailing_gap: bool,
) -> std::ops::Range<usize> {
    if band_count == 0 {
        return 0..0;
    }
    let start = index * width / band_count;
    let end = (index + 1) * width / band_count;
    let visible_end = if leave_trailing_gap && end.saturating_sub(start) > 1 {
        end - 1
    } else {
        end
    };
    start..visible_end
}

fn resample_bands(input: &[f32], output_len: usize) -> Vec<f32> {
    if input.is_empty() || output_len == 0 {
        return Vec::new();
    }
    if input.len() == output_len {
        return input.iter().map(|value| value.clamp(0.0, 1.0)).collect();
    }

    if output_len < input.len() {
        let scale = input.len() as f64 / output_len as f64;
        return (0..output_len)
            .map(|output_index| {
                let start = output_index as f64 * scale;
                let end = (output_index + 1) as f64 * scale;
                let first = start.floor() as usize;
                let last = end.ceil() as usize;
                let mut weighted_sum = 0.0f64;
                for (input_index, value) in input
                    .iter()
                    .enumerate()
                    .take(last.min(input.len()))
                    .skip(first)
                {
                    let overlap_start = start.max(input_index as f64);
                    let overlap_end = end.min((input_index + 1) as f64);
                    let weight = (overlap_end - overlap_start).max(0.0);
                    weighted_sum += f64::from(value.clamp(0.0, 1.0)) * weight;
                }
                (weighted_sum / scale).clamp(0.0, 1.0) as f32
            })
            .collect();
    }

    if input.len() == 1 {
        return vec![input[0].clamp(0.0, 1.0); output_len];
    }

    (0..output_len)
        .map(|output_index| {
            let position = output_index as f64 * (input.len() - 1) as f64 / (output_len - 1) as f64;
            let left = position.floor() as usize;
            let right = position.ceil() as usize;
            let fraction = (position - left as f64) as f32;
            let left_value = input[left].clamp(0.0, 1.0);
            let right_value = input[right].clamp(0.0, 1.0);
            (left_value + (right_value - left_value) * fraction).clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visualizer::SpectrumFrame;
    use std::time::Instant;

    fn symbols(buffer: &Buffer, area: Rect) -> Vec<String> {
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn resampling_handles_empty_identity_and_zero_output() {
        assert!(resample_bands(&[], 3).is_empty());
        assert!(resample_bands(&[0.5], 0).is_empty());
        assert_eq!(resample_bands(&[0.0, 0.5, 1.0], 3), vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn downsampling_uses_area_averages() {
        assert_eq!(resample_bands(&[0.0, 1.0, 0.0, 1.0], 2), vec![0.5, 0.5]);
        assert_eq!(resample_bands(&[0.0, 1.0, 0.0], 1), vec![1.0 / 3.0]);
    }

    #[test]
    fn upsampling_interpolates_and_clamps() {
        assert_eq!(resample_bands(&[0.0, 1.0], 3), vec![0.0, 0.5, 1.0]);
        assert_eq!(resample_bands(&[-1.0, 2.0], 3), vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn zero_sized_and_narrow_spectrums_do_not_panic() {
        for area in [
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 0, 3),
            Rect::new(0, 0, 1, 1),
        ] {
            let mut buffer = Buffer::empty(area);
            Spectrum {
                mode: VisualizerMode::Bars,
                state: &SpectrumState::Active(SpectrumFrame {
                    bands: vec![1.0],
                    received_at: Instant::now(),
                }),
                tick: 0,
            }
            .render(area, &mut buffer);
        }
    }

    #[test]
    fn spectrum_does_not_write_outside_its_area() {
        let buffer_area = Rect::new(0, 0, 8, 5);
        let render_area = Rect::new(2, 1, 3, 3);
        let mut buffer = Buffer::empty(buffer_area);
        let state = SpectrumState::Active(SpectrumFrame {
            bands: vec![1.0; 64],
            received_at: Instant::now(),
        });
        Spectrum {
            mode: VisualizerMode::Bars,
            state: &state,
            tick: 0,
        }
        .render(render_area, &mut buffer);

        for y in buffer_area.y..buffer_area.bottom() {
            for x in buffer_area.x..buffer_area.right() {
                if !render_area.contains((x, y).into()) {
                    assert_eq!(buffer[(x, y)].symbol(), " ");
                }
            }
        }
    }

    #[test]
    fn bars_grow_from_the_bottom() {
        let area = Rect::new(0, 0, 3, 3);
        let mut buffer = Buffer::empty(area);
        render_bars(area, &[0.2], &mut buffer);
        let rows = symbols(&buffer, area);
        assert_eq!(rows[0], "   ");
        assert_eq!(rows[1], "   ");
        assert_ne!(rows[2], "   ");
    }

    #[test]
    fn outline_only_draws_the_current_edge() {
        let area = Rect::new(0, 0, 3, 3);
        let mut buffer = Buffer::empty(area);
        render_outline(area, &[0.5], &mut buffer);
        let rows = symbols(&buffer, area);
        assert_eq!(rows[0], "   ");
        assert_ne!(rows[1], "   ");
        assert_eq!(rows[2], "   ");
    }

    #[test]
    fn columns_are_dense_and_bricks_have_gaps() {
        let area = Rect::new(0, 0, 6, 3);
        let mut columns = Buffer::empty(area);
        render_columns(area, &[1.0; 64], &mut columns);
        assert_eq!(symbols(&columns, area), ["██████"; 3]);

        let mut bricks = Buffer::empty(area);
        render_bricks(area, &[1.0; 64], &mut bricks);
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let expected = if x % 2 == 0 { "▄" } else { " " };
                assert_eq!(bricks[(x, y)].symbol(), expected);
            }
        }
    }

    #[test]
    fn braille_mapping_uses_standard_dot_order() {
        for (point, expected) in [
            ((0, 0), '\u{2801}'),
            ((0, 1), '\u{2802}'),
            ((0, 2), '\u{2804}'),
            ((0, 3), '\u{2840}'),
            ((1, 0), '\u{2808}'),
            ((1, 1), '\u{2810}'),
            ((1, 2), '\u{2820}'),
            ((1, 3), '\u{2880}'),
        ] {
            assert_eq!(braille_char(0, 0, |x, y| (x, y) == point), expected);
        }
    }

    #[test]
    fn dots_fill_from_the_bottom() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        render_dots(area, &[0.25], &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "⣀");
    }

    #[test]
    fn butterfly_is_symmetric_deterministic_and_tick_driven() {
        let area = Rect::new(0, 0, 8, 3);
        let bands = [1.0; 64];
        let (first, width) = butterfly_grid(area, &bands, 0);
        let (repeat, _) = butterfly_grid(area, &bands, 0);
        let (later, _) = butterfly_grid(area, &bands, 7);
        assert_eq!(first, repeat);
        assert_ne!(first, later);

        for y in 0..usize::from(area.height) * 4 {
            for x in 0..width {
                assert_eq!(first[y * width + x], first[y * width + width - 1 - x]);
            }
        }
    }

    #[test]
    fn every_mode_is_bounded_and_deterministic_at_pathological_sizes() {
        let buffer_area = Rect::new(0, 0, 36, 6);
        let modes = [
            VisualizerMode::Off,
            VisualizerMode::Bars,
            VisualizerMode::Outline,
            VisualizerMode::Columns,
            VisualizerMode::Bricks,
            VisualizerMode::Dots,
            VisualizerMode::Butterfly,
        ];
        let sizes = [(0, 0), (0, 3), (1, 1), (1, 3), (2, 1), (8, 3), (30, 3)];
        let patterns = [
            vec![0.0; 64],
            vec![1.0; 64],
            (0..64)
                .map(|index| if index % 2 == 0 { 0.0 } else { 1.0 })
                .collect(),
        ];

        for mode in modes {
            for (width, height) in sizes {
                let area = Rect::new(2, 1, width, height);
                for bands in &patterns {
                    let state = SpectrumState::Active(SpectrumFrame {
                        bands: bands.clone(),
                        received_at: Instant::now(),
                    });
                    let mut first = Buffer::empty(buffer_area);
                    Spectrum {
                        mode,
                        state: &state,
                        tick: 11,
                    }
                    .render(area, &mut first);
                    let mut second = Buffer::empty(buffer_area);
                    Spectrum {
                        mode,
                        state: &state,
                        tick: 11,
                    }
                    .render(area, &mut second);
                    assert_eq!(first, second, "mode={mode:?}, area={area:?}");

                    for y in buffer_area.y..buffer_area.bottom() {
                        for x in buffer_area.x..buffer_area.right() {
                            if !area.contains((x, y).into()) {
                                assert_eq!(first[(x, y)].symbol(), " ");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn progress_handles_start_middle_end_and_unknown_duration() {
        let area = Rect::new(0, 0, 4, 1);
        for (ratio, expected) in [
            (Some(0.0), "────"),
            (Some(0.5), "━━╸─"),
            (Some(1.0), "━━━━"),
            (Some(f64::NAN), "────"),
            (None, "────"),
        ] {
            let mut buffer = Buffer::empty(area);
            ProgressRail { ratio }.render(area, &mut buffer);
            assert_eq!(symbols(&buffer, area), [expected]);
        }

        let zero_area = Rect::new(0, 0, 0, 0);
        ProgressRail { ratio: Some(0.5) }.render(zero_area, &mut Buffer::empty(zero_area));
    }

    #[test]
    fn unavailable_and_stale_states_have_deterministic_fallbacks() {
        let area = Rect::new(0, 0, 20, 3);
        let mut unavailable = Buffer::empty(area);
        Spectrum {
            mode: VisualizerMode::Bars,
            state: &SpectrumState::Unavailable,
            tick: 0,
        }
        .render(area, &mut unavailable);
        assert!(symbols(&unavailable, area)[1].contains("cava unavailable"));

        let mut starting = Buffer::empty(area);
        Spectrum {
            mode: VisualizerMode::Bars,
            state: &SpectrumState::Starting,
            tick: 0,
        }
        .render(area, &mut starting);
        assert!(symbols(&starting, area)[1].contains("starting cava"));

        let stale_state = SpectrumState::Active(SpectrumFrame {
            bands: vec![1.0; 64],
            received_at: Instant::now() - Duration::from_secs(1),
        });
        let mut stale = Buffer::empty(area);
        Spectrum {
            mode: VisualizerMode::Bars,
            state: &stale_state,
            tick: 0,
        }
        .render(area, &mut stale);
        assert_eq!(symbols(&stale, area), ["                    "; 3]);

        let mut off = Buffer::empty(area);
        Spectrum {
            mode: VisualizerMode::Off,
            state: &SpectrumState::Active(SpectrumFrame {
                bands: vec![1.0; 64],
                received_at: Instant::now(),
            }),
            tick: 0,
        }
        .render(area, &mut off);
        assert_eq!(symbols(&off, area), ["                    "; 3]);
    }

    #[test]
    fn band_spans_share_partitioning_and_apply_gaps_only_when_requested() {
        assert_eq!(band_span(12, 0, 4, true), 0..2);
        assert_eq!(band_span(12, 0, 4, false), 0..3);
        assert_eq!(band_span(1, 0, 1, true), 0..1);
        assert_eq!(band_span(10, 0, 0, true), 0..0);
        assert_eq!(preferred_band_count(100), 12);
    }
}
