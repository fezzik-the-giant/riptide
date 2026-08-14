// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Optional CAVA spectrum capture and the state shared with the terminal UI.

use serde::{Deserialize, Deserializer, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Instant,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::watch,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualizerMode {
    #[default]
    Off,
    Bars,
    Outline,
}

impl VisualizerMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bars => "Bars",
            Self::Outline => "Outline",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Bars,
            Self::Bars => Self::Outline,
            Self::Outline => Self::Off,
        }
    }
}

impl<'de> Deserialize<'de> for VisualizerMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "bars" => Self::Bars,
            "outline" => Self::Outline,
            _ => Self::Off,
        })
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
    Unavailable(UnavailableReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    MissingBinary,
    SpawnFailed,
    Exited,
}

pub struct CavaWorker {
    enabled_rx: watch::Receiver<bool>,
    spectrum_tx: watch::Sender<SpectrumState>,
}

enum SessionEnd {
    Disabled,
    SenderClosed,
    Unavailable(UnavailableReason),
}

impl CavaWorker {
    pub fn new(
        enabled_rx: watch::Receiver<bool>,
        spectrum_tx: watch::Sender<SpectrumState>,
    ) -> Self {
        Self {
            enabled_rx,
            spectrum_tx,
        }
    }

    pub async fn run(mut self) {
        self.publish(SpectrumState::Disabled);

        loop {
            if !self.wait_until_enabled().await {
                return;
            }

            self.publish(SpectrumState::Starting);
            match self.run_session().await {
                SessionEnd::Disabled => self.publish(SpectrumState::Disabled),
                SessionEnd::SenderClosed => return,
                SessionEnd::Unavailable(reason) => {
                    self.publish(SpectrumState::Unavailable(reason));
                    if !self.wait_until_disabled().await {
                        return;
                    }
                    self.publish(SpectrumState::Disabled);
                }
            }
        }
    }

    async fn wait_until_enabled(&mut self) -> bool {
        loop {
            if *self.enabled_rx.borrow_and_update() {
                return true;
            }
            if self.enabled_rx.changed().await.is_err() {
                return false;
            }
        }
    }

    async fn wait_until_disabled(&mut self) -> bool {
        loop {
            if !*self.enabled_rx.borrow_and_update() {
                return true;
            }
            if self.enabled_rx.changed().await.is_err() {
                return false;
            }
        }
    }

    async fn run_session(&mut self) -> SessionEnd {
        let config_path = cava_config_path();
        if let Err(error) = tokio::fs::write(&config_path, CAVA_CONFIG).await {
            tracing::warn!(path = %config_path.display(), %error, "failed to write CAVA config");
            remove_config(&config_path).await;
            return SessionEnd::Unavailable(UnavailableReason::SpawnFailed);
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
                let reason = if error.kind() == io::ErrorKind::NotFound {
                    UnavailableReason::MissingBinary
                } else {
                    UnavailableReason::SpawnFailed
                };
                tracing::warn!(%error, "failed to start CAVA");
                remove_config(&config_path).await;
                return SessionEnd::Unavailable(reason);
            }
        };

        let Some(stdout) = child.stdout.take() else {
            tracing::warn!("CAVA started without a stdout pipe");
            terminate_child(&mut child).await;
            remove_config(&config_path).await;
            return SessionEnd::Unavailable(UnavailableReason::SpawnFailed);
        };

        let stderr_task = child.stderr.take().map(|stderr| {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(message = %line, "CAVA stderr");
                }
            })
        });
        let mut lines = BufReader::new(stdout).lines();

        let end = loop {
            tokio::select! {
                changed = self.enabled_rx.changed() => {
                    match changed {
                        Err(_) => {
                            terminate_child(&mut child).await;
                            break SessionEnd::SenderClosed;
                        }
                        Ok(()) if !*self.enabled_rx.borrow_and_update() => {
                            terminate_child(&mut child).await;
                            break SessionEnd::Disabled;
                        }
                        Ok(()) => {}
                    }
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => match parse_frame(&line) {
                            Ok(bands) => self.publish(SpectrumState::Active(SpectrumFrame {
                                bands,
                                received_at: Instant::now(),
                            })),
                            Err(error) => tracing::debug!(%error, "skipping malformed CAVA frame"),
                        },
                        Ok(None) => {
                            match child.wait().await {
                                Ok(status) => tracing::warn!(%status, "CAVA exited"),
                                Err(error) => tracing::warn!(%error, "failed to wait for CAVA"),
                            }
                            break SessionEnd::Unavailable(UnavailableReason::Exited);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "failed to read CAVA output");
                            terminate_child(&mut child).await;
                            break SessionEnd::Unavailable(UnavailableReason::Exited);
                        }
                    }
                }
            }
        };

        if let Some(task) = stderr_task {
            let _ = task.await;
        }
        remove_config(&config_path).await;
        end
    }

    fn publish(&self, state: SpectrumState) {
        let _ = self.spectrum_tx.send(state);
    }
}

fn cava_config_path() -> PathBuf {
    std::env::temp_dir().join(format!("riptide-cava-{}.conf", std::process::id()))
}

async fn terminate_child(child: &mut Child) {
    if let Err(error) = child.kill().await
        && error.kind() != io::ErrorKind::InvalidInput
    {
        tracing::debug!(%error, "failed to terminate CAVA");
    }
    if let Err(error) = child.wait().await {
        tracing::debug!(%error, "failed to reap CAVA");
    }
}

async fn remove_config(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(path = %path.display(), %error, "failed to remove CAVA config");
    }
}

fn parse_frame(line: &str) -> Result<Vec<f32>, &'static str> {
    let trimmed = line.trim_matches(|c: char| c.is_ascii_whitespace());
    let frame = trimmed.strip_suffix(';').unwrap_or(trimmed);
    if frame.is_empty() {
        return Err("empty frame");
    }

    frame
        .split(';')
        .map(|token| {
            let token = token.trim_matches(|c: char| c.is_ascii_whitespace());
            let value = token.parse::<i64>().map_err(|_| "invalid amplitude")?;
            Ok(value.clamp(0, 1000) as f32 / 1000.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_values_round_trip() {
        for (mode, encoded) in [
            (VisualizerMode::Off, "\"off\""),
            (VisualizerMode::Bars, "\"bars\""),
            (VisualizerMode::Outline, "\"outline\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), encoded);
            assert_eq!(
                serde_json::from_str::<VisualizerMode>(encoded).unwrap(),
                mode
            );
        }
    }

    #[test]
    fn unknown_mode_defaults_to_off() {
        assert_eq!(
            serde_json::from_str::<VisualizerMode>("\"butterfly\"").unwrap(),
            VisualizerMode::Off
        );
    }

    #[test]
    fn generated_config_matches_protocol() {
        assert!(CAVA_CONFIG.contains("framerate = 30"));
        assert!(CAVA_CONFIG.contains("bars = 64"));
        assert!(CAVA_CONFIG.contains("source = auto"));
        assert!(CAVA_CONFIG.contains("method = raw"));
        assert!(CAVA_CONFIG.contains("data_format = ascii"));
        assert!(CAVA_CONFIG.contains("ascii_max_range = 1000"));
        assert!(CAVA_CONFIG.contains("bar_delimiter = 59"));
        assert!(CAVA_CONFIG.contains("frame_delimiter = 10"));
        assert!(!CAVA_CONFIG.contains("token"));
        assert!(!CAVA_CONFIG.contains("/home/"));
    }

    #[test]
    fn parser_accepts_normal_trailing_and_whitespace_frames() {
        assert_eq!(parse_frame("0;500;1000").unwrap(), vec![0.0, 0.5, 1.0]);
        assert_eq!(parse_frame("0;500;1000;").unwrap(), vec![0.0, 0.5, 1.0]);
        assert_eq!(
            parse_frame(" \t0; 500 ;1000; \r").unwrap(),
            vec![0.0, 0.5, 1.0]
        );
    }

    #[test]
    fn parser_rejects_empty_and_malformed_frames() {
        assert!(parse_frame("").is_err());
        assert!(parse_frame(";").is_err());
        assert!(parse_frame("1;wat;3").is_err());
        assert!(parse_frame("1;;3").is_err());
    }

    #[test]
    fn parser_clamps_values_and_accepts_any_nonempty_band_count() {
        assert_eq!(parse_frame("-50;1500").unwrap(), vec![0.0, 1.0]);
        assert_eq!(parse_frame("250").unwrap(), vec![0.25]);
    }
}
