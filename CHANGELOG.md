# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-08-13

The Tidal API migration is complete: every endpoint that can use the v2 API now
does. Stream URLs remain on v1 permanently — the v2 equivalent serves
DRM-encrypted media that mpv cannot play. See the note on `get_stream_url`.

### Added
- Fullscreen album-art mode with `Shift+A`, on-demand high-resolution covers, and a compact playback HUD
- Sort order, volume, shuffle state and queue visibility now persist across restarts, in a new `prefs` block in `config.json`
  (the Tracks sort is stored as `tracks_sort`)
- The active sort is shown in each list header, and the sort palette opens on the sort already applied
- `t` shows/hides the queue panel; hiding it gives the content list the full width
- Favorite indicator on search artist results

### Changed
- Tabs moved from the left sidebar to a strip across the top, with the active tab boxed
- Album art moved into the now-playing bar above the track info, and is substantially larger
- Lyrics now sit directly above the waveform
- The search box only opens automatically when there are no results, so results survive leaving and returning to the tab; `Esc` closes the box rather than switching tab, and `Tab` keeps working while it is open
- Global keybinds (transport, volume, tabs, help) now work while the queue is focused
- Track lists number from 1 rather than 0
- The "Favorites" tab is now called "Tracks", and its command palette entry is now `tracks` (previously `favorites`)
- Album, track and artist favorites, lyrics, and search all moved to the v2 API

### Fixed
- Cursor lag and waveform stutter in the artist, album and playlist views, caused by rebuilding the terminal image protocol every frame
- Playlist detail showed 0 tracks: the count read a v1 field name the v2 API never sends, and the pagination response then overwrote it
- Sorting playlists by Last Added did nothing, because `addedAt` was discarded during parsing
- A sort restored from preferences is now applied when data arrives, not only when picked

### Internal
- `client.rs` (3,604 lines) split into ten domain modules, each owning its own requests and parsers
- `ui.rs`, `events.rs`, `app/state.rs` and `api/mod.rs` similarly split; the largest file in the crate dropped from 3,604 lines to 702

## [0.13.0] - 2026-08-11

### Added
- Favorite track indicator (filled heart ❤) displayed next to all favorited tracks across views
- Track counts now displayed in Favorites and Playlist detail headers for consistency

### Fixed
- Fixed duplicate track counts appearing in album and playlist detail headers

## [0.12.2] - 2026-08-11

### Fixed
- Fixed Home tab mixes (New Releases, Daily Mixes, Discovery) now show cover art and descriptions
- Fixed track count displaying correctly in mix detail panes
- Added synchronized loading for Home tab sections with loading animation in title

## [0.12.1] - 2026-08-11

### Fixed
- Reduced default logging level from debug to warn for cleaner output on installed binaries

## [0.12.0] - 2026-08-11

### Added
- Search endpoints now use v2 API with cursor-based pagination
- Modularized search and playlist functionality into dedicated modules for better code organization
- Page Up/Down support for faster navigation through lists
- Dynamic page loading for search results when cursor approaches end of list
- Startup log banner for app initialization
- Streaming buffer to reduce stuttering over mobile data

### Changed
- Improved logging clarity while reducing verbosity
- More natural scrolling behavior when navigating upwards
- Better pagination handling to prevent duplicate data fetches

### Fixed
- Toast messages now display for exactly 5 seconds instead of 20+ seconds (switched from tick-based to wall-clock timing)
- Removed ineffective retry logic from stream URL resolution
- Fixed excessive polling of favorites after each API response
- Fixed stale position/duration data persisting between tracks
- Fixed lossless audio streaming with corrected quality validation
- Improved HTTP authentication for FLAC streaming

## [0.11.0] - 2026-08-05

### Added
- New Home tab to display New Arrivals, Mixes, and Daily Discovery

### Changed
- Playlists now use v2 API for better support.
- Pagination now fetches next page of tracks only once when approaching end of list.

## [0.10.0] - 2026-08-04

### Added
- LastFM scrobbling support. Check out [the README](https://github.com/fezzik-the-giant/riptide#lastfm-scrobbling) for more information.

## [0.9.0] - 2026-08-03

### Added
- Automated install script

### Changed
- README to include instructions for automated install

## [0.8.0] - 2026-08-03

### Added
- Structured logging and ability to change logging level through environment variable

### Changed
- Replaced image rendering logic with [ratatui-image](https://crates.io/crates/ratatui-image) to include Sixel support
- Riptide now detects your terminal graphics protocol and renders image accordingly through Kitty, Sixel, or halfblock

## [0.7.3] - 2026-07-27

### Changed
- Updated styling of Queue items to be more legible and easier to distinguish.

## [0.7.2] - 2026-07-23

### Changed
- Merged Github Actions so AUR and Github Releases run in one job in parallel

## [0.7.1] - 2026-07-23

### Added
- Github Action to build binaries for Linux and MacOS and create Github Releases
- Changelog to keep track of changes -> used as release notes in Github Release

### Changed
- Updated README to include information on installing from Release binaries

## [0.7.0] - 2026-07-23

### Added
- Artist navigation: Press 'g' on any track to navigate to artist page
- Multi-artist support: If a track has multiple artists, a modal lets you select which artist to visit
- Track display now shows all artists instead of just the primary artist

### Fixed
- Quality badge alignment in track listings (badges now appear at end of line)

## [0.6.2] - 2026-07-23

### Added
- Artist Tab carousel styling with block rendering
- Artist detail now shows EPs and Singles in addition to Albums
- Share link copy functionality for tracks and albums (keybinds 'c' and 'C')

### Changed
- Updated artist view carousel styling to better indicate tab options
- Quality badges now display on all tracks

### Fixed
- Missing field from Track struct initializer
- Help modal now displays all keybinds accessibly

## [0.6.1] - 2026-07-15

### Added
- Volume control with keybinds ('+'/'-' for volume up/down)
- Help modal showing all available keybinds (press '?')
- Sorting by artist for albums and favorites

### Changed
- Improved keybind clarity and simplified controls

### Fixed
- Keybind display overflow at bottom of screen
