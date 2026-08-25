// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Self-update support for installations made outside a package manager.
//!
//! Enabled only when the running binary is not managed by pacman, Nix, or
//! Cargo — i.e. it was placed on disk via `install.sh` or a manual download
//! from GitHub Releases. See [`install_method`].

use anyhow::{Context, Result, bail};
use std::io::Read as _;
use std::path::{Path, PathBuf};

const GITHUB_REPO: &str = "fezzik-the-giant/riptide";

/// How the running binary was installed. Self-update only applies to
/// [`InstallMethod::Script`] — anything else has its own package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMethod {
    /// Owned by pacman (AUR `riptide` package). Updated via yay/paru.
    Pacman,
    /// Inside `/nix/store`. Managed by the user's flake. Read-only — never updatable.
    Nix,
    /// Inside `~/.cargo/bin`. Updated via `cargo install`.
    Cargo,
    /// Placed by `install.sh` or a manual release download. Self-update applies.
    Script,
}

/// Information parsed from a GitHub release JSON payload.
pub struct ReleaseInfo {
    pub tag: String,
    /// Exact file name of the matched `.tar.gz` asset (e.g. `riptide-v0.14.0-x86_64-linux-gnu.tar.gz`).
    /// Carried so SHA256SUMS lookup uses the real asset name rather than
    /// re-deriving it from `tag`, which can diverge (e.g. tag `0.14.0` vs asset `v0.14.0`).
    pub asset_name: String,
    /// Download URL of the `.tar.gz` asset matching this platform.
    pub tarball_url: String,
    /// Download URL of the `SHA256SUMS` asset, if present.
    pub checksums_url: Option<String>,
}

/// Platform triple used in release asset names (mirrors install.sh).
fn target_binary_triple() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else {
        // Compile-time fallback: updates for this platform will never find a
        // matching asset and fail gracefully at check time.
        "unknown-unknown-unknown"
    }
}

/// Parse `"v1.2.3"` / `"1.2.3"` (with optional prerelease suffix like
/// `"-beta"` / `"+build"`) into numeric components. Accepts `MAJOR.MINOR`
/// (patch defaults to 0). Extra dot components beyond the third are dropped,
/// so a four-part tag (e.g. `0.14.0.1`) compares equal to its three-part
/// prefix — it does not register as newer and the update is skipped.
fn parse_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    // Strip prerelease / build metadata before numeric parsing.
    let core = tag.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    Some((major, minor, patch))
}

/// True when `tag` parses to a version strictly newer than `current`.
pub fn version_is_newer(tag: &str, current: &str) -> bool {
    match (parse_tag(tag), parse_tag(current)) {
        (Some(remote), Some(local)) => remote > local,
        _ => false,
    }
}

/// True when `name` is the `.tar.gz` asset for the current platform
/// (as opposed to a checksum file, another platform, or the bare binary).
fn is_our_binary_asset(name: &str) -> bool {
    name.starts_with("riptide-v") && name.ends_with(&format!("-{}.tar.gz", target_binary_triple()))
}

/// Host suffixes we will download release assets from.
const GITHUB_HOST_SUFFIXES: &[&str] = &["github.com", "githubusercontent.com"];

/// True when `url` is https and points at a GitHub release host. The TLS pin
/// in [`self_update_with_cancel`]'s trust model only holds if the payload
/// comes from the expected infrastructure.
fn is_github_download_url(url: &str) -> bool {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    if !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let host = host.trim_end_matches('.');
    !host.is_empty()
        && GITHUB_HOST_SUFFIXES
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

/// Parse a GitHub release JSON payload, selecting the assets for this platform.
fn release_info_from_json(json: &str) -> Result<ReleaseInfo> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid release JSON")?;
    let tag = v["tag_name"]
        .as_str()
        .context("release JSON has no tag_name")?
        .to_string();

    let assets = v["assets"]
        .as_array()
        .context("release JSON has no assets")?;
    let mut asset_name: Option<String> = None;
    let mut tarball_url: Option<String> = None;
    for a in assets {
        let Some(name) = a["name"].as_str() else {
            continue;
        };
        if is_our_binary_asset(name) && tarball_url.is_none() {
            asset_name = Some(name.to_string());
            tarball_url = a["browser_download_url"].as_str().map(str::to_string);
        }
    }
    let tarball_url = tarball_url.with_context(|| {
        format!(
            "no asset for platform {} in release {tag}",
            target_binary_triple()
        )
    })?;
    if !is_github_download_url(&tarball_url) {
        bail!("release asset URL {tarball_url} is not a GitHub https URL");
    }
    let asset_name = asset_name.expect("matched asset has a name");
    let checksums_url = assets
        .iter()
        .find_map(|a| {
            if a["name"].as_str() == Some("SHA256SUMS") {
                a["browser_download_url"].as_str().map(str::to_string)
            } else {
                None
            }
        })
        .filter(|url| is_github_download_url(url));

    Ok(ReleaseInfo {
        tag,
        asset_name,
        tarball_url,
        checksums_url,
    })
}

/// Look up the expected sha256 for `asset_name` in `SHA256SUMS` content.
fn parse_sha256sums(content: &str, asset_name: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let filename = parts.next()?;
        if filename == asset_name {
            Some(hash.to_string())
        } else {
            None
        }
    })
}

/// Lowercase hex sha256 digest of `data`.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(data))
}

/// Extract the `riptide` binary from a gzipped tarball into `dir`.
/// Returns the path of the extracted file. Hardened against symlink and
/// traversal attacks: only regular-file entries whose normalized path ends
/// in `riptide` (accepting `./riptide` and `dir/riptide`) are unpacked.
fn extract_binary_from_tarball(tarball: &[u8], dir: &Path) -> Result<PathBuf> {
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().context("cannot read tarball")? {
        let mut entry = entry.context("corrupt tarball entry")?;
        // Reject symlinks, hardlinks, directories, pax extensions.
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("entry has no path")?;
        // Normalize: strip leading `./`, reject absolute and `..` components.
        let mut comps: Vec<std::ffi::OsString> = Vec::new();
        let mut valid = true;
        for c in path.components() {
            match c {
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    valid = false;
                    break;
                }
                std::path::Component::CurDir => continue,
                std::path::Component::ParentDir => {
                    valid = false;
                    break;
                }
                std::path::Component::Normal(os) => comps.push(os.to_os_string()),
            }
        }
        if !valid || comps.is_empty() {
            continue;
        }
        // Accept `riptide` and `dir/riptide` (e.g. `./riptide` or `riptide-v0.14.0/riptide`).
        let is_binary = comps.last().is_some_and(|n| n == "riptide") && comps.len() <= 2;
        if !is_binary {
            continue;
        }
        let dest = dir.join("riptide");
        entry
            .unpack(&dest)
            .context("cannot unpack riptide binary")?;
        // Verify the unpacked path is a regular file, not a symlink left behind.
        let meta = std::fs::symlink_metadata(&dest).context("unpacked binary missing")?;
        if !meta.is_file() {
            std::fs::remove_file(&dest).ok();
            bail!("tarball entry was not a regular file");
        }
        return Ok(dest);
    }
    bail!("tarball contains no 'riptide' binary")
}

/// Atomically replace `target` with `staged` via rename, preserving the
/// running process (writing over a running binary would fail with ETXTBSY).
fn swap_binary(target: &Path, staged: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(staged)
            .context("staged binary disappeared")?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(staged, perms).context("cannot chmod staged binary")?;
        // Ensure data is durable before the atomic rename, and the directory
        // entry is durable after. Without fsync, a power loss on
        // data=writeback / XFS / btrfs can leave a zero-length binary.
        if let Ok(f) = std::fs::File::open(staged) {
            let _ = f.sync_all();
        }
    }
    std::fs::rename(staged, target)
        .with_context(|| format!("cannot replace {}", target.display()))?;
    #[cfg(unix)]
    {
        if let Some(dir) = target.parent()
            && let Ok(d) = std::fs::File::open(dir)
        {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

/// True when `path` lives under a `.cargo/bin` directory (any user's —
/// cargo binaries are always laid out as `<home>/.cargo/bin/<name>`).
fn is_under_cargo_bin(path: &Path) -> bool {
    if let Ok(cargo_home) = std::env::var("CARGO_HOME")
        && !cargo_home.is_empty()
    {
        let bin = Path::new(&cargo_home).join("bin");
        if path.parent().is_some_and(|p| p == bin) {
            return true;
        }
    }
    path.parent()
        .is_some_and(|p| p.ends_with(Path::new(".cargo").join("bin")))
}

/// True when `path` lives under the Nix store. Canonicalizes symlinks so that
/// `/run/current-system/sw/bin/riptide` is correctly classified.
fn is_under_nix_store(path: &Path) -> bool {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        if canonical.starts_with("/nix/store") {
            return true;
        }
    }
    path.starts_with("/nix/store")
}

/// Linux only: true when pacman owns `path` (i.e. it came from an AUR/ALPM package).
/// Conservative on ambiguous failures: only a definitive "No package owns"
/// response is treated as not-owned; db-lock or timeout is assumed owned to
/// avoid overwriting a package-managed binary. A missing `pacman` binary
/// (spawn failure) is treated as not-owned — on non-Arch systems there is no
/// pacman to own anything. The query is executed on a helper thread with a
/// 2 s bound so a stuck ALPM db cannot stall the caller.
#[cfg(target_os = "linux")]
fn is_pacman_owned(path: &Path) -> bool {
    use std::process::Stdio;
    use std::sync::mpsc as std_mpsc;
    use std::time::Duration;

    let path = path.to_path_buf();
    let (tx, rx) = std_mpsc::channel();
    // Resolve pacman's path on the leading thread so spawn failure is distinguishable.
    let path_for_query = path.clone();
    let query = std::thread::spawn(move || {
        let out = std::process::Command::new("pacman")
            .arg("-Qo")
            .arg(&path_for_query)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let _ = tx.send(out);
    });
    let out = match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(o) => o,
        Err(_) => {
            // Timeout or disconnected sender. Detach the query thread (it may
            // still be alive) and fall through to conservative ownership.
            drop(query);
            tracing::warn!(
                "pacman -Qo timed out for {}; assuming pacman-owned",
                path.display()
            );
            return true;
        }
    };
    let _ = query.join();
    match out {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{stderr} {stdout}");
            if combined.contains("No package owns") {
                false
            } else {
                // Ambiguous failure (db locked, pacman missing, etc.) — assume owned to stay safe.
                tracing::warn!(
                    "pacman -Qo failed ambiguously for {}: {combined}; assuming pacman-owned",
                    path.display()
                );
                true
            }
        }
        Err(_) => false,
    }
}

/// True when a process with the given pid currently exists. `kill(pid, 0)`
/// only probes existence — a running or zombie process (Ok) and a live process
/// owned by another user (EPERM) both count as alive; only ESRCH means gone.
#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    // Conservative: if we can't check, assume alive so we never reap something
    // a live instance still owns.
    true
}

/// Remove a root-owned staged file left by `try_sudo_swap` when we lack
/// permission to delete it ourselves. Only unprivileged failure paths route
/// here; sudo prompting is avoided because all calls are `-n` (non-interactive).
fn remove_sudo_staged(path: &Path) {
    if std::fs::remove_file(path).is_ok() {
        return;
    }
    let _ = std::process::Command::new("sudo")
        .arg("-n")
        .arg("rm")
        .arg("-f")
        .arg("--")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Remove stale self-update artifacts left by a previous cancelled or crashed
/// update. Called at startup so a stranded `/tmp/riptide-update-*` or
/// `<installdir>/.riptide.new-*` does not accumulate per cancellation.
///
/// Only artifacts from *dead* processes are reaped: the pid is the first
/// `-`-separated field of the randomized name (e.g. `riptide-update-1234-a1b2c3d4`).
/// A live pid is left alone so a concurrently running instance's in-flight
/// update is never destroyed mid-swap.
pub fn cleanup_stale_artifacts() {
    let reap = |name: &str| {
        name.split('-')
            .nth(2)
            .and_then(|p| p.parse::<u32>().ok())
            .map(|pid| !pid_is_alive(pid))
            .unwrap_or(false)
    };
    // Temp dirs: riptide-update-<pid>-<rnd>
    let tmp = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&tmp) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if s.starts_with("riptide-update-") && reap(&s) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    // Staged binaries next to the running executable.
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy();
        let exe_path = if let Some(stripped) = exe_str.strip_suffix(" (deleted)") {
            Path::new(stripped).to_path_buf()
        } else {
            exe
        };
        if let Some(dir) = exe_path.parent()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let n = entry.file_name();
                let s = n.to_string_lossy();
                if s.starts_with(".riptide.new-") && reap(&s) {
                    let _ = std::fs::remove_file(entry.path());
                } else if s.starts_with(".riptide.sudo-staged-") && reap(&s) {
                    // May be root-owned if the previous updater was killed
                    // between `sudo cp` and `sudo mv`; escalate to remove it.
                    remove_sudo_staged(&entry.path());
                }
            }
        }
    }
}

/// Classify an installation given the resolved path of the running binary.
/// Heuristics over the path; touches the filesystem to canonicalize Nix
/// symlinks and to confirm the binary exists before asking pacman, and (on
/// Linux) shells out to `pacman -Qo` as the tie-breaker.
fn install_method_from_path(exe: &Path) -> InstallMethod {
    if is_under_nix_store(exe) {
        return InstallMethod::Nix;
    }
    if is_under_cargo_bin(exe) {
        return InstallMethod::Cargo;
    }
    // pacman is the tie-breaker for paths that match no convention; only ask
    // it about real files to avoid a slow negative lookup for synthetic paths.
    #[cfg(target_os = "linux")]
    if exe.exists() && is_pacman_owned(exe) {
        return InstallMethod::Pacman;
    }
    InstallMethod::Script
}

/// Classify the currently running binary.
pub fn install_method() -> InstallMethod {
    match std::env::current_exe() {
        Ok(exe) => install_method_from_path(&exe),
        Err(_) => InstallMethod::Script, // cannot tell; fail open but never updatable in practice
    }
}

/// Query GitHub for the latest release and parse it for this platform.
pub fn latest_release() -> Result<ReleaseInfo> {
    let client = http_client()?;
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .with_context(|| format!("cannot reach {url}"))?
        .error_for_status()
        .context("GitHub releases returned an error")?
        .text()
        .context("cannot read release body")?;
    release_info_from_json(&body)
}

/// Result of an update attempt, reported to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// Already running the latest released version.
    AlreadyCurrent,
    /// Updated from the running version to the given tag; restart required.
    Updated(String),
}

/// Check GitHub for a newer release and, if found, download, verify, and
/// atomically install it over the running binary. Call only when
/// [`install_method`] returned [`InstallMethod::Script`].
///
/// `cancel` lets the TUI abort an attempt (e.g. user quits while the download
/// runs); it is polled between each stage, so at most one stage runs after
/// cancellation. The staged file is always cleaned before a cancel bail.
///
/// **Trust model:** release integrity is checksum-only. Downloads are pinned
/// to the hardcoded [`GITHUB_REPO`] over TLS and the tarball must match
/// `SHA256SUMS` (fail-closed), but neither asset is cryptographically signed
/// by the maintainer. A compromised GitHub release could substitute a tarball
/// *and* a matching checksum. Maintainer signing (minisign/cosign) is a
/// release-infrastructure decision pending in the parent project; this code
/// does not invent one.
pub fn self_update_with_cancel(cancel: &std::sync::atomic::AtomicBool) -> Result<UpdateOutcome> {
    let release = latest_release()?;
    let current = env!("CARGO_PKG_VERSION");
    if !version_is_newer(&release.tag, current) {
        return Ok(UpdateOutcome::AlreadyCurrent);
    }
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        bail!("update cancelled");
    }
    tracing::warn!(
        "self-update uses checksum-only verification (no maintainer signature); \
         trust relies on TLS to {GITHUB_REPO}"
    );

    let client = http_client_for_download()?;
    // Randomized temp dir; created non-following below.
    let tmp = {
        let rnd: u32 = rand::random();
        std::env::temp_dir().join(format!("riptide-update-{}-{rnd:08x}", std::process::id()))
    };
    if tmp.exists() {
        let meta = std::fs::symlink_metadata(&tmp)?;
        if meta.file_type().is_symlink() {
            bail!("refusing to use symlink temp dir {}", tmp.display());
        }
        // A stale directory from a previous crash is safe to replace.
        std::fs::remove_dir_all(&tmp).ok();
    }
    std::fs::create_dir(&tmp).context("cannot create temp dir")?;
    let result = download_and_install(&client, &release, &tmp, cancel);
    std::fs::remove_dir_all(&tmp).ok();
    result.map(|()| UpdateOutcome::Updated(release.tag.clone()))
}

/// Backwards-compatible convenience wrapper: no cancellation.
pub fn self_update() -> Result<UpdateOutcome> {
    self_update_with_cancel(&std::sync::atomic::AtomicBool::new(false))
}

/// Inner download/verify/install sequence, factored out so the temp dir is
/// always cleaned up on error paths. Checksum verification is fail-closed:
/// if the release advertises a SHA256SUMS asset it must be downloaded and
/// contain a matching entry, otherwise the install is aborted.
fn download_and_install(
    client: &reqwest::blocking::Client,
    release: &ReleaseInfo,
    tmp: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    let tarball_path = tmp.join("riptide.tar.gz");
    download_to_file(client, &release.tarball_url, &tarball_path)?;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        bail!("update cancelled after download");
    }
    let tarball = std::fs::read(&tarball_path).context("cannot read downloaded tarball")?;

    // Verify against the SHA256SUMS asset when present. Failure to download
    // or a missing entry is a hard error (fail-closed) to prevent
    // installation of an unverified tarball.
    let asset = &release.asset_name;
    if let Some(url) = &release.checksums_url {
        let sums_path = tmp.join("SHA256SUMS");
        download_to_file(client, url, &sums_path).context("cannot download SHA256SUMS")?;
        let sums = std::fs::read_to_string(&sums_path).context("cannot read SHA256SUMS")?;
        let expected = parse_sha256sums(&sums, &asset)
            .with_context(|| format!("SHA256SUMS has no entry for {asset}"))?;
        let actual = sha256_hex(&tarball);
        if actual != expected {
            bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
        }
    } else {
        bail!(
            "release {} has no SHA256SUMS asset; refusing to install unverified tarball",
            release.tag
        );
    }
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        bail!("update cancelled after checksum verification");
    }

    let extracted = extract_binary_from_tarball(&tarball, tmp)?;
    let exe = std::env::current_exe().context("cannot locate running binary")?;
    // Linux reports "/path/riptide (deleted)" if another process already
    // replaced the binary; strip the suffix so the rename targets the real file.
    let target = {
        let s = exe.to_string_lossy();
        if let Some(stripped) = s.strip_suffix(" (deleted)") {
            Path::new(stripped).to_path_buf()
        } else {
            exe
        }
    };
    let dir = target.parent().context("binary path has no parent dir")?;

    // Stage the new binary *in the target directory*: rename() is atomic but
    // does not cross filesystems, and /tmp is usually a different mount.
    // Use a randomized suffix to avoid collisions between concurrent updaters.
    let staged = {
        let rnd: u32 = rand::random();
        dir.join(format!(".riptide.new-{}-{rnd:08x}", std::process::id()))
    };
    // Ensure staged path is not a pre-existing symlink.
    if let Ok(meta) = std::fs::symlink_metadata(&staged) {
        if meta.file_type().is_symlink() {
            bail!("refusing to use symlink staged path {}", staged.display());
        }
        let _ = std::fs::remove_file(&staged);
    }
    // Guard to clean staged file on any error or cancel path.
    let staged_guard = StagedGuard {
        path: Some(staged.clone()),
    };
    match std::fs::copy(&extracted, &staged) {
        Ok(_) => {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                // Drop the guard to remove the staged file, then bail.
                drop(staged_guard);
                bail!("update cancelled before install");
            }
            // Defuse guard before rename — swap_binary will consume staged on success,
            // or we clean it explicitly on failure.
            let staged_path = staged_guard.defuse();
            if let Err(err) = swap_binary(&target, &staged_path) {
                let _ = std::fs::remove_file(&staged_path);
                return Err(err)
                    .context("cannot replace binary; re-run install.sh or `sudo riptide update`");
            }
        }
        Err(e) => {
            drop(staged_guard);
            // Only a permission problem warrants the sudo escalation — the
            // install.sh default of /usr/local/bin. Anything else (disk full,
            // unreadable extract, I/O error) is the real fault; reporting it
            // as "re-run install.sh" would misdiagnose it. Log so riptide.log
            // keeps the errno.
            if e.kind() != std::io::ErrorKind::PermissionDenied {
                tracing::warn!("staging copy failed: {e}");
                return Err(e).context(format!("cannot stage binary into {}", dir.display()));
            }
            // Retry through sudo, which handles the cross-fs copy as root.
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                bail!("update cancelled before sudo install");
            }
            if !try_sudo_swap(&target, &extracted) {
                bail!(
                    "cannot write to {} as this user and sudo refused — re-run install.sh: \
                     curl -fsSL https://raw.githubusercontent.com/{GITHUB_REPO}/master/install.sh | bash",
                    dir.display()
                );
            }
        }
    }
    Ok(())
}

/// RAII guard that removes the staged file if not explicitly defused.
struct StagedGuard {
    path: Option<PathBuf>,
}

impl StagedGuard {
    fn defuse(mut self) -> PathBuf {
        self.path.take().expect("defuse called twice")
    }
}

impl Drop for StagedGuard {
    fn drop(&mut self) {
        if let Some(p) = &self.path {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Run `sudo -n <args>`, logging the outcome. Returns true on exit status 0.
/// On failure the log line records the command and exit code / spawn error so
/// a refused or missing sudo is diagnosable from riptide.log instead of
/// silently collapsing to false.
fn sudo_run(args: &[&std::ffi::OsStr]) -> bool {
    let printable: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    match std::process::Command::new("sudo")
        .arg("-n")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => true,
        Ok(s) => {
            tracing::warn!("sudo -n {} exited {s}", printable.join(" "));
            false
        }
        Err(e) => {
            tracing::warn!("sudo -n {} failed to spawn: {e}", printable.join(" "));
            false
        }
    }
}

/// Last-resort binary replacement via sudo (non-interactive). Returns false
/// when sudo is absent or refuses. Avoids shell interpolation by invoking
/// cp/chmod/mv directly with separate sudo calls.
fn try_sudo_swap(target: &Path, staged: &Path) -> bool {
    let Some(dir) = target.parent() else {
        return false;
    };
    let sudo_tmp = {
        let rnd: u32 = rand::random();
        dir.join(format!(
            ".riptide.sudo-staged-{}-{rnd:08x}",
            std::process::id()
        ))
    };
    // Remove any pre-existing symlink at sudo_tmp.
    if let Ok(meta) = std::fs::symlink_metadata(&sudo_tmp) {
        if meta.file_type().is_symlink() {
            tracing::warn!("refusing to use symlink sudo_tmp {}", sudo_tmp.display());
            return false;
        }
        let _ = std::fs::remove_file(&sudo_tmp);
    }
    let cleanup = |path: &Path| {
        sudo_run(&[
            std::ffi::OsStr::new("rm"),
            std::ffi::OsStr::new("-f"),
            std::ffi::OsStr::new("--"),
            path.as_os_str(),
        ]);
    };
    if !sudo_run(&[
        std::ffi::OsStr::new("cp"),
        std::ffi::OsStr::new("--"),
        staged.as_os_str(),
        sudo_tmp.as_os_str(),
    ]) {
        return false;
    }
    if !sudo_run(&[
        std::ffi::OsStr::new("chmod"),
        std::ffi::OsStr::new("755"),
        std::ffi::OsStr::new("--"),
        sudo_tmp.as_os_str(),
    ]) {
        cleanup(&sudo_tmp);
        return false;
    }
    if !sudo_run(&[
        std::ffi::OsStr::new("mv"),
        std::ffi::OsStr::new("--"),
        sudo_tmp.as_os_str(),
        target.as_os_str(),
    ]) {
        cleanup(&sudo_tmp);
        return false;
    }
    true
}

/// Entry point for `riptide update`. Prints progress to stdout and exits
/// non-zero on failure so scripts can detect it.
pub fn run_update_cli() -> Result<()> {
    match install_method() {
        InstallMethod::Pacman => {
            bail!("installed via pacman/AUR — update with your AUR helper (e.g. `yay -Syu`)")
        }
        InstallMethod::Nix => {
            bail!("installed via Nix — update the riptide input/flake instead")
        }
        InstallMethod::Cargo => {
            bail!(
                "installed via cargo — update with `cargo install --path .` (or --locked riptide)"
            )
        }
        InstallMethod::Script => {}
    }

    println!("Checking for updates…");
    match self_update()? {
        UpdateOutcome::AlreadyCurrent => {
            println!("Already up to date (v{}).", env!("CARGO_PKG_VERSION"));
        }
        UpdateOutcome::Updated(tag) => {
            println!("Updated to {tag}. Restart riptide to use the new version.");
        }
    }
    Ok(())
}

/// Send `message` on an update-check channel, logging if the TUI is gone.
pub(crate) fn sending_check(
    tx: &tokio::sync::mpsc::UnboundedSender<Result<Option<String>, String>>,
    message: Result<Option<String>, String>,
) {
    if tx.send(message).is_err() {
        tracing::debug!("update check result undeliverable; TUI closed");
    }
}

pub(crate) fn check_for_update_assuming_script() -> Result<Option<String>, String> {
    let release = latest_release().map_err(|e| {
        tracing::warn!("update check failed: {e:#}");
        format!("{e:#}")
    })?;
    if version_is_newer(&release.tag, env!("CARGO_PKG_VERSION")) {
        Ok(Some(release.tag))
    } else {
        Ok(None)
    }
}

/// Blocking reqwest client. Proxies are configured explicitly because
/// reqwest's system-proxy autodetection was removed in 0.12. `HTTPS_PROXY`
/// takes precedence over the generic `ALL_PROXY`; `HTTP_PROXY` is ignored
/// because all endpoints here are https. `NO_PROXY`/`no_proxy` are honored
/// for GitHub hostnames so a local bypass still applies. Invalid URLs are
/// skipped with a warning rather than aborting.
fn http_client() -> Result<reqwest::blocking::Client> {
    let mut builder = base_client_builder()?;
    builder = builder.timeout(std::time::Duration::from_secs(30));
    builder.build().context("cannot build HTTP client")
}

/// Hops allowed while chasing an asset URL. GitHub uses exactly one; the
/// slack is for infrastructure changes, not for following a chain anywhere.
const MAX_DOWNLOAD_REDIRECTS: usize = 5;

/// Like [`http_client`] but with more headroom for a large body. On the
/// *blocking* client `timeout` is per-operation, not a whole-request deadline
/// — it bounds the wait for response headers and then restarts for each
/// `read()` — so a slow-but-progressing download is never cut off, while a
/// server that stalls mid-body still fails. Passing `None` here would remove
/// both bounds and park the update actor forever. Body size is capped
/// separately in `download_to_file`.
fn http_client_for_download() -> Result<reqwest::blocking::Client> {
    let mut builder = base_client_builder()?;
    builder = builder
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(30))
        // `browser_download_url` always answers 302 for the asset CDN, so
        // refusing redirects outright means no download ever completes. Every
        // hop is re-validated instead: the trust model is "the bytes came from
        // GitHub", which the first URL alone cannot establish.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > MAX_DOWNLOAD_REDIRECTS {
                return attempt.error(format!(
                    "more than {MAX_DOWNLOAD_REDIRECTS} redirects while downloading"
                ));
            }
            if is_github_download_url(attempt.url().as_str()) {
                attempt.follow()
            } else {
                let url = attempt.url().clone();
                attempt.error(format!("redirect left the GitHub download hosts for {url}"))
            }
        }));
    builder.build().context("cannot build HTTP client")
}

fn base_client_builder() -> Result<reqwest::blocking::ClientBuilder> {
    let mut builder = reqwest::blocking::Client::builder().user_agent(concat!(
        "riptide/",
        env!("CARGO_PKG_VERSION"),
        " (self-update)"
    ));

    // Determine whether GitHub is exempt from proxying via NO_PROXY.
    // Check both NO_PROXY and no_proxy, skipping empty values so an empty
    // NO_PROXY does not shadow a populated no_proxy.
    let raw_no_proxy = ["NO_PROXY", "no_proxy"]
        .iter()
        .find_map(|v| match std::env::var(v) {
            Ok(s) if !s.trim().is_empty() => Some(s),
            _ => None,
        });
    let no_proxy_for_github = raw_no_proxy.is_some_and(|list| {
        // Hosts we contact: api.github.com for the release JSON, and
        // objects.githubusercontent.com where browser_download_url redirects.
        const GITHUB_HOSTS: &[&str] = &[
            "github.com",
            "api.github.com",
            "objects.githubusercontent.com",
            "githubusercontent.com",
        ];
        list.split(',').any(|entry| {
            let e = entry.trim().trim_start_matches('.').to_ascii_lowercase();
            if e.is_empty() {
                return false;
            }
            if e == "*" {
                return true;
            }
            GITHUB_HOSTS
                .iter()
                .any(|target| *target == e || target.ends_with(&format!(".{e}")))
        })
    });

    if !no_proxy_for_github {
        // Strictly ordered: specific before generic, uppercase before lowercase.
        for var in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
            let Ok(proxy) = std::env::var(var) else {
                continue;
            };
            if proxy.is_empty() {
                continue;
            }
            match reqwest::Proxy::all(&proxy) {
                Ok(p) => {
                    builder = builder.proxy(p);
                    tracing::debug!("self-update proxy configured from {var}");
                    break;
                }
                Err(e) => {
                    tracing::warn!("invalid proxy URL in {var}: {e}; trying next");
                }
            }
        }
    } else {
        tracing::debug!("NO_PROXY matches github.com; self-update bypasses proxy");
    }

    Ok(builder)
}

/// Download `url` to `dest` as a bounded stream. Enforces both a declared
/// `Content-Length` cap and a hard byte cap so a malicious or corrupt server
/// cannot exhaust memory or disk. Returns an error mentioning the HTTP status
/// on non-2xx responses so failures are diagnosable.
fn download_to_file(client: &reqwest::blocking::Client, url: &str, dest: &Path) -> Result<()> {
    const MAX_BYTES: u64 = 128 * 1024 * 1024; // release tarballs are ~8 MiB
    let resp = client
        .get(url)
        .send()
        .with_context(|| format!("cannot download {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("HTTP {status} downloading {url}");
    }
    // Reject upfront when the server declares an oversized body; otherwise cap.
    if let Some(len) = resp.content_length()
        && len > MAX_BYTES
    {
        bail!("download too large ({len} bytes) for {url}");
    }
    let mut file =
        std::fs::File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    let mut limited = resp.take(MAX_BYTES + 1); // +1 lets us detect overflow
    let written = std::io::copy(&mut limited, &mut file)
        .with_context(|| format!("cannot write {}", dest.display()))?;
    if written > MAX_BYTES {
        let _ = std::fs::remove_file(dest);
        bail!("download exceeded {MAX_BYTES} bytes for {url}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    const SAMPLE_RELEASE: &str = r#"{
        "tag_name": "v0.14.0",
        "assets": [
            {"name": "riptide-v0.14.0-x86_64-linux-gnu.tar.gz", "browser_download_url": "https://github.com/fezzik-the-giant/riptide/releases/download/v0.14.0/riptide-v0.14.0-x86_64-linux-gnu.tar.gz"},
            {"name": "riptide-v0.14.0-x86_64-linux-gnu.tar.gz.sha256", "browser_download_url": "https://example.com/redundant"},
            {"name": "riptide-v0.14.0-x86_64-apple-darwin.tar.gz", "browser_download_url": "https://github.com/fezzik-the-giant/riptide/releases/download/v0.14.0/riptide-v0.14.0-x86_64-apple-darwin.tar.gz"},
            {"name": "riptide-v0.14.0-aarch64-apple-darwin.tar.gz", "browser_download_url": "https://github.com/fezzik-the-giant/riptide/releases/download/v0.14.0/riptide-v0.14.0-aarch64-apple-darwin.tar.gz"},
            {"name": "SHA256SUMS", "browser_download_url": "https://github.com/fezzik-the-giant/riptide/releases/download/v0.14.0/SHA256SUMS"}
        ]
    }"#;

    // ── parse_tag ─────────────────────────────────────────────────────────

    #[test]
    fn parse_tag_strips_v_prefix() {
        assert_eq!(parse_tag("v0.14.0"), Some((0, 14, 0)));
    }

    #[test]
    fn parse_tag_accepts_bare_version() {
        assert_eq!(parse_tag("0.14.0"), Some((0, 14, 0)));
    }

    #[test]
    fn parse_tag_rejects_garbage() {
        assert_eq!(parse_tag("latest"), None);
        assert_eq!(parse_tag(""), None);
        assert_eq!(parse_tag("v1.x"), None);
    }

    #[test]
    fn parse_tag_accepts_two_part_and_prerelease() {
        assert_eq!(parse_tag("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_tag("v0.14.0-beta"), Some((0, 14, 0)));
        assert_eq!(parse_tag("v0.14.0+build"), Some((0, 14, 0)));
        assert_eq!(parse_tag("0.14"), Some((0, 14, 0)));
    }

    // ── version_is_newer ──────────────────────────────────────────────────

    #[test]
    fn version_is_newer_detects_upgrade() {
        assert!(version_is_newer("v0.14.0", "0.13.0"));
        assert!(version_is_newer("v1.0.0", "0.13.0"));
        assert!(version_is_newer("v0.13.1", "0.13.0"));
    }

    #[test]
    fn version_is_newer_rejects_equal_or_older() {
        assert!(!version_is_newer("v0.13.0", "0.13.0"));
        assert!(!version_is_newer("v0.12.9", "0.13.0"));
    }

    #[test]
    fn version_is_newer_rejects_unparseable() {
        assert!(!version_is_newer("latest", "0.13.0"));
    }

    // ── is_our_binary_asset ───────────────────────────────────────────────

    #[test]
    fn asset_filter_rejects_checksum_files() {
        assert!(!is_our_binary_asset(
            "riptide-v0.14.0-x86_64-linux-gnu.tar.gz.sha256"
        ));
    }

    #[test]
    fn asset_filter_accepts_exact_platform_match() {
        assert!(is_our_binary_asset(&format!(
            "riptide-v0.14.0-{}.tar.gz",
            target_binary_triple()
        )));
    }

    #[test]
    fn asset_filter_rejects_other_platforms() {
        let other = if cfg!(target_os = "linux") {
            "riptide-v0.14.0-aarch64-apple-darwin.tar.gz"
        } else {
            "riptide-v0.14.0-x86_64-linux-gnu.tar.gz"
        };
        assert!(!is_our_binary_asset(other));
    }

    #[test]
    fn asset_filter_rejects_untarred_binary() {
        assert!(!is_our_binary_asset("riptide"));
    }

    // ── target_binary_triple ──────────────────────────────────────────────

    #[test]
    fn target_triple_is_supported() {
        let triple = target_binary_triple();
        assert!(
            [
                "x86_64-linux-gnu",
                "aarch64-linux-gnu",
                "x86_64-apple-darwin",
                "aarch64-apple-darwin"
            ]
            .contains(&triple),
            "unexpected triple: {triple}"
        );
    }

    // ── release_info_from_json ────────────────────────────────────────────

    #[test]
    fn release_info_parses_tag_and_platform_assets() {
        let info = release_info_from_json(SAMPLE_RELEASE).expect("sample release parses");
        assert_eq!(info.tag, "v0.14.0");
        assert!(info.tarball_url.contains(".tar.gz"));
        assert!(!info.tarball_url.contains("sha256"));
        assert!(!info.tarball_url.ends_with(".tar.gz.sha256"));
        assert_eq!(
            info.checksums_url.as_deref(),
            Some(
                "https://github.com/fezzik-the-giant/riptide/releases/download/v0.14.0/SHA256SUMS"
            )
        );
    }

    #[test]
    fn release_info_errors_without_matching_asset() {
        let json = r#"{"tag_name": "v9.9.9", "assets": [
            {"name": "SHA256SUMS", "browser_download_url": "https://example.com/SHA256SUMS"}
        ]}"#;
        assert!(release_info_from_json(json).is_err());
    }

    // ── parse_sha256sums ──────────────────────────────────────────────────

    #[test]
    fn parse_sha256sums_finds_entry() {
        let content = "e3b0c442  riptide-v0.14.0-x86_64-linux-gnu.tar.gz\nabcd  SHA256SUMS\n";
        assert_eq!(
            parse_sha256sums(content, "riptide-v0.14.0-x86_64-linux-gnu.tar.gz"),
            Some("e3b0c442".to_string())
        );
        assert_eq!(parse_sha256sums(content, "nonexistent.tar.gz"), None);
    }

    #[test]
    fn parse_sha256sums_matches_real_release_format() {
        // Exact format of a real v0.13.0 SHA256SUMS: "<hash><2 spaces><name>".
        let content = "3a64f7cbe6f354eb90c47cbb812de456acc4c10bcda06d70539e7e229af43292  riptide-v0.13.0-aarch64-apple-darwin.tar.gz\n\
                       c994546035d91470b87287465b809bcb545e8b81dad2c23a8ed3ff1922cad762  riptide-v0.13.0-x86_64-apple-darwin.tar.gz\n\
                       bf619082322035618822713061fd37eb69f801c81c383306d8c0cb846222e28e  riptide-v0.13.0-x86_64-linux-gnu.tar.gz\n";
        assert_eq!(
            parse_sha256sums(content, "riptide-v0.13.0-x86_64-linux-gnu.tar.gz"),
            Some("bf619082322035618822713061fd37eb69f801c81c383306d8c0cb846222e28e".to_string())
        );
    }

    // ── extract_binary_from_tarball ───────────────────────────────────────

    fn make_tarball_with_binary(
        dir: &std::path::Path,
        binary_name: &str,
        content: &[u8],
    ) -> Vec<u8> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        let bin_path = dir.join(binary_name);
        fs::write(&bin_path, content).unwrap();

        let mut archive_data = Vec::new();
        {
            let enc = GzEncoder::new(&mut archive_data, Compression::fast());
            let mut builder = tar::Builder::new(enc);
            builder
                .append_path_with_name(&bin_path, binary_name)
                .unwrap();
            let enc = builder.into_inner().unwrap();
            enc.finish().unwrap();
        }
        fs::remove_file(&bin_path).unwrap();
        archive_data
    }

    #[test]
    fn extract_binary_extracts_riptide_and_ignores_checksums() {
        let dir = std::env::temp_dir().join(format!("riptide-test-extract-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let tarball = make_tarball_with_binary(&dir, "riptide", b"\x7fELF-fake-binary");
        let extracted = extract_binary_from_tarball(&tarball, &dir).unwrap();
        assert_eq!(extracted, dir.join("riptide"));
        assert_eq!(fs::read(&extracted).unwrap(), b"\x7fELF-fake-binary");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extract_binary_errors_without_binary() {
        let dir =
            std::env::temp_dir().join(format!("riptide-test-extract-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let tarball = make_tarball_with_binary(&dir, "checksums.txt", b"nothing here");
        assert!(extract_binary_from_tarball(&tarball, &dir).is_err());

        fs::remove_dir_all(&dir).ok();
    }

    // ── swap_binary ───────────────────────────────────────────────────────

    #[test]
    fn swap_binary_replaces_target_atomically() {
        let dir = std::env::temp_dir().join(format!("riptide-test-swap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let target = dir.join("riptide");
        fs::write(&target, b"old-binary").unwrap();

        let staged = dir.join(format!(".riptide.new-{}", std::process::id()));
        {
            let mut f = fs::File::create(&staged).unwrap();
            f.write_all(b"new-binary").unwrap();
        }

        swap_binary(&target, &staged).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new-binary");
        assert!(!staged.exists());

        // sanity: target must be executable for the install to be usable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode();
            assert_ne!(mode & 0o111, 0, "installed binary must be executable");
        }

        fs::remove_dir_all(&dir).ok();
    }

    // ── install_method ────────────────────────────────────────────────────

    #[test]
    fn install_method_detects_cargo_binary() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let exe = std::env::current_exe().unwrap();
        // Tests run from `target/debug`, which sits under a cargo-managed
        // directory tree in this repo, so it must match the Cargo rule.
        if is_under_cargo_bin(&exe) {
            assert_eq!(install_method_from_path(&exe), InstallMethod::Cargo);
        } else {
            // Built outside ~/.cargo/bin (e.g. flakes copy the target dir):
            // still must not be misclassified as pacman-managed.
            assert_ne!(install_method_from_path(&exe), InstallMethod::Pacman);
        }
    }

    #[test]
    fn install_method_detects_known_paths() {
        let _guard = ENV_MUTEX.lock().unwrap();
        assert_eq!(
            install_method_from_path(Path::new("/home/someone/.cargo/bin/riptide")),
            InstallMethod::Cargo
        );
        assert_eq!(
            install_method_from_path(Path::new("/nix/store/abc123-riptide-0.13.0/bin/riptide")),
            InstallMethod::Nix
        );
    }

    #[test]
    fn install_method_for_nonexistent_path_is_not_pacman() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // A path that does not exist can never be owned by pacman; on
        // non-Linux it must fall through to Script directly.
        let method = install_method_from_path(Path::new("/tmp/riptide-no-existe-xyz"));
        assert_eq!(method, InstallMethod::Script);
    }

    #[test]
    fn install_method_dev_target_path_is_script_when_writable() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // The dev binary in target/debug is a plausible install target for
        // manual testing: it must not be treated as package-managed.
        let exe = std::env::current_exe().unwrap();
        if !is_under_cargo_bin(&exe) {
            assert_eq!(install_method_from_path(&exe), InstallMethod::Script);
        }
    }

    // ── download_to_file ──────────────────────────────────────────────────

    #[test]
    fn download_to_file_maps_http_errors() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("riptide-test-dl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // A tld that is guaranteed (RFC 2606) to never resolve.
        let err = download_to_file(
            &http_client().unwrap(),
            "https://riptide.invalid/nope.tar.gz",
            &dir.join("nope.tar.gz"),
        )
        .unwrap_err();
        assert!(
            !err.to_string().contains("HTTP 2"),
            "unexpected success message: {err}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn release_info_preserves_matched_asset_name_for_checksum_lookup() {
        // Tag without `v` but asset still uses `v` prefix (workflow naming).
        // The SHA256SUMS lookup must use the asset's real name, which carries
        // the `v`, not a name re-derived from the bare tag.
        let json = format!(
            r#"{{"tag_name": "0.14.0", "assets": [
                {{"name": "riptide-v0.14.0-{triple}.tar.gz", "browser_download_url": "https://github.com/fezzik-the-giant/riptide/releases/download/0.14.0/riptide-v0.14.0-{triple}.tar.gz"}},
                {{"name": "SHA256SUMS", "browser_download_url": "https://github.com/fezzik-the-giant/riptide/releases/download/0.14.0/SHA256SUMS"}}
            ]}}"#,
            triple = target_binary_triple()
        );
        let info = release_info_from_json(&json).expect("parses");
        // The stored asset name must be the matched file name, not a re-derived one.
        assert_eq!(
            info.asset_name,
            format!("riptide-v0.14.0-{}.tar.gz", target_binary_triple())
        );
        // And it must be the key that finds the entry in SHA256SUMS.
        let sums = format!("abc123  {}\n", info.asset_name);
        assert_eq!(
            parse_sha256sums(&sums, &info.asset_name),
            Some("abc123".to_string())
        );
    }

    // ── is_github_download_url ────────────────────────────────────────────

    #[test]
    fn github_url_accepts_github_hosts() {
        assert!(is_github_download_url(
            "https://github.com/owner/repo/releases/download/v1/riptide.tar.gz"
        ));
        assert!(is_github_download_url(
            "https://objects.githubusercontent.com/github-production-release-asset/x"
        ));
        assert!(is_github_download_url(
            "https://release-assets.githubusercontent.com/x"
        ));
    }

    #[test]
    fn github_url_rejects_http_and_non_github() {
        assert!(!is_github_download_url(
            "http://github.com/owner/repo/release.tar.gz"
        ));
        assert!(!is_github_download_url(
            "https://example.com/riptide.tar.gz"
        ));
        assert!(!is_github_download_url("https://github.com.evil.example/x"));
        assert!(!is_github_download_url("ftp://github.com/x"));
    }

    #[test]
    fn release_info_rejects_non_github_asset_url() {
        let json = r#"{"tag_name": "v9.9.9", "assets": [
            {"name": "riptide-v9.9.9-x86_64-linux-gnu.tar.gz", "browser_download_url": "https://attacker.example/riptide-v9.9.9-x86_64-linux-gnu.tar.gz"},
            {"name": "SHA256SUMS", "browser_download_url": "https://attacker.example/SHA256SUMS"}
        ]}"#;
        assert!(release_info_from_json(json).is_err());
    }

    #[test]
    fn release_info_filters_non_github_checksum_url() {
        // A tarball on GitHub but a checksum pointer off-domain: the URL is
        // dropped, which fails the install closed (no unverified tarball).
        let json = format!(
            r#"{{"tag_name": "v9.9.9", "assets": [
                {{"name": "riptide-v9.9.9-{triple}.tar.gz", "browser_download_url": "https://github.com/o/r/releases/download/v9.9.9/riptide-v9.9.9-{triple}.tar.gz"}},
                {{"name": "SHA256SUMS", "browser_download_url": "https://attacker.example/SHA256SUMS"}}
            ]}}"#,
            triple = target_binary_triple()
        );
        let info = release_info_from_json(&json).expect("parses (tarball is github)");
        assert_eq!(info.checksums_url, None);
    }

    // Serializes tests that mutate process-global environment variables
    // (CARGO_HOME), which would otherwise race concurrent `std::env::var`
    // readers in other tests — glibc `setenv` may realloc the environ block
    // while another thread's `getenv` walks it.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn is_under_cargo_bin_respects_cargo_home() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let cargo_home =
            std::env::temp_dir().join(format!("riptide-cargo-home-{}", std::process::id()));
        let bin = cargo_home.join("bin");
        let exe = bin.join("riptide");
        // Safety: CARGO_HOME is process-global; restore after.
        let prev = std::env::var("CARGO_HOME").ok();
        unsafe { std::env::set_var("CARGO_HOME", &cargo_home) };
        let result = is_under_cargo_bin(&exe);
        match prev {
            Some(v) => unsafe { std::env::set_var("CARGO_HOME", v) },
            None => unsafe { std::env::remove_var("CARGO_HOME") },
        }
        assert!(
            result,
            "CARGO_HOME={} should make {:?} a cargo bin",
            cargo_home.display(),
            exe
        );
        // Also check fallback still works when CARGO_HOME is unset.
        let fallback = Path::new("/home/someone/.cargo/bin/riptide");
        let prev2 = std::env::var("CARGO_HOME").ok();
        unsafe { std::env::remove_var("CARGO_HOME") };
        let fallback_result = is_under_cargo_bin(fallback);
        match prev2 {
            Some(v) => unsafe { std::env::set_var("CARGO_HOME", v) },
            None => {}
        }
        assert!(
            fallback_result,
            "fallback .cargo/bin should still be cargo bin"
        );
    }
}
