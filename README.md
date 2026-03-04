# posterm

Terminal-based HTTP client built with Rust + Ratatui.

## Install from Releases

CI publishes installable artifacts on GitHub Releases for supported platforms:

- Linux: `.tar.gz`
- macOS: `.tar.gz` and `.pkg`
- Windows: `.zip` and `.msi`

Download the artifact for your OS from the repo's **Releases** page, then install/run:

- Linux/macOS `.tar.gz`: extract and run the `posterm` binary
- macOS `.pkg`: open the package and follow the installer
- Windows `.zip`: extract and run `posterm.exe`
- Windows `.msi`: run the installer and launch from Start Menu/terminal

## Build from source

Prerequisites:

- Rust toolchain (stable) with `cargo` installed
- A terminal that supports TUI applications

Install Rust (if needed):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

From the project root, build the app:

```bash
cargo build
```

Run in development mode:

```bash
cargo run
```

Run optimized mode:

```bash
cargo run --release
```

## Configuration and History

posterm stores local state in a single app config directory (`posterm/`) resolved from `dirs::config_dir()`, with a fallback to `$HOME/.config/posterm`.

Typical resolved paths:

- Linux: `$XDG_CONFIG_HOME/posterm` (or `~/.config/posterm`)
- macOS: `~/Library/Application Support/posterm`
- Windows: `%APPDATA%\\posterm`
- Fallback (if platform config dir lookup fails): `$HOME/.config/posterm`

### What gets created, and when

At startup (`App::new`):

- `posterm/collections/` is created immediately
- `posterm/config.toml` is created immediately if missing, using defaults
- `posterm/history.toml` is **not** created on startup

`history.toml` is written when history is persisted (after request completion, explicit history record, or clear-history action), via debounced saves.

### `config.toml`

`config.toml` is deserialized into `AppConfig`. If the file is missing or invalid TOML, defaults are used.

```toml
default_timeout_secs = 30
history_limit = 200
follow_redirects = true
persist_sensitive_headers = false
```

Current behavior tied to these fields in code:

- `history_limit`: caps in-memory history and persisted history (`take(limit)` from newest first)
- `persist_sensitive_headers`: controls redaction for saved history snapshots and collection snapshots
- `default_timeout_secs` and `follow_redirects`: currently persisted but not applied to request execution path yet

### `history.toml`

History is stored as TOML `[[entries]]`, newest entry first.

Each entry contains:

- `id` (UUID string)
- `timestamp_secs` (Unix seconds)
- `method`, `url`
- optional `status_code`, `elapsed_ms`
- optional `request` snapshot (`SavedRequest`), including method/url/query/header/auth/body fields

Representative snippet (field names/shape from serde structs):

```toml
[[entries]]
id = "5d6fd02e-7fc2-4c34-9ed5-bceec8df89a2"
timestamp_secs = 1700000000
method = "GET"
url = "https://example.com/users"
status_code = 200
elapsed_ms = 42

[entries.request]
id = "f95a84cf-8d35-4525-96c0-af2f4cc24313"
name = ""
method = "GET"
url = "https://example.com/users"
auth_mode = "None"
auth_token = ""
auth_username = ""
auth_password = ""
body_format = "JSON"
body_json = ""
```

Persistence/retention details:

- Debounced writes: saves are scheduled and flushed after ~500ms of inactivity on that target
- Flushes happen on tick events (app tick is 250ms)
- Limit enforcement happens both when entries are added and again when writing to disk
- No history file rotation/archive is implemented (single `history.toml` is rewritten)
- On read error or TOML parse error, history loads as empty

### Security and privacy notes

- Sensitive headers are identified case-insensitively by name: `Authorization`, `Cookie`, `Set-Cookie`, `Proxy-Authorization`
- If `persist_sensitive_headers = false` (default), persisted snapshots replace sensitive header values with `"[REDACTED]"` and clear `auth_token`, `auth_username`, and `auth_password`
- If `persist_sensitive_headers = true`, those values are stored as entered
- Persistence uses atomic write (`*.tmp` then rename); on Unix, temp files are set to `0600` before rename

Optional environment variable:

- `POSTERM_ALLOW_INSECURE_TLS=1` allows selecting intentionally insecure TLS mode in request execution

## Useful scripts

- Run tests:

  ```bash
  cargo test
  ```

- Build release binary:

  ```bash
  cargo build --release
  ```
