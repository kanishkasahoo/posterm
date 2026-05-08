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
| Linux (Ubuntu) | `posterm-linux-x86_64.tar.gz`, `posterm-linux-aarch64.tar.gz`, `.deb`, `.rpm` |
| macOS | `posterm-macos-x86_64.tar.gz`, `posterm-macos-aarch64.tar.gz`, `.dmg` |
| Windows | `posterm-windows-x86_64.zip`, `.msi`, `posterm-install.ps1` |

Download the artifact for your platform, extract or run the installer, and place the binary on your `PATH`.

### Build from Source

Requires a stable Rust toolchain with `cargo`.

```bash
cargo build --release
```

The binary is written to `target/release/posterm`.

Release packaging is handled by the Forgejo workflow in `.github/workflows/release.yml`.
Push a `v*` tag, or run the workflow manually, to build macOS, Linux, and Windows artifacts.
The old `cargo build-releases` local task is deprecated.

The workflow writes these release assets:

```text
posterm-macos-x86_64.tar.gz
posterm-macos-aarch64.tar.gz
posterm-linux-x86_64.tar.gz
posterm-linux-aarch64.tar.gz
posterm-windows-x86_64.zip
posterm-macos-x86_64.dmg
posterm-macos-aarch64.dmg
posterm-linux-x86_64.deb
posterm-linux-aarch64.deb
posterm-linux-x86_64.rpm
posterm-linux-aarch64.rpm
posterm-windows-x86_64.msi
posterm-install.sh
posterm-install.ps1
*.sha256
*.sig
checksums.txt
```

Set `FORGEJO_TOKEN` so the publish job can create/upload release assets, and
`POSTERM_UPDATE_SIGNING_KEY` to the Ed25519 private key PEM used by `posterm upgrade`.

## Self-Update

Run `posterm upgrade` from your terminal to check for and apply the latest release:

```bash
posterm upgrade
```

The upgrade command checks the latest Forgejo release API response, downloads the matching OS/architecture asset from that release, verifies the SHA-256 checksum and Ed25519 signature, and replaces the running binary in-place. If the installation path requires elevated permissions, the staged binary path is printed for manual copy.

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
