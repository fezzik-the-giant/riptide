// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The queue as it stood at exit, restored stopped on the next launch.

use super::App;
use crate::api::models::Track;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Kept out of `config.json` so a queue of a few hundred tracks is not rewritten
/// into the file holding the OAuth tokens, and so a session that fails to parse
/// costs the queue rather than the login.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub queue: Vec<Track>,
    #[serde(default)]
    pub original_queue: Vec<Track>,
    pub queue_index: usize,
    #[serde(default)]
    pub position: f64,
}

fn session_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("riptide")
        .join("session.json")
}

/// A missing or unreadable session starts riptide with an empty queue; it must
/// never stop it starting.
pub fn load_session() -> Session {
    let path = session_path();
    let Ok(data) = fs::read_to_string(&path) else {
        return Session::default();
    };
    serde_json::from_str(&data).unwrap_or_else(|e| {
        tracing::warn!("ignoring unreadable session {}: {e}", path.display());
        Session::default()
    })
}

/// An empty queue removes the file, so a queue the user cleared stays cleared.
pub fn save_session(session: Option<&Session>) -> Result<()> {
    let path = session_path();
    let Some(session) = session else {
        if path.exists() {
            fs::remove_file(&path)?;
        }
        return Ok(());
    };
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string(session)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

impl App {
    pub fn session(&self) -> Option<Session> {
        let np = &self.now_playing;
        if np.queue.is_empty() {
            return None;
        }
        // Between pressing play on a restored queue and mpv loading it, the
        // position reads 0 while the seek it is owed is still pending.
        let position = np.resume_at.map(|(_, secs)| secs).unwrap_or(np.position);
        Some(Session {
            queue: np.queue.clone(),
            original_queue: np.original_queue.clone(),
            queue_index: np.queue_index,
            position,
        })
    }

    /// Put the queue back stopped, on the track it was playing. Nothing plays
    /// until the user asks; the first Play then picks up at `position`.
    pub fn restore_session(&mut self, session: Session) {
        if session.queue.is_empty() {
            return;
        }
        let index = session.queue_index.min(session.queue.len() - 1);
        let track = session.queue[index].clone();
        let np = &mut self.now_playing;
        np.duration = f64::from(track.duration);
        np.position = session.position.clamp(0.0, np.duration);
        np.resume_at = (np.position > 0.0).then_some((track.id, np.position));
        np.track = Some(track);
        np.queue = session.queue;
        np.queue_index = index;
        if np.shuffle {
            np.original_queue = session.original_queue;
        }
        tracing::info!(
            "restored a queue of {} at [{}/{}]",
            np.queue.len(),
            index + 1,
            np.queue.len()
        );
        self.fetch_now_playing_metadata();
        self.push_mpris_state();
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use crate::app::test_support::{test_app, track};
    use crate::player::PlayerEvent;

    fn restored(position: f64) -> crate::app::test_support::TestApp {
        let mut t = test_app();
        let mut queue: Vec<_> = (1..=3).map(track).collect();
        queue[1].duration = 200;
        t.app.restore_session(Session {
            queue,
            original_queue: Vec::new(),
            queue_index: 1,
            position,
        });
        t.drain_api();
        t
    }

    #[test]
    fn a_restored_queue_waits_stopped_on_its_track() {
        let t = restored(42.0);
        let np = &t.app.now_playing;
        assert_eq!(np.track.as_ref().map(|t| t.id), Some(2));
        assert!(!np.active);
        assert_eq!(np.position, 42.0);
    }

    #[test]
    fn play_resumes_the_restored_track_where_it_left_off() {
        let mut t = restored(42.0);
        t.app.toggle_pause();
        let resolved: Vec<_> = t
            .api_requests()
            .into_iter()
            .filter_map(|r| match r {
                crate::api::ApiRequest::ResolveStreamUrl { track_id, .. } => Some(track_id),
                _ => None,
            })
            .collect();
        assert_eq!(resolved, vec![2]);

        t.app.handle_player_event(PlayerEvent::TrackStarted);
        assert_eq!(t.app.now_playing.position, 42.0);
        assert!(t.app.now_playing.resume_at.is_none());
    }

    #[test]
    fn skipping_away_first_drops_the_resume_point() {
        let mut t = restored(42.0);
        t.app.next_track();
        t.app.now_playing.track = t.app.now_playing.queue.get(2).cloned();
        t.app.handle_player_event(PlayerEvent::TrackStarted);
        assert_eq!(t.app.now_playing.position, 0.0);
        assert!(t.app.now_playing.resume_at.is_none());
    }

    #[test]
    fn a_session_round_trips_through_json() {
        let t = restored(42.0);
        let json = serde_json::to_string(&t.app.session().unwrap()).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.queue.iter().map(|t| t.id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(back.queue_index, 1);
        assert_eq!(back.position, 42.0);
    }
}
