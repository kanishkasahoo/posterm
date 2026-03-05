# posterm

posterm is a TUI-based API client for testing HTTP APIs, built with Rust and Ratatui.

<img width="2914" height="1678" alt="image" src="https://github.com/user-attachments/assets/dacda58b-3d19-4c48-8ca7-1543bf26ec33" />

## Install

### Script (macOS and Ubuntu — recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/kanishkasahoo/posterm/main/scripts/install.sh | bash
```

To install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/kanishkasahoo/posterm/main/scripts/install.sh | bash -s -- v1.2.3
```

Or clone the repo and run directly:

```bash
bash scripts/install.sh
bash scripts/install.sh v1.2.3   # pin to a version
```

The script detects your OS and architecture, downloads the matching release tarball from GitHub Releases, verifies the SHA-256 checksum, and installs the binary to `/usr/local/bin/posterm` (using `sudo` if needed).

### From Releases

Pre-built artifacts are published on the [Releases](../../releases) page for each version:

| Platform | Formats |
|----------|---------|
| Linux (Ubuntu) | `.tar.gz` |
| macOS | `.tar.gz`, `.pkg` |
| Windows | `.zip`, `.msi` |

Download the artifact for your platform, extract or run the installer, and place the binary on your `PATH`.

### Build from Source

Requires a stable Rust toolchain with `cargo`.

```bash
cargo build --release
```

The binary is written to `target/release/posterm`.

## Self-Update

Run `posterm upgrade` from your terminal to check for and apply the latest release:

```bash
posterm upgrade
```

The upgrade command checks the latest GitHub release, downloads the matching artifact, verifies the SHA-256 checksum and Ed25519 signature, and replaces the running binary in-place. If the installation path requires elevated permissions, the staged binary path is printed for manual copy.

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
