// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Compact spectrum renderers and the independent playback progress rail.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::Style,
    widgets::{Paragraph, Widget},
};
use std::{borrow::Cow, time::Duration};

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
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.mode == VisualizerMode::Off {
            return;
        }

        let bands = match self.state {
            SpectrumState::Active(frame) if frame.received_at.elapsed() <= FRAME_STALE_AFTER => {
                Cow::Borrowed(frame.bands.as_slice())
            }
            SpectrumState::Active(frame) => Cow::Owned(vec![0.0; frame.bands.len()]),
            SpectrumState::Starting => {
                render_diagnostic("starting cava", area, buffer);
                return;
            }
            SpectrumState::Unavailable(reason) => {
                let message = match reason {
                    crate::visualizer::UnavailableReason::MissingBinary
                    | crate::visualizer::UnavailableReason::SpawnFailed
                    | crate::visualizer::UnavailableReason::Exited => "cava unavailable",
                };
                render_diagnostic(message, area, buffer);
                return;
            }
            SpectrumState::Disabled => Cow::Borrowed(&[] as &[f32]),
        };

        // Rendering concepts are adapted from CLIamp's MIT-licensed modes; see
        // THIRD_PARTY_NOTICES.md for the source and copyright notice.
        match self.mode {
            VisualizerMode::Bars => render_bars(area, &bands, buffer),
            VisualizerMode::Outline => render_outline(area, &bands, buffer),
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
        let filled_eighths = (level * f32::from(area.height) * 8.0).round() as u16;
        let (start, end) = band_columns(area, index, band_count);
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

fn preferred_band_count(width: u16) -> usize {
    if width == 0 {
        0
    } else {
        usize::from(width.div_ceil(3)).clamp(1, 12)
    }
}

fn band_columns(area: Rect, index: usize, band_count: usize) -> (u16, u16) {
    if band_count == 0 {
        return (area.x, area.x);
    }
    let width = usize::from(area.width);
    let start = index * width / band_count;
    let end = (index + 1) * width / band_count;
    let visible_end = if end.saturating_sub(start) > 1 {
        end - 1
    } else {
        end
    };
    (area.x + start as u16, area.x + visible_end as u16)
}

pub(super) fn resample_bands(input: &[f32], output_len: usize) -> Vec<f32> {
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
    use crate::visualizer::{SpectrumFrame, UnavailableReason};
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
    fn progress_handles_start_middle_end_and_unknown_duration() {
        let area = Rect::new(0, 0, 4, 1);
        for (ratio, expected) in [
            (Some(0.0), "────"),
            (Some(0.5), "━━╸─"),
            (Some(1.0), "━━━━"),
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
            state: &SpectrumState::Unavailable(UnavailableReason::MissingBinary),
        }
        .render(area, &mut unavailable);
        assert!(symbols(&unavailable, area)[1].contains("cava unavailable"));

        let stale_state = SpectrumState::Active(SpectrumFrame {
            bands: vec![1.0; 64],
            received_at: Instant::now() - Duration::from_secs(1),
        });
        let mut stale = Buffer::empty(area);
        Spectrum {
            mode: VisualizerMode::Bars,
            state: &stale_state,
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
        }
        .render(area, &mut off);
        assert_eq!(symbols(&off, area), ["                    "; 3]);
    }
}
