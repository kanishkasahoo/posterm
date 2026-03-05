use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};
use flate2::read::GzDecoder;
use reqwest::Url;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;

use crate::persistence::config::config_dir;

const GITHUB_API_URL: &str = "https://api.github.com/repos/kanishkasahoo/posterm/releases/latest";
const GITHUB_DOWNLOAD_PREFIX: &str = "https://github.com/kanishkasahoo/posterm/releases/download";
const UPDATES_DIR_NAME: &str = "updates";
const PENDING_METADATA_FILE: &str = "pending-update.json";

/// Maximum size accepted for a downloaded release archive (100 MiB).
const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;

/// Maximum size accepted for an extracted binary (50 MiB).
const MAX_BINARY_BYTES: u64 = 50 * 1024 * 1024;

/// Maximum size accepted for a downloaded checksum file (256 bytes is ample
/// for a single `<sha256hex>  <filename>` line; reject anything larger to
/// prevent a malicious server from causing unbounded memory growth).
const MAX_CHECKSUM_BYTES: u64 = 256;

/// Maximum size accepted for a downloaded Ed25519 signature file.
/// A raw Ed25519 signature is always exactly 64 bytes.
pub const MAX_SIG_BYTES: u64 = 64;

/// Maximum length allowed for a release tag string (LOW-1 fix).
const MAX_RELEASE_TAG_LEN: usize = 64;

// ── Ed25519 signature verification (HIGH-1) ──────────────────────────────────
//
// The CI/CD pipeline signs the extracted binary with the corresponding private
// key, producing a `<asset>.sig` file uploaded as a release asset.  The
// signature covers exactly the bytes that `extract_expected_binary_from_tar`
// returns, so the chain of trust is:
//   SHA-256  → protects archive integrity
//   Ed25519  → protects binary authenticity
//
// To rotate the signing key: generate a new keypair, set POSTERM_UPDATE_SIGNING_KEY
// in GitHub repo secrets, derive the new public key bytes, and update
// POSTERM_UPDATE_PUBKEY below.
//
// Production Ed25519 public key — do NOT replace unless you rotate the signing key.
const POSTERM_UPDATE_PUBKEY: [u8; 32] = [
    255, 50, 18, 29, 81, 153, 118, 126, 34, 78, 16, 12, 173, 43, 229, 238, 98, 38, 104, 191, 236,
    206, 113, 192, 23, 93, 129, 132, 82, 49, 48, 125,
];

/// Returns `true` when the `POSTERM_SKIP_UPDATE_SIGNATURE_CHECK` environment
/// variable is set to `"1"`.  This bypass exists **only** for development and
/// testing; the `#[cfg(any(debug_assertions, test))]` gate ensures it is
/// compiled out of release builds entirely so it cannot be silently abused in
/// production.  It still prints a loud warning so accidental use is obvious.
#[cfg(any(debug_assertions, test))]
fn signature_check_bypassed() -> bool {
    std::env::var("POSTERM_SKIP_UPDATE_SIGNATURE_CHECK").as_deref() == Ok("1")
}

/// In release builds the bypass function always returns `false` — the env var
/// is not consulted, and the symbol is fully dead-code-eliminated.
#[cfg(not(any(debug_assertions, test)))]
fn signature_check_bypassed() -> bool {
    false
}

// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestRelease {
    pub tag_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    NoPendingUpdate,
    Applied {
        version: String,
        target_path: PathBuf,
    },
    PermissionDenied {
        version: String,
        staged_path: PathBuf,
        target_path: PathBuf,
    },
    Failed {
        version: Option<String>,
        reason: String,
    },
}

#[derive(Debug)]
pub enum UpdateError {
    UnsupportedPlatform(String),
    InvalidReleaseTag(String),
    Http(String),
    Checksum(String),
    Signature(String),
    Archive(String),
    Io(String),
    Json(String),
    Security(String),
    VersionParse(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform(msg)
            | Self::InvalidReleaseTag(msg)
            | Self::Http(msg)
            | Self::Checksum(msg)
            | Self::Signature(msg)
            | Self::Archive(msg)
            | Self::Io(msg)
            | Self::Json(msg)
            | Self::Security(msg)
            | Self::VersionParse(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

#[derive(Debug, Deserialize)]
struct LatestReleaseApiResponse {
    tag_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PendingUpdateMetadata {
    version: String,
    staged_binary_path: String,
}

pub fn release_asset_for_current_platform() -> Result<&'static str, UpdateError> {
    asset_name_for_current_platform()
}

pub async fn check_latest_version_via_github_api(
    client: &reqwest::Client,
) -> Result<LatestRelease, UpdateError> {
    let api_url = Url::parse(GITHUB_API_URL)
        .map_err(|error| UpdateError::Http(format!("Invalid GitHub API URL: {error}")))?;

    if api_url.host_str() != Some("api.github.com")
        || api_url.path() != "/repos/kanishkasahoo/posterm/releases/latest"
    {
        return Err(UpdateError::Security(String::from(
            "Refusing to call unexpected update API host/path",
        )));
    }

    let response =
        client.get(api_url).send().await.map_err(|error| {
            UpdateError::Http(format!("Failed to query latest release: {error}"))
        })?;

    if !response.status().is_success() {
        return Err(UpdateError::Http(format!(
            "GitHub API returned {} while checking latest release",
            response.status()
        )));
    }

    let body = response.text().await.map_err(|error| {
        UpdateError::Http(format!("Failed reading GitHub API response: {error}"))
    })?;

    let parsed: LatestReleaseApiResponse = serde_json::from_str(&body).map_err(|error| {
        UpdateError::Json(format!("Failed to parse GitHub API response: {error}"))
    })?;

    validate_release_tag(&parsed.tag_name)?;

    Ok(LatestRelease {
        tag_name: parsed.tag_name,
    })
}

/// Compare `current_version` and `latest_version` as semver and return
/// `Err(UpdateUpToDate)` if `latest <= current` (MEDIUM-2: downgrade protection).
///
/// Both strings may carry an optional leading `v` prefix.
pub fn check_version_is_upgrade(
    current_version: &str,
    latest_tag: &str,
) -> Result<(), UpdateError> {
    fn strip_v(s: &str) -> &str {
        s.trim_start_matches('v')
    }

    let current = Version::parse(strip_v(current_version)).map_err(|err| {
        UpdateError::VersionParse(format!(
            "Current version '{}' is not valid semver: {err}",
            current_version
        ))
    })?;

    let latest = Version::parse(strip_v(latest_tag)).map_err(|err| {
        UpdateError::VersionParse(format!(
            "Latest tag '{}' is not valid semver: {err}",
            latest_tag
        ))
    })?;

    if latest <= current {
        // Callers map this specific variant to Action::UpdateUpToDate.
        return Err(UpdateError::VersionParse(format!(
            "ALREADY_UP_TO_DATE:{latest_tag}"
        )));
    }

    Ok(())
}

pub async fn download_release_asset_and_checksum(
    client: &reqwest::Client,
    tag_name: &str,
    asset_name: &str,
) -> Result<(Vec<u8>, String), UpdateError> {
    validate_release_tag(tag_name)?;

    let asset_url = strict_download_url(tag_name, asset_name)?;
    let checksum_url = strict_download_url(tag_name, &format!("{asset_name}.sha256"))?;

    // ── Archive download (HIGH-3: check Content-Length before buffering) ──────
    let archive_response = client.get(asset_url).send().await.map_err(|error| {
        UpdateError::Http(format!(
            "Failed to download release asset for {tag_name}: {error}"
        ))
    })?;
    if !archive_response.status().is_success() {
        return Err(UpdateError::Http(format!(
            "Asset download failed with status {}",
            archive_response.status()
        )));
    }

    if let Some(content_length) = archive_response.content_length()
        && content_length > MAX_ARCHIVE_BYTES
    {
        return Err(UpdateError::Http(format!(
            "Release archive Content-Length ({content_length} bytes) exceeds the \
             {MAX_ARCHIVE_BYTES}-byte safety limit; aborting download"
        )));
    }

    let archive_bytes = archive_response.bytes().await.map_err(|error| {
        UpdateError::Http(format!("Failed to read release asset bytes: {error}"))
    })?;

    // Double-check actual size after streaming (server may omit Content-Length).
    if archive_bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(UpdateError::Http(format!(
            "Release archive ({} bytes) exceeds the {MAX_ARCHIVE_BYTES}-byte safety limit",
            archive_bytes.len()
        )));
    }

    // ── Checksum download ─────────────────────────────────────────────────────
    let checksum_response = client.get(checksum_url).send().await.map_err(|error| {
        UpdateError::Http(format!(
            "Failed to download checksum file for {tag_name}: {error}"
        ))
    })?;
    if !checksum_response.status().is_success() {
        return Err(UpdateError::Http(format!(
            "Checksum download failed with status {}",
            checksum_response.status()
        )));
    }

    // MEDIUM-NW-1: Reject oversized checksum responses before buffering them.
    // Check Content-Length header if present, then re-check the actual byte length.
    if let Some(content_length) = checksum_response.content_length()
        && content_length > MAX_CHECKSUM_BYTES
    {
        return Err(UpdateError::Http(format!(
            "Checksum Content-Length ({content_length} bytes) exceeds the \
             {MAX_CHECKSUM_BYTES}-byte safety limit; aborting download"
        )));
    }

    let checksum_raw = checksum_response
        .bytes()
        .await
        .map_err(|error| UpdateError::Http(format!("Failed to read checksum content: {error}")))?;

    if checksum_raw.len() as u64 > MAX_CHECKSUM_BYTES {
        return Err(UpdateError::Http(format!(
            "Checksum file ({} bytes) exceeds the {MAX_CHECKSUM_BYTES}-byte safety limit",
            checksum_raw.len()
        )));
    }

    let checksum_text = String::from_utf8(checksum_raw.to_vec()).map_err(|error| {
        UpdateError::Checksum(format!("Checksum file is not valid UTF-8: {error}"))
    })?;

    Ok((archive_bytes.to_vec(), checksum_text))
}

pub fn verify_sha256(archive_bytes: &[u8], checksum_text: &str) -> Result<(), UpdateError> {
    let expected_hex = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| UpdateError::Checksum(String::from("Checksum file was empty")))?;

    if expected_hex.len() != 64 || !expected_hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(UpdateError::Checksum(String::from(
            "Checksum file did not contain a valid SHA-256 hash",
        )));
    }

    let mut hasher = Sha256::new();
    hasher.update(archive_bytes);
    let digest = hasher.finalize();
    let actual_hex = format!("{digest:x}");

    if !actual_hex.eq_ignore_ascii_case(expected_hex) {
        return Err(UpdateError::Checksum(String::from(
            "Downloaded archive checksum did not match",
        )));
    }

    Ok(())
}

/// Verify the Ed25519 signature of `binary_bytes` against the embedded public key.
///
/// HIGH-1: SHA-256 protects archive integrity but not authenticity — an attacker
/// who can replace both the archive and its accompanying `.sha256` file (e.g. via
/// a compromised GitHub release or a MITM against the CDN) could deliver a
/// malicious binary.  Ed25519 closes that gap: only the holder of the private
/// signing key can produce a valid signature.
///
/// In production:
///   - The CI pipeline signs the extracted binary bytes with the private key.
///   - The resulting 64-byte signature is uploaded as `<asset_name>.sig`.
///   - This function verifies that signature against `POSTERM_UPDATE_PUBKEY`.
///   - Do not replace `POSTERM_UPDATE_PUBKEY` unless you rotate the signing key.
pub fn verify_ed25519_signature(
    binary_bytes: &[u8],
    signature_bytes: &[u8],
) -> Result<(), UpdateError> {
    // Bypass for development/testing only.
    if signature_check_bypassed() {
        eprintln!(
            "\n\
             ╔══════════════════════════════════════════════════════════════════╗\n\
             ║  WARNING: Ed25519 signature check is DISABLED via               ║\n\
             ║  POSTERM_SKIP_UPDATE_SIGNATURE_CHECK=1                          ║\n\
             ║  DO NOT use this in production.                                 ║\n\
             ╚══════════════════════════════════════════════════════════════════╝\n"
        );
        return Ok(());
    }

    let verifying_key = VerifyingKey::from_bytes(&POSTERM_UPDATE_PUBKEY).map_err(|err| {
        UpdateError::Signature(format!("Invalid embedded Ed25519 public key: {err}"))
    })?;

    if signature_bytes.len() != 64 {
        return Err(UpdateError::Signature(format!(
            "Ed25519 signature must be exactly 64 bytes, got {}",
            signature_bytes.len()
        )));
    }

    let sig_array: [u8; 64] = signature_bytes.try_into().expect("length checked above");
    let signature = Signature::from_bytes(&sig_array);

    use ed25519_dalek::Verifier;
    verifying_key
        .verify(binary_bytes, &signature)
        .map_err(|err| {
            UpdateError::Signature(format!(
                "Ed25519 signature verification failed — binary may have been tampered with: {err}"
            ))
        })
}

pub fn extract_expected_binary_from_tar(
    archive_bytes: &[u8],
    expected_binary_name: &str,
) -> Result<Vec<u8>, UpdateError> {
    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(decoder);
    let mut selected_contents: Option<Vec<u8>> = None;

    let entries = archive.entries().map_err(|error| {
        UpdateError::Archive(format!("Failed to inspect archive entries: {error}"))
    })?;

    for entry_result in entries {
        let mut entry = entry_result
            .map_err(|error| UpdateError::Archive(format!("Invalid tar entry: {error}")))?;
        let path = entry
            .path()
            .map_err(|error| UpdateError::Archive(format!("Invalid tar path: {error}")))?;

        validate_safe_relative_path(&path)?;

        if !entry.header().entry_type().is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if file_name != expected_binary_name {
            continue;
        }

        if selected_contents.is_some() {
            return Err(UpdateError::Archive(String::from(
                "Archive contains multiple matching posterm binaries",
            )));
        }

        // HIGH-3: Reject oversized entries before reading into memory.
        let entry_size = entry.header().size().unwrap_or(u64::MAX);
        if entry_size > MAX_BINARY_BYTES {
            return Err(UpdateError::Archive(format!(
                "Binary entry size ({entry_size} bytes) exceeds the \
                 {MAX_BINARY_BYTES}-byte safety limit"
            )));
        }

        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).map_err(|error| {
            UpdateError::Archive(format!("Failed to read binary data: {error}"))
        })?;

        // Final size guard after extraction (tar header may be untrustworthy).
        if contents.len() as u64 > MAX_BINARY_BYTES {
            return Err(UpdateError::Archive(format!(
                "Extracted binary ({} bytes) exceeds the {MAX_BINARY_BYTES}-byte safety limit",
                contents.len()
            )));
        }

        selected_contents = Some(contents);
    }

    selected_contents.ok_or_else(|| {
        UpdateError::Archive(String::from(
            "Archive did not contain expected posterm binary",
        ))
    })
}

pub fn stage_file_and_metadata(
    binary_bytes: &[u8],
    version_tag: &str,
) -> Result<PathBuf, UpdateError> {
    validate_release_tag(version_tag)?;

    let updates_dir = updates_dir_path();
    create_updates_dir_secure(&updates_dir)?;

    // MEDIUM-4: Use a non-predictable temporary name in the executable directory
    // via `tempfile::Builder`, then atomically rename into the final staged location.
    let normalized_tag = version_tag.trim_start_matches('v');
    let staged_binary_path = updates_dir.join(format!("posterm-{normalized_tag}"));

    // Write to a temp file first, then rename atomically.
    {
        let mut tmp = tempfile::Builder::new()
            .suffix(".tmp")
            .tempfile_in(&updates_dir)
            .map_err(|error| {
                UpdateError::Io(format!(
                    "Failed to create temp file for staged binary: {error}"
                ))
            })?;

        use std::io::Write;
        tmp.write_all(binary_bytes).map_err(|error| {
            UpdateError::Io(format!(
                "Failed to write staged binary to temp file: {error}"
            ))
        })?;
        tmp.flush().map_err(|error| {
            UpdateError::Io(format!("Failed to flush staged binary temp file: {error}"))
        })?;

        // Persist (keep) the temp file so we can rename it.
        let (_, tmp_path) = tmp.keep().map_err(|error| {
            UpdateError::Io(format!(
                "Failed to persist staged binary temp file: {error}"
            ))
        })?;

        fs::rename(&tmp_path, &staged_binary_path).map_err(|error| {
            // Clean up orphaned temp file on rename failure.
            let _ = fs::remove_file(&tmp_path);
            UpdateError::Io(format!(
                "Failed to rename staged binary into place: {error}"
            ))
        })?;
    }

    ensure_regular_executable_file(&staged_binary_path)?;

    let metadata = PendingUpdateMetadata {
        version: version_tag.to_string(),
        staged_binary_path: staged_binary_path.to_string_lossy().to_string(),
    };
    let metadata_json = serde_json::to_string_pretty(&metadata).map_err(|error| {
        UpdateError::Json(format!("Failed to encode pending metadata: {error}"))
    })?;

    fs::write(pending_metadata_path(), metadata_json.as_bytes())
        .map_err(|error| UpdateError::Io(format!("Failed to write pending metadata: {error}")))?;

    Ok(staged_binary_path)
}

pub fn apply_pending_update_on_exit() -> ApplyOutcome {
    let metadata = match read_pending_metadata() {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return ApplyOutcome::NoPendingUpdate,
        Err(error) => {
            return ApplyOutcome::Failed {
                version: None,
                reason: error.to_string(),
            };
        }
    };

    // HIGH-2: Canonicalize the staged path and assert it lives inside updates_dir.
    let staged_path = match canonicalize_and_assert_child(
        &PathBuf::from(&metadata.staged_binary_path),
        &updates_dir_path(),
    ) {
        Ok(path) => path,
        Err(error) => {
            return ApplyOutcome::Failed {
                version: Some(metadata.version),
                reason: error.to_string(),
            };
        }
    };

    if let Err(error) = ensure_regular_executable_file(&staged_path) {
        return ApplyOutcome::Failed {
            version: Some(metadata.version),
            reason: error.to_string(),
        };
    }

    let target_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return ApplyOutcome::Failed {
                version: Some(metadata.version),
                reason: format!("Failed to locate running executable: {error}"),
            };
        }
    };

    match replace_executable(&staged_path, &target_path) {
        Ok(()) => {
            let _ = fs::remove_file(pending_metadata_path());
            let _ = fs::remove_file(&staged_path);
            ApplyOutcome::Applied {
                version: metadata.version,
                target_path,
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            ApplyOutcome::PermissionDenied {
                version: metadata.version,
                staged_path,
                target_path,
            }
        }
        Err(error) => ApplyOutcome::Failed {
            version: Some(metadata.version),
            reason: format!("Failed to apply staged update: {error}"),
        },
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// LOW-1 + existing character checks.
fn validate_release_tag(tag_name: &str) -> Result<(), UpdateError> {
    if tag_name.is_empty() {
        return Err(UpdateError::InvalidReleaseTag(String::from(
            "Release tag was empty",
        )));
    }

    // LOW-1: Reject excessively long tags before any further processing.
    if tag_name.len() > MAX_RELEASE_TAG_LEN {
        return Err(UpdateError::InvalidReleaseTag(format!(
            "Release tag exceeds maximum length of {MAX_RELEASE_TAG_LEN} characters"
        )));
    }

    let sanitized = tag_name.strip_prefix('v').unwrap_or(tag_name);
    if sanitized.is_empty()
        || !sanitized
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(UpdateError::InvalidReleaseTag(format!(
            "Release tag contains unsupported characters: {tag_name}"
        )));
    }

    Ok(())
}

fn strict_download_url(tag_name: &str, file_name: &str) -> Result<Url, UpdateError> {
    let encoded_tag = tag_name;
    let url = Url::parse(&format!(
        "{GITHUB_DOWNLOAD_PREFIX}/{encoded_tag}/{file_name}"
    ))
    .map_err(|error| UpdateError::Http(format!("Failed to build download URL: {error}")))?;

    if url.host_str() != Some("github.com")
        || !url
            .path()
            .starts_with("/kanishkasahoo/posterm/releases/download/")
    {
        return Err(UpdateError::Security(String::from(
            "Refusing to use unexpected download host/path",
        )));
    }

    Ok(url)
}

fn asset_name_for_current_platform() -> Result<&'static str, UpdateError> {
    if cfg!(target_os = "macos") {
        return Ok("posterm-macos.tar.gz");
    }

    if cfg!(target_os = "linux") {
        if is_ubuntu_linux() {
            return Ok("posterm-linux.tar.gz");
        }
        return Err(UpdateError::UnsupportedPlatform(String::from(
            "Self-update is supported on Ubuntu Linux only",
        )));
    }

    Err(UpdateError::UnsupportedPlatform(String::from(
        "Self-update is supported only on macOS and Ubuntu Linux",
    )))
}

fn is_ubuntu_linux() -> bool {
    let content = match fs::read_to_string("/etc/os-release") {
        Ok(content) => content,
        Err(_) => return false,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("ID=") {
            let value = rest.trim_matches('"').trim_matches('\'');
            return value.eq_ignore_ascii_case("ubuntu");
        }
    }

    false
}

fn validate_safe_relative_path(path: &Path) -> Result<(), UpdateError> {
    if path.is_absolute() {
        return Err(UpdateError::Security(String::from(
            "Archive contained an absolute path",
        )));
    }

    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(UpdateError::Security(String::from(
                "Archive contained an unsafe path traversal entry",
            )));
        }
    }

    Ok(())
}

/// HIGH-2: Use `symlink_metadata` so we never silently follow a symlink.
/// Reject anything that is not a plain regular file.
fn ensure_regular_executable_file(path: &Path) -> Result<(), UpdateError> {
    // Use symlink_metadata — unlike metadata(), this does NOT follow symlinks.
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        UpdateError::Io(format!(
            "Failed to inspect file {}: {error}",
            path.display()
        ))
    })?;

    // Reject symlinks explicitly — they could be redirected to an attacker-controlled target.
    if metadata.file_type().is_symlink() {
        return Err(UpdateError::Security(format!(
            "Staged update {} is a symlink, which is not permitted",
            path.display()
        )));
    }

    if !metadata.is_file() {
        return Err(UpdateError::Security(format!(
            "Staged update {} is not a regular file",
            path.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 {
            let mut permissions = metadata.permissions();
            permissions.set_mode(mode | 0o755);
            fs::set_permissions(path, permissions).map_err(|error| {
                UpdateError::Io(format!(
                    "Failed to mark staged update executable {}: {error}",
                    path.display()
                ))
            })?;
        }
    }

    Ok(())
}

/// LOW-NW-1: Create the updates directory with restricted permissions (0700 on Unix).
///
/// Fixes applied:
///   - Removed the `exists()` TOCTOU pre-check; `create_dir_all` is idempotent.
///   - `set_permissions(0o700)` is called unconditionally so an existing dir with
///     relaxed permissions is always corrected.
///   - `symlink_metadata` is used to detect and reject a symlink at the expected
///     directory path, preventing a local attacker from redirecting staged binaries
///     to an arbitrary location.
fn create_updates_dir_secure(updates_dir: &Path) -> Result<(), UpdateError> {
    // LOW-NW-1: Reject a symlink masquerading as the updates directory.
    // symlink_metadata does not follow symlinks, so is_symlink() is reliable.
    match fs::symlink_metadata(updates_dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(UpdateError::Security(format!(
                "Updates directory path '{}' is a symlink, which is not permitted",
                updates_dir.display()
            )));
        }
        // Does not exist yet — create_dir_all will handle it below.
        Err(ref error) if error.kind() == std::io::ErrorKind::NotFound => {}
        // Exists as a real directory (or other non-symlink) — proceed.
        Ok(_) => {}
        Err(error) => {
            return Err(UpdateError::Io(format!(
                "Failed to inspect updates directory '{}': {error}",
                updates_dir.display()
            )));
        }
    }

    // Idempotent: creates the directory if absent, no-ops if it already exists.
    fs::create_dir_all(updates_dir)
        .map_err(|error| UpdateError::Io(format!("Failed to create updates directory: {error}")))?;

    // LOW-NW-1: Always enforce 0o700 so an existing directory with relaxed
    // permissions is corrected.  This is a no-op cost on already-correct dirs.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o700);
        fs::set_permissions(updates_dir, permissions).map_err(|error| {
            UpdateError::Io(format!(
                "Failed to set permissions on updates directory: {error}"
            ))
        })?;
    }

    Ok(())
}

/// HIGH-2: Canonicalize `path` (resolving symlinks and `..`) and assert that
/// the result is a direct child of `expected_parent`.  This prevents a tampered
/// metadata file from pointing to an arbitrary path on disk.
fn canonicalize_and_assert_child(
    path: &Path,
    expected_parent: &Path,
) -> Result<PathBuf, UpdateError> {
    // Canonicalize the parent first so we have an absolute reference.
    let canonical_parent = fs::canonicalize(expected_parent).map_err(|error| {
        UpdateError::Security(format!(
            "Failed to canonicalize updates directory {}: {error}",
            expected_parent.display()
        ))
    })?;

    let canonical_path = fs::canonicalize(path).map_err(|error| {
        UpdateError::Security(format!(
            "Failed to canonicalize staged path {}: {error}",
            path.display()
        ))
    })?;

    if !canonical_path.starts_with(&canonical_parent) {
        return Err(UpdateError::Security(format!(
            "Staged binary path '{}' is not inside the expected updates directory '{}' — \
             refusing to apply update",
            canonical_path.display(),
            canonical_parent.display()
        )));
    }

    Ok(canonical_path)
}

/// MEDIUM-4: Use a temp file with an unpredictable name in the executable's
/// parent directory, then perform an atomic rename.  The caller must already
/// hold the staged binary in a known location; this function stages the copy
/// into the target directory safely.
fn replace_executable(staged_path: &Path, target_path: &Path) -> std::io::Result<()> {
    let parent = target_path
        .parent()
        .ok_or_else(|| std::io::Error::other("Executable path does not have a parent directory"))?;

    // Use tempfile for an unpredictable name to prevent TOCTOU races.
    let tmp = tempfile::Builder::new()
        .suffix(".tmp")
        .tempfile_in(parent)?;
    let (_, tmp_path) = tmp.keep()?;

    // Ensure cleanup on any subsequent error.
    let result = (|| {
        fs::copy(staged_path, &tmp_path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&tmp_path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&tmp_path, permissions)?;
        }

        fs::rename(&tmp_path, target_path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    result
}

fn read_pending_metadata() -> Result<Option<PendingUpdateMetadata>, UpdateError> {
    let path = pending_metadata_path();
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path)
        .map_err(|error| UpdateError::Io(format!("Failed to read pending metadata: {error}")))?;
    let metadata: PendingUpdateMetadata = serde_json::from_str(&raw)
        .map_err(|error| UpdateError::Json(format!("Failed to parse pending metadata: {error}")))?;

    Ok(Some(metadata))
}

pub fn updates_dir_path() -> PathBuf {
    config_dir().join(UPDATES_DIR_NAME)
}

fn pending_metadata_path() -> PathBuf {
    updates_dir_path().join(PENDING_METADATA_FILE)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── LOW-1: validate_release_tag length cap ────────────────────────────────

    #[test]
    fn tag_within_max_length_is_accepted() {
        let tag = format!("v{}", "1.2.3");
        assert!(validate_release_tag(&tag).is_ok());
    }

    #[test]
    fn tag_exactly_at_max_length_is_accepted() {
        // 64 chars starting with 'v'
        let tag = format!("v{}", "1".repeat(63));
        assert!(validate_release_tag(&tag).is_ok());
    }

    #[test]
    fn tag_exceeding_max_length_is_rejected() {
        let tag = "v".to_string() + &"1".repeat(64); // 65 chars total
        let err = validate_release_tag(&tag).unwrap_err();
        assert!(
            matches!(err, UpdateError::InvalidReleaseTag(_)),
            "expected InvalidReleaseTag, got: {err}"
        );
    }

    #[test]
    fn empty_tag_is_rejected() {
        assert!(matches!(
            validate_release_tag(""),
            Err(UpdateError::InvalidReleaseTag(_))
        ));
    }

    #[test]
    fn tag_with_special_chars_is_rejected() {
        assert!(matches!(
            validate_release_tag("v1.0.0; rm -rf /"),
            Err(UpdateError::InvalidReleaseTag(_))
        ));
    }

    // ── MEDIUM-2: semver downgrade protection ─────────────────────────────────

    #[test]
    fn same_version_is_treated_as_up_to_date() {
        let result = check_version_is_upgrade("1.0.0", "v1.0.0");
        assert!(result.is_err());
        if let Err(UpdateError::VersionParse(msg)) = result {
            assert!(msg.starts_with("ALREADY_UP_TO_DATE:"));
        } else {
            panic!("expected VersionParse ALREADY_UP_TO_DATE");
        }
    }

    #[test]
    fn older_remote_version_is_treated_as_up_to_date() {
        let result = check_version_is_upgrade("2.0.0", "v1.9.9");
        assert!(result.is_err());
        if let Err(UpdateError::VersionParse(msg)) = result {
            assert!(msg.starts_with("ALREADY_UP_TO_DATE:"));
        } else {
            panic!("expected VersionParse ALREADY_UP_TO_DATE");
        }
    }

    #[test]
    fn newer_remote_version_is_accepted() {
        assert!(check_version_is_upgrade("1.0.0", "v1.0.1").is_ok());
        assert!(check_version_is_upgrade("0.9.9", "v1.0.0").is_ok());
    }

    #[test]
    fn invalid_semver_returns_version_parse_error() {
        assert!(matches!(
            check_version_is_upgrade("not-semver", "v1.0.0"),
            Err(UpdateError::VersionParse(_))
        ));
        assert!(matches!(
            check_version_is_upgrade("1.0.0", "not-semver"),
            Err(UpdateError::VersionParse(_))
        ));
    }

    // ── HIGH-3: download size limits ──────────────────────────────────────────

    #[test]
    fn size_limit_constants_are_sane() {
        // Verify the constants used by the size guards have the expected values.
        // The actual enforcement logic is covered by the production code paths
        // (Content-Length check in download_release_asset_and_checksum and the
        // entry-header check in extract_expected_binary_from_tar).
        assert_eq!(MAX_BINARY_BYTES, 50 * 1024 * 1024);
        assert_eq!(MAX_ARCHIVE_BYTES, 100 * 1024 * 1024);
    }

    #[test]
    fn extract_rejects_tiny_archive_with_no_matching_binary() {
        // A minimal valid gzip of an empty tar should return an Archive error,
        // not panic or hang.
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use tar::Builder;

        let gz_buf = Vec::new();
        let enc = GzEncoder::new(gz_buf, Compression::default());
        let mut ar = Builder::new(enc);
        let data: &[u8] = b"not a posterm binary";
        let mut header = tar::Header::new_gnu();
        header.set_path("other-file").unwrap();
        header.set_size(data.len() as u64);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o755);
        header.set_cksum();
        ar.append(&header, data).unwrap();
        let gz_bytes = ar.into_inner().unwrap().finish().unwrap();

        let result = extract_expected_binary_from_tar(&gz_bytes, "posterm");
        assert!(
            matches!(result, Err(UpdateError::Archive(_))),
            "missing binary should produce Archive error"
        );
    }

    // ── HIGH-1: Ed25519 signature verification ────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn wrong_signature_is_rejected() {
        unsafe {
            std::env::remove_var("POSTERM_SKIP_UPDATE_SIGNATURE_CHECK");
        }
        let result = verify_ed25519_signature(b"test payload", &[0u8; 64]);
        assert!(
            matches!(result, Err(UpdateError::Signature(_))),
            "a zeroed signature must be rejected by the real key"
        );
    }

    #[test]
    #[serial_test::serial]
    fn signature_bypass_emits_warning_and_succeeds() {
        // SAFETY: test binary is single-threaded at this point.
        unsafe {
            std::env::set_var("POSTERM_SKIP_UPDATE_SIGNATURE_CHECK", "1");
        }
        let result = verify_ed25519_signature(b"anything", &[]);
        // SAFETY: same rationale.
        unsafe {
            std::env::remove_var("POSTERM_SKIP_UPDATE_SIGNATURE_CHECK");
        }
        assert!(result.is_ok(), "bypass should succeed regardless of input");
    }
}
