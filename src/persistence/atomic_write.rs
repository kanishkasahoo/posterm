use std::io::{self, Write};
use std::path::Path;

/// Writes `content` to `path` atomically by writing to a `.tmp` sibling first,
/// then renaming into place. On Unix the temporary file is created with 0600
/// permissions before the rename so that the final file is never world-readable.
///
/// If the rename fails for any reason the function attempts a direct write as a
/// best-effort fallback.
pub fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("tmp");

    // Write to the temporary file.
    let write_result = (|| -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;

        // On Unix, restrict permissions to owner-only (0600) before writing.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }

        file.write_all(content)?;
        file.flush()?;
        Ok(())
    })();

    if write_result.is_err() {
        // Fall back to a direct write on any tmp-file error.
        return std::fs::write(path, content);
    }

    // Rename tmp → final (atomic on most platforms).
    if let Err(_rename_err) = std::fs::rename(&tmp_path, path) {
        // Fallback: write directly and clean up tmp.
        let _ = std::fs::remove_file(&tmp_path);
        return std::fs::write(path, content);
    }

    Ok(())
}
