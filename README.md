# posterm

posterm is a TUI-based API client for testing HTTP APIs, built with Rust and Ratatui.

![Posterm](https://files.ksahoo.com/filebrowser/api/public/dl/VihfYa6f?inline=true)

## Install

### Script (macOS and Ubuntu — recommended)

```bash
curl -fsSL https://git.ksahoo.com/kanishkasahoo/posterm/raw/branch/main/scripts/install.sh | bash
```

To install a specific version:

```bash
curl -fsSL https://git.ksahoo.com/kanishkasahoo/posterm/raw/branch/main/scripts/install.sh | bash -s -- v1.2.3
```

Or clone the repo and run directly:

```bash
bash scripts/install.sh
bash scripts/install.sh v1.2.3   # pin to a version
```

The script detects your OS and architecture, downloads the matching release tarball from Forgejo Releases, verifies the SHA-256 checksum, and installs the binary to `/usr/local/bin/posterm` (using `sudo` if needed).

### From Releases

Pre-built artifacts are published on the [Releases](../../releases) page for each version:

| Platform | Formats |
|----------|---------|
| Linux (Ubuntu) | `posterm-linux-x86_64.tar.gz`, `posterm-linux-aarch64.tar.gz` |
| macOS | `posterm-macos-x86_64.tar.gz`, `posterm-macos-aarch64.tar.gz` |
| Windows | `posterm-windows-x86_64.zip` |

Download the artifact for your platform, extract or run the installer, and place the binary on your `PATH`.

### Build from Source

Requires a stable Rust toolchain with `cargo`.

```bash
cargo build --release
```

The binary is written to `target/release/posterm`.

To build release binaries for Windows, macOS, and Linux targets:

```bash
rustup target add \
  x86_64-pc-windows-gnu \
  x86_64-apple-darwin \
  aarch64-apple-darwin \
  x86_64-unknown-linux-gnu \
  aarch64-unknown-linux-gnu

cargo build-releases
```

This builds each release target, packages the binaries, and writes checksums to `dist/`:

```text
dist/posterm-macos-x86_64.tar.gz
dist/posterm-macos-aarch64.tar.gz
dist/posterm-linux-x86_64.tar.gz
dist/posterm-linux-aarch64.tar.gz
dist/posterm-windows-x86_64.zip
dist/*.sha256
dist/checksums.txt
```

Upload the files in `dist/` to the release. Cross-compiling may require platform linkers and SDKs beyond the Rust targets.

## Self-Update

Run `posterm upgrade` from your terminal to check for and apply the latest release:

```bash
posterm upgrade
```

The upgrade command checks the latest Forgejo release, downloads the matching OS/architecture artifact, verifies the SHA-256 checksum and Ed25519 signature, and replaces the running binary in-place. If the installation path requires elevated permissions, the staged binary path is printed for manual copy.

## Configuration

posterm stores configuration in a `posterm/` directory under the platform config dir:

| Platform | Path |
|----------|------|
| Linux | `$XDG_CONFIG_HOME/posterm` (or `~/.config/posterm`) |
| macOS | `~/Library/Application Support/posterm` |
| Windows | `%APPDATA%\posterm` |

`config.toml` is created with defaults on first run. Available options:

| Key | Default | Description |
|-----|---------|-------------|
| `default_timeout_secs` | `30` | Request timeout in seconds |
| `history_limit` | `200` | Maximum number of history entries to keep |
| `follow_redirects` | `true` | Follow HTTP redirects |
| `persist_sensitive_headers` | `false` | Whether to save sensitive headers to disk |
