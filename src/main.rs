mod action;
mod app;
mod components;
mod event;
mod highlight;
mod http;
mod persistence;
mod state;
mod tui;
mod updater;
mod util;

use std::time::Duration;

use app::App;
use tui::Tui;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("upgrade") => {
            std::process::exit(run_upgrade().await);
        }
        Some("--help") | Some("-h") | Some("help") => {
            print_usage();
            return Ok(());
        }
        Some("version") | Some("--version") | Some("-V") => {
            println!("posterm {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(unknown) => {
            eprintln!("error: unknown subcommand '{unknown}'");
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
        None => {
            // Default: start the TUI.
            {
                let mut tui = Tui::new()?;
                let initial_size = tui.size()?;
                let mut app = App::new(initial_size);
                app.run(&mut tui).await?;
            }

            // Safety net: apply any update that was staged during a previous run.
            apply_update_on_exit();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("posterm — a terminal HTTP client");
    println!();
    println!("USAGE:");
    println!("  posterm             Start the interactive TUI");
    println!("  posterm upgrade     Check for and apply the latest release");
    println!("  posterm version     Print version information and exit");
    println!("  posterm help        Show this help message");
    println!();
    println!("FLAGS:");
    println!("  -h, --help          Show this help message");
    println!("  -V, --version       Print version information and exit");
}

fn apply_update_on_exit() {
    match updater::apply_pending_update_on_exit() {
        updater::ApplyOutcome::NoPendingUpdate => {}
        updater::ApplyOutcome::Applied {
            version,
            target_path,
        } => {
            println!(
                "posterm update {version} applied successfully to {}",
                target_path.display()
            );
        }
        updater::ApplyOutcome::PermissionDenied {
            version,
            staged_path,
            target_path,
        } => {
            // LOW-2: Do not interpolate attacker-influenced paths into a sudo
            // command suggestion — an adversary who tampers with the metadata
            // JSON could inject shell metacharacters via staged_path.
            // Instead, emit a generic message; the canonical path check in
            // apply_pending_update_on_exit() already ensures staged_path is safe,
            // but we keep the message generic to prevent confusion in CI logs.
            eprintln!(
                "posterm update {version} is staged but could not be applied automatically \
                 due to insufficient permissions."
            );
            eprintln!(
                "Re-run posterm with elevated privileges, or copy the staged binary manually \
                 from: {}",
                staged_path.display()
            );
            eprintln!("Target installation path: {}", target_path.display());
        }
        updater::ApplyOutcome::Failed { version, reason } => {
            if let Some(version) = version {
                eprintln!("posterm update {version} failed to apply: {reason}");
            } else {
                eprintln!("posterm update apply step failed: {reason}");
            }
        }
    }
}

/// Runs the `posterm upgrade` subcommand inline, printing progress to stdout.
/// Returns an exit code: 0 = success / already up-to-date, 1 = error.
async fn run_upgrade() -> i32 {
    let current_version = env!("CARGO_PKG_VERSION");

    // ── Step 1: Build reqwest client ──────────────────────────────────────────
    let client = match reqwest::Client::builder()
        .user_agent(format!("posterm/{current_version}"))
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
    {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: failed to build HTTP client: {err}");
            return 1;
        }
    };

    // ── Step 2: Resolve platform asset name ──────────────────────────────────
    let asset_name = match updater::release_asset_for_current_platform() {
        Ok(name) => name,
        Err(updater::UpdateError::UnsupportedPlatform(reason)) => {
            eprintln!("error: unsupported platform — {reason}");
            return 1;
        }
        Err(err) => {
            eprintln!("error: {err}");
            return 1;
        }
    };

    // ── Step 3: Check latest version ─────────────────────────────────────────
    println!("Checking for updates...");
    let latest = match updater::check_latest_version_via_github_api(&client).await {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: failed to fetch latest release info: {err}");
            return 1;
        }
    };

    // ── Step 4: Compare versions ──────────────────────────────────────────────
    match updater::check_version_is_upgrade(current_version, &latest.tag_name) {
        Ok(()) => {}
        Err(updater::UpdateError::VersionParse(ref msg))
            if msg.starts_with("ALREADY_UP_TO_DATE:") =>
        {
            println!("Already up to date ({})", latest.tag_name);
            return 0;
        }
        Err(err) => {
            // Semver parse failure: fall back to string equality.
            if latest.tag_name.trim_start_matches('v') == current_version.trim_start_matches('v') {
                println!("Already up to date ({})", latest.tag_name);
                return 0;
            }
            // Log parse error but proceed; the binary is authenticated
            // by SHA-256 + Ed25519 regardless.
            eprintln!("[posterm] updater: semver comparison failed ({err}); proceeding");
        }
    }

    // ── Step 5: Download archive + checksum ──────────────────────────────────
    println!("Downloading {}...", latest.tag_name);
    let (archive_bytes, checksum_text) =
        match updater::download_release_asset_and_checksum(&client, &latest.tag_name, asset_name)
            .await
        {
            Ok(pair) => pair,
            Err(err) => {
                eprintln!("error: download failed: {err}");
                return 1;
            }
        };

    // ── Step 6: Verify SHA-256 ────────────────────────────────────────────────
    println!("Verifying...");
    if let Err(err) = updater::verify_sha256(&archive_bytes, &checksum_text) {
        eprintln!("error: checksum verification failed: {err}");
        return 1;
    }

    // ── Step 7: Extract binary from archive ───────────────────────────────────
    let binary_bytes = match updater::extract_expected_binary_from_tar(&archive_bytes, "posterm") {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("error: extraction failed: {err}");
            return 1;
        }
    };

    // ── Step 8: Download Ed25519 signature ────────────────────────────────────
    let sig_asset_name = format!("{asset_name}.sig");
    let sig_url = format!(
        "https://github.com/kanishkasahoo/posterm/releases/download/{}/{sig_asset_name}",
        latest.tag_name
    );

    let sig_bytes: Vec<u8> = match async {
        let response = client.get(&sig_url).send().await.map_err(|err| {
            updater::UpdateError::Http(format!("Failed to download signature: {err}"))
        })?;

        if !response.status().is_success() {
            return Err(updater::UpdateError::Http(format!(
                "Signature download failed with status {}",
                response.status()
            )));
        }

        if let Some(len) = response.content_length() {
            if len > updater::MAX_SIG_BYTES {
                return Err(updater::UpdateError::Http(format!(
                    "Signature Content-Length ({len} bytes) exceeds the {}-byte limit",
                    updater::MAX_SIG_BYTES
                )));
            }
        }

        let raw = response.bytes().await.map_err(|err| {
            updater::UpdateError::Http(format!("Failed to read signature bytes: {err}"))
        })?;

        if raw.len() as u64 > updater::MAX_SIG_BYTES {
            return Err(updater::UpdateError::Http(format!(
                "Signature file ({} bytes) exceeds the {}-byte limit",
                raw.len(),
                updater::MAX_SIG_BYTES
            )));
        }

        Ok(raw.to_vec())
    }
    .await
    {
        Ok(b) => b,
        Err(err) => {
            eprintln!("error: signature download failed: {err}");
            return 1;
        }
    };

    // ── Step 9: Verify Ed25519 signature ─────────────────────────────────────
    if let Err(err) = updater::verify_ed25519_signature(&binary_bytes, &sig_bytes) {
        eprintln!("error: signature verification failed: {err}");
        return 1;
    }

    // ── Step 10: Stage binary ─────────────────────────────────────────────────
    println!("Staging update...");
    let staged_path = match updater::stage_file_and_metadata(&binary_bytes, &latest.tag_name) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("error: staging failed: {err}");
            return 1;
        }
    };

    // ── Step 11: Apply update ─────────────────────────────────────────────────
    println!("Applying update...");
    match updater::apply_pending_update_on_exit() {
        updater::ApplyOutcome::Applied {
            version,
            target_path,
        } => {
            println!(
                "Update applied. posterm {version} is now at {}",
                target_path.display()
            );
            0
        }
        updater::ApplyOutcome::NoPendingUpdate => {
            // Staged but apply didn't find it — staged path printed for manual use.
            println!(
                "Update staged at {}. Re-run posterm to apply.",
                staged_path.display()
            );
            0
        }
        updater::ApplyOutcome::PermissionDenied {
            version,
            staged_path,
            target_path,
        } => {
            eprintln!(
                "error: update {version} staged but could not be applied — permission denied."
            );
            eprintln!(
                "Copy the binary manually: cp {} {}",
                staged_path.display(),
                target_path.display()
            );
            1
        }
        updater::ApplyOutcome::Failed { version, reason } => {
            if let Some(v) = version {
                eprintln!("error: update {v} failed to apply: {reason}");
            } else {
                eprintln!("error: update apply step failed: {reason}");
            }
            1
        }
    }
}
