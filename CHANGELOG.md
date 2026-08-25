# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Self-update for binaries installed via `install.sh` or a manual release download. The player checks GitHub Releases shortly after startup; when a newer version exists the footer hints at it and `U` opens a dialog to download, verify the SHA-256 checksum, and install it atomically. A failed check can be retried from the dialog (`u`). Pacman/AUR, Nix and Cargo installs are left to their own package manager. Also available headless as `riptide update`
- Fullscreen album-art mode with `Shift+A`, on-demand high-resolution covers, and a compact playback HUD

## [1.3.0] - 2026-08-19

### Added
- `j` and `k` move down and up in every list — the tabs, the detail views, the queue, the help modal and the pickers. `h` and `l` already moved between panes, so the same hand now moves within one (#37). They stay ordinary letters inside the search box, the filter box and the command palette
- Volume, seek, shuffle and stop now work from desktop media widgets and `playerctl`, alongside the play/pause and skip controls that already did. Thanks to @dghelm (#49)
- The now-playing bar reports the bit depth and sample rate Tidal actually delivered, next to the quality badge. The badge describes the catalogue; the two differ whenever the client is not entitled to the advertised tier, which is why a `MAX` release can still arrive as 16-bit/44.1 kHz

### Changed
- The Home tab is a carousel. Its three sections used to split the tab in thirds regardless of content, so "New Releases" and "Daily Discovery" — one mix each — each owned a third of the screen while the eight daily mixes were squeezed into what was left. One section now fills the tab, the strip along the top names all three with their counts, and `←`/`→` switches between them, which is what the layout implied all along. Each section also shows its own cover art and its own loading state, instead of the whole tab waiting on the slowest of the three
- List rows are laid out in columns: title, artists, then duration, quality badge and favourite marker. Nothing truncated before, so the terminal clipped rows at the right edge and a long title pushed the metadata off the screen entirely. The metadata now stays put and only the text ellipsizes, the artist column stepping aside first on a narrow terminal. The row under the cursor scrolls its text if it does not fit

### Fixed
- Tracks credited to more than one artist listed only the first. Every list parser dropped the rest of the `artists` relationship, so a collaboration or a feature showed a single name — and `g` could not reach the other artists, because the picker only appears when a track has more than one
- Mix playlists showed "0 tracks". Tidal sends no item count for them at all, and the missing value was being rendered as zero rather than left out
- The Search tab's playlist results showed no favourite marker, so saved and unsaved playlists looked identical
- The command palette was unusable while the queue had focus: `:` opened it, but everything typed afterwards went to the queue, where `c` copied a link and `d` removed a track
- Album art on the now-playing bar could vanish when a track without a cover started, and stayed gone for the rest of the queue. Thanks to @dghelm (#49)
- Play from a stopped state could restart the current track instead of resuming it, if a media key repeated or a desktop sent the command twice
- Sign-in failures now say what Tidal actually reported instead of only the HTTP status. Credentials from the Tidal developer portal cannot drive the device-login flow at all, and that case now says so rather than failing with a bare `400 Bad Request`

### Internal
- Listing the Playlists tab no longer fetches every playlist's full track list to display their names — 320 KB and three quarters of a second became 26 KB. The request that did it was also redundant: everything it returned was refetched by a second call that pages correctly, and it stopped at 20 playlists
- The tab strip shared by the artist, search and Home views is one helper rather than two copies, and every list row goes through one of four row builders instead of being assembled by hand in twelve places
- Truncation is measured in display columns rather than characters, so CJK titles and emoji no longer misalign the columns around them

## [1.2.0] - 2026-08-18

### Added
- Press `u` to undo the last thing you removed from your library — a track, artist, album or playlist. The undo stays available until the next removal rather than expiring with the message, so it still works if you only notice the mistake minutes later. Note that Tidal stamps a fresh "added" date on the way back in, so a restored item sorts as newly added rather than returning to its old position
- Dolby Atmos tracks and albums now show an `ATMOS` badge. They previously showed no quality badge at all: Tidal frequently tags them `DOLBY_ATMOS` with no `LOSSLESS` alongside, and the badge only looked for the lossless tags

### Changed
- Removing something from your library moved from `f` to `d`, matching what `d` already does in the queue. In the Tracks, Artists, Albums and Playlists tabs every row is by definition already saved, so `f` there could only ever remove — which people were firing by accident with a finger resting on the key (#39). `f` still favorites, follows and saves everywhere it can actually add something, and in the library tabs it now points you at `d` instead

### Internal
- The identical quality-badge implementations on tracks and albums collapsed into one helper
- Dropped the `audioQuality` field from tracks and albums along with the MQA and 320 badges that read it. The v2 API never sends it, so those badges could not fire; the stream endpoint's own `audioQuality`, which is still live, is untouched

## [1.1.1] - 2026-08-18

### Fixed
- The Albums and Playlists tabs stopped at the first page, showing about 20 entries however large the library was. Albums synthesised a total from the page it had just received, which made the list look complete as soon as one page arrived, and the cursor for the next page was a path being used as a URL; Playlists fetched a single page and discarded the cursor entirely
- A track that appeared twice in a row in the queue restarted from the beginning over and over, and the repeated stream requests eventually drew a "429 Too Many Requests" from Tidal. Stream URLs are matched by track id, so the prefetch for the second copy was mistaken for a request to play the first. Queueing the track that is already playing triggered this too (#43)
- Tracks that Tidal reports more than once in your favorites are now listed once. Libraries imported from another service can end up with repeated entries (#43)

### Internal
- Logs are readable again. A bare `RIPTIDE_LOG_LEVEL` now applies to riptide alone, so a debug log is no longer a quarter connection-pool chatter from an HTTP dependency, and the per-object parser lines that made up another third are gone. `info` reports what loaded and every track as it starts; anomalies that explain a bug report — duplicate entries, missing references, mpv disagreeing with the queue — are warnings instead of being buried in debug
- A failed AUR or COPR publish now fails the release run instead of being tolerated

## [1.1.0] - 2026-08-17

### Added
- Filter the Tracks, Artists, Albums and Playlists tabs by pressing `/` and typing. Tracks match on title or artist, the other tabs on name. The active filter is named in the list header (`Tracks (3 of 214) · A-Z · /ts`), `Enter` keeps it applied so you can navigate the narrowed list, and `Esc` clears it

### Changed
- The command palette moved from `/` to `:`, freeing `/` for filtering. `/` already opened the search box on the Search tab, so it now means "find something here" nearly everywhere

### Fixed
- The Now Playing bar — title, album art, details and lyrics — could describe a different track than the one actually playing. Toggling shuffle removed the wrong entry from mpv's playlist, which made mpv jump to the next track without telling the app. It only happened once a track had advanced on its own, because skipping with next/previous silently repaired it
- Pressing shuffle while a track was ending could hand playback to a different track, again leaving Now Playing behind. Clearing the queued-up next track before its replacement was ready left mpv with nothing to play, and a file handed to an empty mpv playlist starts playing rather than waiting its turn
- Turning shuffle off discarded every track queued since it was turned on, and could leave the selection pointing at a different track than the one playing
- Starting a new album or playlist while shuffle was on left the previous playlist still loading pages into the queue

### Internal
- The app now tracks what mpv actually has queued and re-syncs when the two disagree, instead of assuming mpv followed along. mpv's playlist is append-only with a moving position, so the old fixed-index removal hit the playing entry once anything had advanced
- Removing an item from a library list goes through one helper rather than three near-identical copies, and the blinking input cursor is defined once instead of in every text box

## [1.0.1] - 2026-08-14

### Fixed
- Favorite tracks could appear twice once you scrolled near the end of the Tracks tab. Reaching the end re-requested the collection, and because that request always fetches everything from the start, the whole list was appended a second time
- Quality badges (MAX / HI-FI) were missing throughout the Tracks tab. The favorites parser still read `audioQuality`, a v1 field name the v2 API never sends; it now reads `mediaTags` like every other parser
- The placeholder logo only appeared when running from a source checkout. It was loaded from `assets/` via a path relative to the working directory, so an installed binary silently showed nothing; it is now embedded in the binary

### Internal
- Removed scroll-triggered loading. Every list already loads in full before it reaches the UI, so `should_load_more()`, `check_load_more()` and the per-list `load_more_*` helpers were dead weight that could still fire and duplicate data
- Dropped `offset` and `limit` from API requests and request types — the Tidal v2 API ignores both, and `offset=0`, `offset=20` and no offset all return identical pages
- Formatted the codebase with rustfmt, added a CI lint job that fails on unformatted code, and bundled an opt-in pre-commit hook (`git config core.hooksPath .githooks`)
- Bumped `softprops/action-gh-release` to v3 for the Node 24 Actions runtime

## [1.0.0] - 2026-08-13

The Tidal API migration is complete: every endpoint that can use the v2 API now
does. Stream URLs remain on v1 permanently — the v2 equivalent serves
DRM-encrypted media that mpv cannot play. See the note on `get_stream_url`.

### Added
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
