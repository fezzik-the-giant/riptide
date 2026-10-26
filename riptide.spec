# SPDX-FileCopyrightText: 2026 Nicolás Rodríguez Álvarez
# SPDX-License-Identifier: GPL-3.0-or-later

# cargo build --release carries no debug info, so the default debuginfo
# extraction produces an empty package and fails the build.
%global debug_package %{nil}

Name:           riptide
Version:        2.0.0
Release:        1%{?dist}
Summary:        Terminal UI music player for Tidal
License:        GPL-3.0-or-later
URL:            https://github.com/fezzik-the-giant/riptide
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  pkgconfig(openssl)
BuildRequires:  chafa-devel

# mpv is launched as a subprocess over its JSON IPC socket, so nothing links
# against it and RPM's automatic dependency generator cannot see it. The library
# dependencies (libchafa, libssl, glib) are read from the ELF and need no entry.
Requires:       mpv

%description
Riptide is a terminal-based music player for Tidal with a TUI interface built in
Rust (ratatui), driving mpv over its JSON IPC socket for playback.

%prep
%autosetup -n %{name}-%{version}

# Deliberately not cargo-rpm-macros. Its prep macro writes "net offline = true"
# unconditionally and repoints cargo at Fedora's local crate registry, which
# would require all 478 transitive dependencies to be packaged as crate() RPMs.
# This targets a COPR user repo instead, where "Enable internet access during
# builds" lets cargo fetch from crates.io against the committed Cargo.lock.
# Submitting to Fedora proper would need cargo vendor and a vendored tarball.

%build
cargo build --release --locked

%install
# -s strips: with debug_package disabled rpm never strips the binary itself, and
# an unstripped Rust binary carries a symbol table several times its own size.
install -D -p -m 0755 -s target/release/%{name} %{buildroot}%{_bindir}/%{name}

%check
cargo test --release --locked

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/%{name}

%changelog
* Mon Oct 26 2026 Fezzik the Giant <noreply@github.com> - 2.0.0-1
- Added: Playlists can now be created and filled from riptide (#57, #71).
- Added: The Playlists tab labels every playlist as Yours, Tidal (curated by Tidal), User (made by someone else) or Mix, so the ones you can edit stand out (#71).
- Added: Tracks can be removed from your own playlists (#72).
- Added: Your own playlists can be renamed and deleted (#73).
- Added: Tracks in your own playlists can be reordered (#74).
- Added: A spectrum visualizer above the progress bar, driven by [CAVA](https://github.com/karlstav/cava) (#40).
- Added: Search has an Albums pane, so you can find a record by name instead of finding the artist and scrolling their discography (#55).
- Added: Seek from the keyboard (#58).
- Added: Repeat (#56).
- Added: Riptide remembers your queue between sessions (#59).
- Changed: The now-playing bar grows and shrinks with the terminal instead of always taking 16 rows (#63).
- Changed: The queue pane hides itself when the terminal is too narrow for it, instead of taking 26 columns the list needs (#65).
- Changed: Riptide now looks for a new release every 10 minutes while it is open, not just once at startup, so a session left running for days still finds out when one comes out (#68).
- Changed: Installs from a package manager now hear about new releases too (#68).
- Changed: The progress bar is a steady line rather than a sine wave.
- Fixed: Space did nothing once the queue had played to the end or been stopped from a desktop widget.
- Fixed: On Fedora, a COPR install of riptide was treated as one it could update itself (#68).
- Fixed: Lyrics now fill the space beside the cover art instead of three rows at its foot, showing several lines either side of the one being sung.
- Internal: CI builds the Nix flake far less often and far faster (#62).
- Internal: cargo clippy runs clean, so a new warning stands out instead of disappearing among 130 old ones (#77).

* Mon Oct 12 2026 Fezzik the Giant <noreply@github.com> - 1.6.0-1
- Added: A settings modal on Shift+S, covering Dolby Atmos, shuffle, the queue panel, volume and Last.fm scrobbling, over a read-only summary of the signed-in account.
- Added: Dolby Atmos is now a setting rather than something that happened to you.
- Added: With Atmos off, releases Tidal holds only as an Atmos mix are hidden, the way the official apps hide them.
- Changed: Atmos releases now play the stereo FLAC master unless Atmos is turned on.
- Changed: Last.fm scrobbling can be turned on and off without restarting
- Fixed: Installing with Nix failed to build, stopping at include_bytes! with a missing assets/wave-logo-320-transparent.png (#60).
- Fixed: Quitting with q never finished when Last.fm was not set up.
- Fixed: Playback stopped with No stream available for "…" once riptide had been open for about four hours — most often noticed after leaving it paused for a while and coming back to find the rest of the queue dead.
- Fixed: A rejected session was reported as though the track were at fault.
- Fixed: An artist's albums, EPs, singles and top tracks were drawn straight from the backing list rather than the rows on screen.
- Fixed: Resolving an Atmos track took three requests to Tidal instead of one.
- Internal: CI now builds the Nix flake.

* Thu Sep 10 2026 Fezzik the Giant <noreply@github.com> - 1.5.0-1
- Added: a on an album or playlist row queues the whole thing, and Alt+a puts it next (#54).
- Added: Alt+f adds the playing track to your favorites from anywhere in the app.
- Added: Alt+a queues a track to play after the current one instead of at the end (#53).
- Fixed: Pressing a in the moment between starting a track and it beginning to play replaced the whole queue with that one track.

* Thu Sep 10 2026 Fezzik the Giant <noreply@github.com> - 1.4.1-1
- Fixed: Building from source in a terminal could fail in the test suite, which broke paru/makepkg installs of 1.4.0.

* Wed Sep 09 2026 Fezzik the Giant <noreply@github.com> - 1.4.0-1
- Added: Self-update for binaries installed via install.sh or a manual release download.
- Added: Fullscreen album-art mode with Shift+A, on-demand high-resolution covers, and a compact playback HUD
- Added: The help modal filters as you type.
- Changed: j and k type into the help modal instead of scrolling it, now that it has a filter box of its own — the same rule the search box, the filter box and the command palette already followed.
- Fixed: The selected row was unreadable on light terminal themes.
- Fixed: The quality badge punched an eight-column hole through the middle of the highlighted row.
- Fixed: The library list and the queue could both draw a cursor at once, so neither looked like the pane the keys went to.
- Fixed: The queue's divider and title were a fixed dark grey, which on a light terminal read stronger than the focused accent and inverted the signal they exist to give.

* Wed Aug 19 2026 Fezzik the Giant <noreply@github.com> - 1.3.0-1
- Added: j and k move down and up in every list — the tabs, the detail views, the queue, the help modal and the pickers.
- Added: Volume, seek, shuffle and stop now work from desktop media widgets and playerctl, alongside the play/pause and skip controls that already did.
- Added: The now-playing bar reports the bit depth and sample rate Tidal actually delivered, next to the quality badge.
- Changed: The Home tab is a carousel.
- Changed: List rows are laid out in columns: title, artists, then duration, quality badge and favourite marker.
- Fixed: Tracks credited to more than one artist listed only the first.
- Fixed: Mix playlists showed "0 tracks".
- Fixed: The Search tab's playlist results showed no favourite marker, so saved and unsaved playlists looked identical
- Fixed: The command palette was unusable while the queue had focus: : opened it, but everything typed afterwards went to the queue, where c copied a link and d removed a track
- Fixed: Album art on the now-playing bar could vanish when a track without a cover started, and stayed gone for the rest of the queue.
- Fixed: Play from a stopped state could restart the current track instead of resuming it, if a media key repeated or a desktop sent the command twice
- Fixed: Sign-in failures now say what Tidal actually reported instead of only the HTTP status.
- Internal: Listing the Playlists tab no longer fetches every playlist's full track list to display their names — 320 KB and three quarters of a second became 26 KB.
- Internal: The tab strip shared by the artist, search and Home views is one helper rather than two copies, and every list row goes through one of four row builders instead of being assembled by hand in twelve places
- Internal: Truncation is measured in display columns rather than characters, so CJK titles and emoji no longer misalign the columns around them

* Tue Aug 18 2026 Fezzik the Giant <noreply@github.com> - 1.2.0-1
- Added: Press u to undo the last thing you removed from your library — a track, artist, album or playlist.
- Added: Dolby Atmos tracks and albums now show an ATMOS badge.
- Changed: Removing something from your library moved from f to d, matching what d already does in the queue.
- Internal: The identical quality-badge implementations on tracks and albums collapsed into one helper
- Internal: Dropped the audioQuality field from tracks and albums along with the MQA and 320 badges that read it.

* Tue Aug 18 2026 Fezzik the Giant <noreply@github.com> - 1.1.1-1
- Fixed: The Albums and Playlists tabs stopped at the first page, showing about 20 entries however large the library was.
- Fixed: A track that appeared twice in a row in the queue restarted from the beginning over and over, and the repeated stream requests eventually drew a "429 Too Many Requests" from Tidal.
- Fixed: Tracks that Tidal reports more than once in your favorites are now listed once.
- Internal: Logs are readable again.
- Internal: A failed AUR or COPR publish now fails the release run instead of being tolerated

* Mon Aug 17 2026 Fezzik the Giant <noreply@github.com> - 1.1.0-1
- Added: Filter the Tracks, Artists, Albums and Playlists tabs by pressing / and typing.
- Changed: The command palette moved from / to :, freeing / for filtering.
- Fixed: The Now Playing bar — title, album art, details and lyrics — could describe a different track than the one actually playing.
- Fixed: Pressing shuffle while a track was ending could hand playback to a different track, again leaving Now Playing behind.
- Fixed: Turning shuffle off discarded every track queued since it was turned on, and could leave the selection pointing at a different track than the one playing
- Fixed: Starting a new album or playlist while shuffle was on left the previous playlist still loading pages into the queue
- Internal: The app now tracks what mpv actually has queued and re-syncs when the two disagree, instead of assuming mpv followed along.
- Internal: Removing an item from a library list goes through one helper rather than three near-identical copies, and the blinking input cursor is defined once instead of in every text box

* Fri Aug 14 2026 Nicolás Rodríguez Álvarez <noreply@github.com> - 1.0.0-1
- Initial COPR packaging
