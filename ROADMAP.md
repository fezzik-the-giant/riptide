# Roadmap

Candidate work, with the evidence that each one is actually missing. Nothing
here is committed to a release; the point is that a later session can pick one
up without re-deriving the gap.

No entry is ever deleted. A candidate that ships moves to `## Shipped` with its
version; one that is ruled out moves to `## Considered and Rejected` with the
reason. Left under `## Candidates`, either one gets proposed again by a session
that has no way to know it was already settled.

## Tracking

Every entry here has a matching issue on the **Riptide Development** board:
<https://github.com/users/fezzik-the-giant/projects/2>. The two halves carry
different things and neither replaces the other — this file holds the evidence
and the open questions, the board holds the state. An entry with no issue is
invisible to anyone not reading the repo; an issue with no entry loses the
reasoning the moment its author forgets it.

**Adding an entry means filing its issue.** Use the repo's existing title
convention (`[FEATURE] …`, `[Bug] …`), open it against
`fezzik-the-giant/riptide`, add it to the project, and record the number on the
entry's `**Issue:**` line here. The issue body should link back to this file
rather than restate it.

**Move the board status the moment the work moves**, not in a sweep afterwards:

| Board status  | What it means here                                           |
|---------------|--------------------------------------------------------------|
| `Backlog`     | A candidate below, with open questions still unanswered       |
| `Ready`       | Questions settled and a design approved; nobody has started   |
| `In progress` | A feature branch exists                                       |
| `In review`   | A PR is open                                                  |
| `Done`        | Merged — move the entry to `## Shipped` in the same breath    |

A candidate that is rejected closes its issue with the reason, and the entry
moves to `## Considered and Rejected`. The board status and this file disagreeing
is the failure mode the two-way link exists to prevent, so whichever one you
touch, touch the other before you move on.

## Candidates

### Repeat / loop mode

**Issue:** [#56](https://github.com/fezzik-the-giant/riptide/issues/56) — `Backlog`

Shuffle is a persisted preference with a keybind, a settings row and an MPRIS
property. Repeat is absent everywhere: `loop_status` in `src/mpris.rs` returns a
hardcoded `"None"`, no key in `src/app/state/keybinds.rs` binds it, and
`Preferences` in `src/app/state/sort.rs` has no field for it.

Off / repeat-queue / repeat-track, to match what MPRIS `LoopStatus` already
names, and the property should become settable rather than constant.

This one lands in the middle of the mpv prefetch machinery — see "mpv's Playlist
Is Not the Queue" in `AGENTS.md`. Repeat-track means `SetNext` queues the stream
that is already playing, and repeat-queue means the track after the last one is
the first one, which `TrackEnded` currently treats as the end. The `FakeMpv`
tests in `src/app/playback.rs` are where that gets proven.

### Playlist editing

**Issue:** [#57](https://github.com/fezzik-the-giant/riptide/issues/57) — `Backlog`

`src/api/client/playlists.rs` can `save_playlist` and `remove_playlist` —
add and remove someone else's playlist from the collection — and nothing else.
There is no create, no add-track-to-playlist, no remove-track-from-playlist. A
track can be favorited but not filed.

Likely the largest user-facing win of anything here, and the only entry with an
unverified API. The v2 write endpoints have to be confirmed against the live API
before any design: whether `/playlists` takes a POST, whether items move through
`/playlists/{id}/relationships/items` the way `userCollection*` relationships do,
and what a created playlist's response carries. Ask before assuming field names.

The UI question is separate and also open: a picker over the user's own
playlists, filtered to the ones they can write to.

### Seek from the keyboard

**Issue:** [#58](https://github.com/fezzik-the-giant/riptide/issues/58) — `Backlog`

`PlayerCmd::SeekAbsolute` (`src/player.rs`) exists, `App::seek_by_us`
(`src/app/playback.rs`) drives it, and the only caller is `MprisCmd::Seek` in
`src/events/mod.rs`. So `playerctl position 30+` seeks and the keyboard cannot.
No seek row appears anywhere in the help modal.

Smallest item here. The real question is which keys: `←` / `→` switch carousel
tabs, so seek needs something else — `,` / `.`, `[` / `]`, or a modifier on the
arrows.

### Resume the last session

**Issue:** [#59](https://github.com/fezzik-the-giant/riptide/issues/59) — `Backlog`

`Preferences` persists sorts, volume, shuffle, queue visibility and Atmos. It
does not persist the queue, the playing track or its position, so a restart
drops whatever was lined up.

Worth scoping before designing: restoring the queue is cheap, restoring the
playback position within a track is a different feature, and restoring either
one means deciding whether riptide starts playing on launch (it should not).

## Shipped

`CHANGELOG.md` stays the user-facing record of what a release contained; an
entry moved here only needs the version and a line on what landed, so a later
session can tell the work is done without reading the changelog.

### PR #40 — CAVA visualizer

**Issue:** none — tracked as [PR #40](https://github.com/fezzik-the-giant/riptide/pull/40) instead.
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

@dghelm's draft, finished by the maintainer. The dependency question was settled
in favour of spawning `cava` as an optional child process: embedding cavacore
would mean capturing and buffering PCM ourselves for no gain, and a missing
`cava` costs only the spectrum. The mode also became a `Shift+S` row.

### Now-playing bar that scales with the terminal

**Issue:** [#63](https://github.com/fezzik-the-giant/riptide/issues/63) — `Done`
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

The bar takes a third of the terminal's height, capped at 30 rows
(`now_playing_height` in `src/ui/now_playing.rs`). Below 11 rows — a 6-row
cover plus border and info — it collapses to just the border and the track
info, dropping cover and lyrics rather than squeezing the art to a thumbnail.
A 48-row terminal gets the old fixed 16. The cover is sized from the
terminal's real cell size, so it fills its rows and sits directly on the
track info.

### Queue pane that collapses on narrow terminals

**Issue:** [#65](https://github.com/fezzik-the-giant/riptide/issues/65) — `In review`
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

The queue pane hides whenever showing it would leave the list under 60 columns
— what a track row needs to keep its artist column — so it shows at 86 columns
and up (`queue_fits` in `src/app/mod.rs`). It stays 26 columns when shown;
widening it on wide terminals was left out. The saved `queue_visible` setting is
unchanged by the collapse. `App` learns the width before each draw so focus
cannot stay in, or move into, a pane that is not on screen.

### Album results in search

**Issue:** [#55](https://github.com/fezzik-the-giant/riptide/issues/55) — `In review`
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

Albums needed their own `/searchResults` request, like the other three panes:
`include=albums.artists`, repeated on every cursor page or those return bare
ids. Track counts come from `numberOfItems`; covers are not requested, because
opening the album fetches its own. The pane sits second in the `Tab` cycle —
Tracks, Albums, Artists, Playlists — and drops Atmos-only albums on arrival,
as the tracks pane does.

### Periodic update check

**Issue:** [#68](https://github.com/fezzik-the-giant/riptide/issues/68) — `In review`
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

The update actor ran one check 3 s after startup and never again, and none at
all on package-managed installs. `tick` now sends `UpdateCmd::Check` once the
last result is `UPDATE_RECHECK_INTERVAL` (10 minutes) old — six requests an hour
against GitHub's unauthenticated limit of 60 per IP. It skips an open dialog
and a staged install. A failed check keeps the release it already knew about,
and the footer leads with the notice so a narrow terminal truncates the hint
instead.

Package-managed installs check too. The actor reports the `InstallMethod`, and
`InstallMethod::upgrade_hint` words the footer and `U` for it — `U` handles
those installs before `check_done`, so a found release can never open the
installer over a binary a package manager owns. The same change fixed COPR
installs being classed as `Script`: ownership is now asked of `rpm -qf` after
`pacman -Qo`, both under `LC_ALL=C` so their "not owned" text is English.

## Considered and Rejected

Nothing yet. Two standing decisions live in `AGENTS.md` rather than here,
because they are properties of the API and not candidates that were weighed:
hi-res playback is unreachable, and stream URLs stay on v1.
