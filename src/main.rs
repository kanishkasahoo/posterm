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

use app::App;
use tui::Tui;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    {
        let mut tui = Tui::new()?;
        let initial_size = tui.size()?;

        let mut app = App::new(initial_size);
        app.run(&mut tui).await?;
    }

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

    Ok(())
}
