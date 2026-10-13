// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Optional CAVA spectrum capture and the state shared with the terminal UI.
//!
//! CAVA runs as a child process rather than a linked library: it already owns
//! system-output capture, the FFT and smoothing, and cavacore alone expects the
//! caller to supply PCM. Nothing here touches mpv, so a capture failure can only
//! ever blank the spectrum, never interrupt playback.

use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::watch,
};

/// Raw ASCII output, so a frame is one `;`-separated line `parse_frame` can read
/// without knowing CAVA's binary layout. Written to a private file so the
/// user's own `~/.config/cava/config` is never read or touched.
const CAVA_CONFIG: &str = r#"[general]
framerate = 30
bars = 64
autosens = 1
scaling = linear
lower_cutoff_freq = 50
higher_cutoff_freq = 10000
sleep_timer = 0

[input]
source = auto

[output]
method = raw
channels = mono
mono_option = average
raw_target = /dev/stdout
data_format = ascii
ascii_max_range = 1000
bar_delimiter = 59
frame_delimiter = 10
"#;

const ASCII_MAX_RANGE: f32 = 1000.0;

/// CAVA prints nothing until its input opens, so silence this long means the
/// capture source never came up rather than a quiet passage.
const FRAME_TIMEOUT: Duration = Duration::from_secs(3);

/// Tolerates a torn line or two while the pipe fills, without spinning forever
/// on a CAVA build whose output format has changed under us.
const MAX_CONSECUTIVE_MALFORMED_FRAMES: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualizerMode {
    Bars,
    Outline,
    Columns,
    Bricks,
    Dots,
    Butterfly,
    /// `other` so a config written by a newer version with a mode this one
    /// lacks still loads, rather than failing the whole preferences block.
    #[default]
    #[serde(other)]
    Off,
}

impl VisualizerMode {
    const CYCLE: [VisualizerMode; 7] = [
        Self::Off,
        Self::Bars,
        Self::Outline,
        Self::Columns,
        Self::Bricks,
        Self::Dots,
        Self::Butterfly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bars => "Bars",
            Self::Outline => "Outline",
            Self::Columns => "Columns",
            Self::Bricks => "Bricks",
            Self::Dots => "Dots",
            Self::Butterfly => "Butterfly",
        }
    }

    pub fn next(self) -> Self {
        self.step(1)
    }

    pub fn previous(self) -> Self {
        self.step(Self::CYCLE.len() - 1)
    }

    fn step(self, by: usize) -> Self {
        let at = Self::CYCLE.iter().position(|&m| m == self).unwrap_or(0);
        Self::CYCLE[(at + by) % Self::CYCLE.len()]
    }
}

#[derive(Debug, Clone)]
pub struct SpectrumFrame {
    pub bands: Vec<f32>,
    pub received_at: Instant,
}

#[derive(Debug, Clone, Default)]
pub enum SpectrumState {
    #[default]
    Disabled,
    Starting,
    Active(SpectrumFrame),
    Unavailable,
}

/// The UI's ends of the capture worker's channels.
pub struct VisualizerLink {
    pub enabled_tx: watch::Sender<bool>,
    pub spectrum_rx: watch::Receiver<SpectrumState>,
}

impl VisualizerLink {
    /// A link with the worker's ends handed to the test instead.
    #[cfg(test)]
    pub fn for_test() -> (Self, watch::Receiver<bool>, watch::Sender<SpectrumState>) {
        let (enabled_tx, enabled_rx) = watch::channel(false);
        let (spectrum_tx, spectrum_rx) = watch::channel(SpectrumState::Disabled);
        (
            Self {
                enabled_tx,
                spectrum_rx,
            },
            enabled_rx,
            spectrum_tx,
        )
    }
}

/// Builds the link and the worker that answers it. Capture starts at once if
/// the saved mode is anything but Off.
pub fn channel(mode: VisualizerMode) -> (VisualizerLink, CavaWorker) {
    let (enabled_tx, enabled_rx) = watch::channel(mode != VisualizerMode::Off);
    let (spectrum_tx, spectrum_rx) = watch::channel(SpectrumState::Disabled);
    (
        VisualizerLink {
            enabled_tx,
            spectrum_rx,
        },
        CavaWorker {
            enabled_rx,
            spectrum_tx,
        },
    )
}

pub struct CavaWorker {
    enabled_rx: watch::Receiver<bool>,
    spectrum_tx: watch::Sender<SpectrumState>,
}

enum SessionEnd {
    Disabled,
    SenderClosed,
    Unavailable,
}

impl CavaWorker {
    pub async fn run(mut self) {
        loop {
            if self.enabled_rx.wait_for(|enabled| *enabled).await.is_err() {
                return;
            }

            self.publish(SpectrumState::Starting);
            match self.run_session().await {
                SessionEnd::Disabled => self.publish(SpectrumState::Disabled),
                SessionEnd::SenderClosed => return,
                // Wait for the user to ask again (any mode change re-sends
                // `true`) instead of respawning a CAVA that just failed.
                SessionEnd::Unavailable => {
                    self.publish(SpectrumState::Unavailable);
                    if self.enabled_rx.changed().await.is_err() {
                        return;
                    }
                    if !*self.enabled_rx.borrow_and_update() {
                        self.publish(SpectrumState::Disabled);
                    }
                }
            }
        }
    }

    async fn run_session(&mut self) -> SessionEnd {
        let config_path = cava_config_path();
        if let Err(error) = tokio::fs::write(&config_path, CAVA_CONFIG).await {
            tracing::warn!(path = %config_path.display(), %error, "failed to write CAVA config");
            return SessionEnd::Unavailable;
        }

        let mut child = match Command::new("cava")
            .arg("-p")
            .arg(&config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(%error, "failed to start CAVA");
                remove_config(&config_path).await;
                return SessionEnd::Unavailable;
            }
        };
        tracing::info!("CAVA started");

        let stdout = child.stdout.take().expect("stdout is piped");
        // Drained concurrently so a chatty CAVA cannot fill the pipe and stall;
        // only the last line is kept, as the likeliest explanation of a failure.
        let stderr_task = child.stderr.take().map(|stderr| {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                let mut last_line = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.trim().is_empty() {
                        last_line = Some(line);
                    }
                }
                last_line
            })
        });

        let mut lines = BufReader::new(stdout).lines();
        let frame_timeout = tokio::time::sleep(FRAME_TIMEOUT);
        tokio::pin!(frame_timeout);
        let mut malformed_frames = 0;

        let end = loop {
            tokio::select! {
                changed = self.enabled_rx.changed() => match changed {
                    Err(_) => break SessionEnd::SenderClosed,
                    Ok(()) if !*self.enabled_rx.borrow_and_update() => break SessionEnd::Disabled,
                    Ok(()) => {}
                },
                line = lines.next_line() => match line {
                    Ok(Some(line)) => match parse_frame(&line) {
                        Some(bands) => {
                            malformed_frames = 0;
                            frame_timeout
                                .as_mut()
                                .reset(tokio::time::Instant::now() + FRAME_TIMEOUT);
                            self.publish(SpectrumState::Active(SpectrumFrame {
                                bands,
                                received_at: Instant::now(),
                            }));
                        }
                        None => {
                            malformed_frames += 1;
                            if malformed_frames >= MAX_CONSECUTIVE_MALFORMED_FRAMES {
                                tracing::warn!(
                                    malformed_frames,
                                    "CAVA repeatedly produced unreadable output"
                                );
                                break SessionEnd::Unavailable;
                            }
                        }
                    },
                    Ok(None) => {
                        tracing::warn!("CAVA closed its output");
                        break SessionEnd::Unavailable;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to read CAVA output");
                        break SessionEnd::Unavailable;
                    }
                },
                () = &mut frame_timeout => {
                    tracing::warn!(
                        timeout_seconds = FRAME_TIMEOUT.as_secs(),
                        "timed out waiting for CAVA output"
                    );
                    break SessionEnd::Unavailable;
                }
            }
        };

        terminate_child(&mut child).await;
        if let Some(task) = stderr_task
            && let Ok(Some(message)) = task.await
            && matches!(end, SessionEnd::Unavailable)
        {
            tracing::warn!(%message, "CAVA stopped");
        }
        remove_config(&config_path).await;
        end
    }

    fn publish(&self, state: SpectrumState) {
        let _ = self.spectrum_tx.send(state);
    }
}

/// Per-process so two riptide instances never share, or delete, one file.
fn cava_config_path() -> PathBuf {
    std::env::temp_dir().join(format!("riptide-cava-{}.conf", std::process::id()))
}

async fn terminate_child(child: &mut Child) {
    // `InvalidInput` means it had already exited, which is the normal case
    // when the session ended because CAVA closed its stdout.
    if let Err(error) = child.kill().await
        && error.kind() != io::ErrorKind::InvalidInput
    {
        tracing::debug!(%error, "failed to terminate CAVA");
    }
    match child.wait().await {
        Ok(status) => tracing::debug!(%status, "CAVA exited"),
        Err(error) => tracing::debug!(%error, "failed to reap CAVA"),
    }
}

async fn remove_config(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(path = %path.display(), %error, "failed to remove CAVA config");
    }
}

fn parse_frame(line: &str) -> Option<Vec<f32>> {
    let trimmed = line.trim();
    let frame = trimmed.strip_suffix(';').unwrap_or(trimmed);
    if frame.is_empty() {
        return None;
    }
    frame
        .split(';')
        .map(|token| {
            let value = token.trim().parse::<i64>().ok()?;
            Some(value.clamp(0, ASCII_MAX_RANGE as i64) as f32 / ASCII_MAX_RANGE)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_cycle_visits_every_mode_and_reverses() {
        let mut mode = VisualizerMode::Off;
        for expected in VisualizerMode::CYCLE
            .iter()
            .skip(1)
            .chain([&VisualizerMode::Off])
        {
            mode = mode.next();
            assert_eq!(mode, *expected);
        }
        assert_eq!(VisualizerMode::Off.previous(), VisualizerMode::Butterfly);
        assert_eq!(VisualizerMode::Bars.previous(), VisualizerMode::Off);
    }

    #[test]
    fn modes_persist_lowercase_and_unknown_ones_read_as_off() {
        assert_eq!(
            serde_json::to_string(&VisualizerMode::Butterfly).unwrap(),
            "\"butterfly\""
        );
        assert_eq!(
            serde_json::from_str::<VisualizerMode>("\"bars\"").unwrap(),
            VisualizerMode::Bars
        );
        assert_eq!(
            serde_json::from_str::<VisualizerMode>("\"future-mode\"").unwrap(),
            VisualizerMode::Off
        );
    }

    #[test]
    fn parser_accepts_trailing_delimiters_and_whitespace() {
        assert_eq!(parse_frame("0;500;1000").unwrap(), vec![0.0, 0.5, 1.0]);
        assert_eq!(
            parse_frame(" \t0; 500 ;1000; \r").unwrap(),
            vec![0.0, 0.5, 1.0]
        );
        assert_eq!(parse_frame("-50;1500").unwrap(), vec![0.0, 1.0]);
    }

    #[test]
    fn parser_rejects_empty_and_malformed_frames() {
        assert!(parse_frame("").is_none());
        assert!(parse_frame(";").is_none());
        assert!(parse_frame("1;wat;3").is_none());
        assert!(parse_frame("1;;3").is_none());
    }
}
