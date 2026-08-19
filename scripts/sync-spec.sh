#!/usr/bin/env bash
# Sync riptide.spec to the version in Cargo.toml, deriving the RPM %changelog
# entry from CHANGELOG.md.
#
# Run this when staging a release, before committing, so the tag and the spec
# agree. COPR builds the spec as committed in git rather than the tag payload,
# so a spec updated after the tag would ship the wrong version — see the
# copr-build comment in .github/workflows/release.yml.
#
#   ./scripts/sync-spec.sh
#   SPEC_PACKAGER="Name <you@example.com>" ./scripts/sync-spec.sh
#
# Idempotent: re-running for a version already in the %changelog does nothing.

set -euo pipefail

cd "$(dirname "$0")/.."

SPEC=riptide.spec
CHANGELOG=CHANGELOG.md
PACKAGER="${SPEC_PACKAGER:-Ryan Cohan <noreply@github.com>}"

version=$(sed -n '0,/^version = /s/^version = "\(.*\)"/\1/p' Cargo.toml)
if [ -z "$version" ]; then
    echo "error: no version found in Cargo.toml" >&2
    exit 1
fi

if grep -q "^\* .* - ${version}-" "$SPEC"; then
    echo "riptide.spec already has a %changelog entry for $version — nothing to do."
    exit 0
fi

# "## [1.1.0] - 2026-08-17" -> the release date, so the weekday rpmlint checks
# matches the date rather than whenever this script happened to run.
date_iso=$(sed -n "s/^## \[${version}\] - \([0-9-]*\)$/\1/p" "$CHANGELOG" | head -1)
if [ -z "$date_iso" ]; then
    echo "error: CHANGELOG.md has no '## [$version] - <date>' heading" >&2
    exit 1
fi
date_rpm=$(date -u -d "$date_iso" "+%a %b %d %Y")

# Flatten the release's notes into RPM changelog bullets: "Fixed: <first
# sentence>". The full prose lives in CHANGELOG.md; this is the summary rpm
# users see in `rpm -q --changelog`.
entry=$(awk -v ver="$version" '
    index($0, "## [" ver "]") == 1 { inside = 1; next }
    /^## \[/ { if (inside) exit; next }
    !inside { next }
    /^### / { section = substr($0, 5); next }
    /^- / {
        line = substr($0, 3)
        gsub(/`/, "", line)
        # First sentence only: the notes lead with the user-visible statement
        # and then explain the cause, which is more than an rpm changelog wants.
        if (match(line, /\. /)) line = substr(line, 1, RSTART)
        printf "- %s: %s\n", section, line
    }
' "$CHANGELOG")

if [ -z "$entry" ]; then
    echo "error: no bullets found under '## [$version]' in CHANGELOG.md" >&2
    exit 1
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

awk -v ver="$version" -v date_rpm="$date_rpm" -v packager="$PACKAGER" -v entry="$entry" '
    /^Version:/ { printf "Version:        %s\n", ver; next }
    /^Release:/ { print "Release:        1%{?dist}"; next }
    /^%changelog$/ {
        print
        printf "* %s %s - %s-1\n", date_rpm, packager, ver
        print entry
        print ""
        next
    }
    { print }
' "$SPEC" > "$tmp"

mv "$tmp" "$SPEC"
trap - EXIT

echo "riptide.spec synced to $version ($date_rpm)."
