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

**Issue:** [#65](https://github.com/fezzik-the-giant/riptide/issues/65) — `Done`
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

The queue pane hides whenever showing it would leave the list under 60 columns
— what a track row needs to keep its artist column — so it shows at 86 columns
and up (`queue_fits` in `src/app/mod.rs`). It stays 26 columns when shown;
widening it on wide terminals was left out. The saved `queue_visible` setting is
unchanged by the collapse. `App` learns the width before each draw so focus
cannot stay in, or move into, a pane that is not on screen.

### Album results in search

**Issue:** [#55](https://github.com/fezzik-the-giant/riptide/issues/55) — `Done`
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

Albums needed their own `/searchResults` request, like the other three panes:
`include=albums.artists`, repeated on every cursor page or those return bare
ids. Track counts come from `numberOfItems`; covers are not requested, because
opening the album fetches its own. The pane sits second in the `Tab` cycle —
Tracks, Albums, Artists, Playlists — and drops Atmos-only albums on arrival,
as the tracks pane does.

### Periodic update check

**Issue:** [#68](https://github.com/fezzik-the-giant/riptide/issues/68) — `Done`
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

### Cheaper Nix CI

**Issue:** [#62](https://github.com/fezzik-the-giant/riptide/issues/62) — `Done` ([PR #70](https://github.com/fezzik-the-giant/riptide/pull/70))
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

The `nix` job in `.github/workflows/ci.yml` took 4–5 minutes against under one
for Lint and Test, and ran far more often than it needed to. `ci.yml` fired on
`push` to every branch *and* on `pull_request`, so each push to a branch with an
open PR built the same tree twice; PR #69 paid for five full Nix builds, one of
them for a `ROADMAP.md`-only commit. Its log splits the time as ~1m50 fetching
store paths, 2m23 building `riptide-deps` (crane's `buildDepsOnly`) and 22 s
building riptide itself — the expensive part changes only with the lockfiles.

Shipped as two changes, neither needing a secret or an outside service:

1. **Triggers:** `push` to `master` plus `pull_request`, and a `concurrency`
   group that cancels superseded runs everywhere but master. A branch with no PR
   gets no CI; open a draft PR for that.
2. **Store cache:** `nix-community/cache-nix-action` keyed on `flake.lock` and
   `Cargo.lock`, falling back to the latest key on a miss. Only master saves, so
   PRs restore master's cache and cannot evict it. The flake's filtered `src`
   already makes Nix's derivation hash decide whether riptide must rebuild,
   which is the "only when packaging files change" filter without a `paths:`
   list that could miss a relevant file.

The guarantee from #60 is unchanged: every master commit, and so every `v*`
tag, has had its flake built. Cachix would pull faster than a GitHub cache
restore and stays the next step if the restore proves slow. Gradient (suggested
on the issue) is a self-hosted Hydra-style server — infrastructure out of
proportion to one flake.

### Playlist editing

**Issue:** [#57](https://github.com/fezzik-the-giant/riptide/issues/57) — `Done`,
delivered as sub-issues #71–#74 (design: `docs/superpowers/specs/2026-10-16-playlist-editing-design.md`)
**Version:** next release (under `[Unreleased]` in `CHANGELOG.md`).

Your own playlists can be created (`N`, or `+ New playlist…` in the `P` picker),
filled from any track row (`P`), and edited: `d` removes an entry, `m` moves one,
`R` renames and `D` deletes after a confirmation. Every write was verified live
first; the facts the code depends on are in `AGENTS.md` under "Playlist Editing —
Verified API". Ownership comes from a separate `filter[owners.id]=me` request,
which is also what the Playlists tab's `Yours` badge reads.

## Considered and Rejected

Nothing yet. Two standing decisions live in `AGENTS.md` rather than here,
because they are properties of the API and not candidates that were weighed:
hi-res playback is unreachable, and stream URLs stay on v1.
