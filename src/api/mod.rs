// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Tidal API: auth, models, the HTTP client, and the async worker that fronts it.

pub mod auth;
pub mod client;
pub mod models;

mod messages;
mod worker;

pub use messages::{ApiRequest, ApiResponse};
pub use worker::ApiWorker;
