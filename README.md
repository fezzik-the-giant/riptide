# Riptide

## Table of Contents

- [Features](#features)
- [Requirements](#requirements)
- [Installation](#installation)
    - [Automated Install](#automated-install)
    - [Manual Install](#manual-install)
    - [Arch (AUR)](#arch-aur)
    - [Build from Source](#build-from-source)
    - [Nix](#nix)
- [Setup](#setup)
- [Configuration](#configuration)
- [Last.fm Scrobbling](#lastfm-scrobbling)
- [Keybindings](#keybindings)
- [Image Rendering](#image-rendering)
- [Logging](#logging)
- [License](#license)

A terminal UI music player for Tidal, built with Rust.

![img_2.png](assets/riptide_screenshot.png)

## Features

- Browse your Tidal library: tracks, artists, playlists, and albums
- Full-text search across tracks, artists, and playlists
- Synchronized lyrics
- Album art in the now-playing bar and detail views (quality determined by terminal graphics protocol)
- Artist pictures and biography
- Queue management — add tracks, navigate to any position, remove entries, play from any point; collapse it with `t`
  when you want the full width for browsing
- Gapless playback via mpv
- Audio quality indicator (Hi-Res, FLAC, MQA, AAC)
- Stable playback progress rail and optional compact spectrum visualization
- Sort any library list by name, artist, or date added — your choice is remembered between sessions and shown in the
  list header
- Volume, shuffle, and queue visibility persist across restarts

## Requirements

- **Rust** 1.85+ (2024 edition) — to build from source
- **mpv** — used as the audio backend; must be on your `PATH`
- A **Tidal** account (HiFi or HiFi Plus recommended for lossless quality)
- **chafa** — used for terminal graphics support, dependency of ratatui-image
- **cava** (optional) — enables the music-reactive spectrum visualizer

### Installing dependecies

| Platform              | Command                                        |
|-----------------------|------------------------------------------------|
| Linux (Debian/Ubuntu) | `sudo apt install mpv dbus libglib2.0-0 chafa` |
| Linux (Arch)          | `sudo pacman -S mpv dbus glib2 chafa`          |
| Linux (Fedora)        | `sudo dnf install mpv dbus glib2 chafa`        |

To enable visualization, install CAVA separately:

```bash
# Arch
sudo pacman -S cava

# Debian/Ubuntu
sudo apt install cava

# macOS
brew install cava
```

Riptide works normally without CAVA. Press `v` after installing it to cycle
through Bars, Outline, Columns, Bricks, Dots, and Butterfly; press `V` to turn
visualization off. If a previous launch attempt failed, cycle back to Off
first. CAVA captures the default output monitor, so its bars may also react to
browser audio, notifications, or another player. Capture or visualization
failures do not affect playback.

## Installation

### Automated Install

Download and install the latest binary automatically:

```bash
curl -fsSL https://raw.githubusercontent.com/fezzik-the-giant/riptide/master/install.sh | bash
```

The script detects your platform (Linux/macOS, x86_64/ARM64) and installs to `/usr/local/bin`.

To install to a custom directory:

```bash
INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/fezzik-the-giant/riptide/master/install.sh | bash
```

### Manual Install

Download the latest binary for your platform
from [GitHub Releases](https://github.com/fezzik-the-giant/riptide/releases):

```bash
# Linux x86_64
wget https://github.com/fezzik-the-giant/riptide/releases/download/vX.Y.Z/riptide-vX.Y.Z-x86_64-linux-gnu.tar.gz
tar -xzf riptide-vX.Y.Z-x86_64-linux-gnu.tar.gz
./riptide

# macOS x86_64
wget https://github.com/fezzik-the-giant/riptide/releases/download/vX.Y.Z/riptide-vX.Y.Z-x86_64-apple-darwin.tar.gz
tar -xzf riptide-vX.Y.Z-x86_64-apple-darwin.tar.gz
./riptide

# macOS ARM64 (Apple Silicon)
wget https://github.com/fezzik-the-giant/riptide/releases/download/vX.Y.Z/riptide-vX.Y.Z-aarch64-apple-darwin.tar.gz
tar -xzf riptide-vX.Y.Z-aarch64-apple-darwin.tar.gz
./riptide
```

Or install to your PATH:

```bash
tar -xzf riptide-vX.Y.Z-*.tar.gz
sudo mv riptide /usr/local/bin/
```

Verify checksums with the included `SHA256SUMS` file:

```bash
sha256sum -c SHA256SUMS
```

### Arch (AUR)

> [!WARNING]
> The AUR team has
recently [disabled all pushes](https://lists.archlinux.org/archives/list/aur-general@lists.archlinux.org/message/YPJ3FQYJTJXXY3RUXCYLMHUKHLIUNVFF/).
As such, the current version on the AUR is behind by several releases. The current recommended installation method for
all users is to use [the automated install script](#automated-install). If you previously installed Riptide from AUR,
remove it with `paru/yay -R riptide` before using the automated installer.

Riptide is available on the AUR and can be installed with:

```bash
paru -S riptide

# or if using yay
yay -S riptide
```

### Build from Source

Requires Rust 1.85+ and Cargo:

```bash
git clone https://github.com/fezzik-the-giant/riptide
cd riptide
cargo install --path .
```

The `riptide` binary will be placed in `~/.cargo/bin/`. Make sure that directory is on your `PATH`.

### Nix

Tested on: `x86_64-linux`.

Add riptide as your `flake.nix` input:

```nix
{
  inputs.riptide.url = "github:fezzik-the-giant/riptide";
}
```

Then add it to your home-manager:

```nix
home.packages =  (with pkgs; [
    inputs.riptide.packages.${system}.default
 ];
```

A development shell is also available:

```sh
git clone https://github.com/fezzik-the-giant/riptide
cd riptide/
nix develop
```

You can also run riptide directly:

```sh
nix run github:fezzik-the-giant/riptide
```

## Setup

Riptide uses Tidal's OAuth device-authorization flow. On first launch it will print a URL and a short code:

```
╔══════════════════════════════════════════╗
║           Tidal Authorization            ║
╠══════════════════════════════════════════╣
║  Open:                                   ║
║  https://link.tidal.com/XXXXX            ║
╠══════════════════════════════════════════╣
║  Code: ABCD-1234                         ║
╚══════════════════════════════════════════╝

Waiting for authorization…
```

Open the URL in a browser, log in with your Tidal account, and enter the code. Riptide will save your tokens to the
config file and launch immediately. You will not need to authenticate again unless your refresh token expires.

## Configuration

The config file lives at:

| Platform | Path                            |
|----------|---------------------------------|
| Linux    | `~/.config/riptide/config.json` |

It is created automatically on first run. Example:

```json
{
  "client_id": null,
  "client_secret": null,
  "access_token": "...",
  "refresh_token": "...",
  "expires_at": "2025-01-01T00:00:00+00:00",
  "user_id": 12345678,
  "country_code": "US",
  "session_id": "...",
  "prefs": {
    "tracks_sort": "LastAdded",
    "artists_sort": null,
    "fav_albums_sort": "ByArtist",
    "playlists_sort": null,
    "volume": 80,
    "shuffle": false,
    "queue_visible": true,
    "visualizer_mode": "off"
  }
}
```

### Preferences

The `prefs` block records UI choices so they survive a restart. It is written once when Riptide exits, and every key is
optional — a config from an older version loads fine and picks up the defaults below.

| Key                                                                   | Values                                                   | Default      |
|-----------------------------------------------------------------------|----------------------------------------------------------|--------------|
| `tracks_sort`, `artists_sort`, `fav_albums_sort`, `playlists_sort` | `"Alphabetical"`, `"LastAdded"`, `"ByArtist"`, or `null` | `null` (A–Z) |
| `volume`                                                              | `0`–`100`                                                | `100`        |
| `shuffle`                                                             | `true` / `false`                                         | `false`      |
| `queue_visible`                                                       | `true` / `false`                                         | `true`       |
| `visualizer_mode`                                                     | `"off"`, `"bars"`, `"outline"`, `"columns"`, `"bricks"`, `"dots"`, `"butterfly"` | `"off"` |

`null` for a sort means "never chosen", which sorts alphabetically. Note that
`ByArtist` only applies to the Tracks and Albums tabs; the Artists and Playlists tabs offer name and date only.

Because preferences are saved on exit, changes made during a session that ends in a crash are not kept.

### Using your own OAuth credentials

Riptide ships with built-in fallback credentials (provided by the
open-source [tidalapi](https://github.com/tamland/python-tidal) project). If those credentials are ever revoked you can
substitute your own:

1. Register a device-authorization client at [developer.tidal.com](https://developer.tidal.com)
2. Add your credentials to `config.json`:

```json
{
  "client_id": "your-client-id",
  "client_secret": "your-client-secret"
}
```

3. Delete `access_token` and `refresh_token` from the file (or delete the file entirely) to trigger a fresh login with
   your credentials.

## Last.fm Scrobbling

Riptide can automatically scrobble your plays to Last.fm. To enable scrobbling:

### Setup

1. **Create a Last.fm account** (free at [last.fm](https://www.last.fm))
2. **Register an API account** at [https://www.last.fm/api/account/create](https://www.last.fm/api/account/create) to
   get your API key and secret
    1. You don't need to provide a description, callback URL, or application homepage to get your key.
3. **Add credentials to your config** (`~/.config/riptide/config.json`):
   ```json
   "lastfm": {
     "api_key": "your-api-key-here",
     "api_secret": "your-api-secret-here"
   }
   ```
4. **Authorize Riptide** by running:
   ```bash
   cargo run -- --lastfm-auth
   ```
   Or if riptide is installed: `riptide --lastfm-auth`
5. **Open the URL** shown in your terminal and authorize Riptide
6. Riptide will automatically save your session key and enable scrobbling

### Scrobbling Behavior

- Tracks are scrobbled after you've listened for **30 seconds OR 30% of the track duration** (whichever comes first)
- Paused time does not count toward scrobbling
- You can disable scrobbling by setting `"enabled": false` in the `lastfm` section of your config
- View your scrobbles at https://www.last.fm/user/YOUR_USERNAME

### Custom Scrobble Thresholds

You can customize when tracks are scrobbled by adding `min_seconds` and `min_percent` to your Last.fm config:

```json
"lastfm": {
  "username": "your_username",
  "session_key": "...",
  "api_key": "...",
  "api_secret": "...",
  "enabled": true,
  "min_seconds": 45,
  "min_percent": 50
}
```

- `min_seconds`: Minimum seconds to play before scrobbling (default: 30, minimum enforced: 30)
- `min_percent`: Minimum percentage of track to play before scrobbling (default: 30, minimum enforced: 30)

The actual threshold used is whichever is **less** between the two values. For example, with the settings above:

- A 3-minute (180s) song: min (90s, 45s) = 45 seconds
- A 2-minute (120s) song: min (60s, 45s) = 45 seconds
- A 10-second song: min (5s, 45s) = 5 seconds (no minimum enforced for very short tracks)

## Keybindings

Press `?` in the player to view all keybinds. Here's the complete reference:

### Global

| Key         | Action          |
|-------------|-----------------|
| `?`         | Show this help  |
| `q`         | Quit            |
| `/`         | Command palette |
| `Tab`       | Next tab        |
| `Shift+Tab` | Previous tab    |
| `Space`     | Play/Pause      |
| `n`         | Next track      |
| `p`         | Previous track  |
| `z`         | Toggle shuffle  |
| `t`         | Show/hide queue |
| `v`         | Cycle visualizer |
| `V`         | Disable visualizer |
| `+ or =`    | Volume Up       |
| `-`         | Volume Down     |
| `Esc`       | Back/Go up      |

These work everywhere, including while the queue is focused.

### Navigation

| Key     | Action                           |
|---------|----------------------------------|
| `↑`     | Up                               |
| `↓`     | Down                             |
| `Enter` | Select/Open                      |
| `a`     | Add to queue                     |
| `f`     | Toggle favorite/follow/save      |
| `g`     | Go to artist                     |
| `s`     | Sort (opens on the current sort) |
| `r`     | Start radio                      |
| `c`     | Copy share link (song)           |
| `C`     | Copy share link (album/playlist) |
| `→`     | Focus queue                      |

### Queue

| Key     | Action                  |
|---------|-------------------------|
| `↑`     | Up                      |
| `↓`     | Down                    |
| `d`     | Remove track            |
| `c`     | Copy share link (song)  |
| `C`     | Copy share link (album) |
| `g`     | Go to artist            |
| `Enter` | Play track              |
| `t`     | Show/hide queue         |
| `Esc`   | Unfocus queue           |

Hiding the queue with `t` also releases focus, and `→` won't focus it again while it's hidden.

### Search

Jump to the Search tab with `Tab`, or via the command palette.

The search box opens automatically when there are no results yet. Once you have results the tab shows them instead, so
you can leave and come back without losing them — press `/` to start a new search.

| Key     | Action                                 |
|---------|----------------------------------------|
| `/`     | Open the search box                    |
| `↑`     | Up                                     |
| `↓`     | Down                                   |
| `← →`   | Switch pane (Tracks/Artists/Playlists) |
| `Enter` | Run the search, or open a result       |
| `Esc`   | Close the search box                   |

`Tab` and `Shift+Tab` change tabs as usual, even with the search box open.

Note that `/` opens the search box on the Search tab, and the command palette everywhere else.

### Command Palette

Open with `/` (on any tab except Search) and type the start of a destination (Tab to autocomplete):

- `home` — Go to Home
- `tracks` — Go to Tracks
- `artists` — Go to Artists
- `albums` — Go to Albums
- `playlists` — Go to Playlists
- `search` — Open search

## Image Rendering

Album art and artist pictures are rendered using the best available graphics protocol for your terminal:

| Terminal                                   | Protocol       | Quality                   |
|--------------------------------------------|----------------|---------------------------|
| [Kitty](https://sw.kovidgoyal.net/kitty/)  | Kitty graphics | Full color, pixel-perfect |
| [foot](https://codeberg.org/dnkl/foot)     | Sixel          | Full color                |
| [mintty](https://github.com/mintty/mintty) | Sixel          | Full color                |
| Other terminals                            | Half-blocks    | Color approximation       |

The terminal is automatically detected via the `TERM` and `COLORTERM` environment variables. If your terminal supports
Kitty graphics, it will be used. If not, Riptide falls back to Sixel (if supported), and finally to half-block
characters as a universal fallback.

## Logging

Logs are written to `~/.local/share/riptide/riptide.log.<date>` and roll on a daily basis. By default, the logging level
is set to `INFO` providing only errors and important events.

To adjust logging verbosity for debugging, use the `RIPTIDE_LOG_LEVEL` environment variable:

```bash
RIPTIDE_LOG_LEVEL=debug riptide  # Verbose logging (includes all API requests)
RIPTIDE_LOG_LEVEL=info riptide   # Standard logging (errors and important events)
RIPTIDE_LOG_LEVEL=error riptide  # Errors only
```

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE).

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
