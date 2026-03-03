# posterm — Architecture Document

> **Version:** 0.1.0
> **Last Updated:** 2026-03-03
> **Status:** APPROVED

---

## 1. Architectural Overview

posterm follows a **Component-Action Architecture** — an Elm-inspired unidirectional data flow pattern that is the idiomatic standard for ratatui applications. This architecture cleanly separates event handling, state management, and rendering.

### 1.1 Core Architectural Pattern

```
 ┌──────────────────────────────────────────────────────────────────┐
 │                        Event Loop (tokio)                        │
 │                                                                  │
 │   ┌─────────────┐     ┌─────────────┐     ┌──────────────────┐  │
 │   │ Event Source │────>│   App       │────>│   Terminal       │  │
 │   │ (crossterm)  │     │  (dispatch) │     │   (ratatui)      │  │
 │   └─────────────┘     └──────┬──────┘     └──────────────────┘  │
 │                               │                     ▲            │
 │                               │ Actions             │ Render     │
 │                               ▼                     │            │
 │                        ┌──────────────┐    ┌────────┴─────────┐ │
 │                        │  Components  │───>│  State (AppState) │ │
 │                        │  (handle +   │    │                   │ │
 │                        │   update)    │    └───────────────────┘ │
 │                        └──────────────┘                          │
 │                               │                                  │
 │                               │ Spawn async                     │
 │                               ▼                                  │
 │                        ┌──────────────┐                          │
 │                        │ HTTP Worker  │                          │
 │                        │ (reqwest)    │                          │
 │                        └──────────────┘                          │
 └──────────────────────────────────────────────────────────────────┘
```

### 1.2 Data Flow

1. **Event Source** (crossterm + tokio timers) emits keyboard, resize, tick, and render events into an `mpsc` channel.
2. **App** receives events, maps them to **Actions** (an enum of all possible state transitions).
3. **Actions** are dispatched to the relevant **Component** which updates the shared **AppState**.
4. Long-running work (HTTP requests) is spawned as tokio tasks that send **Actions** back through the channel when complete.
5. On each render tick, **Components** read from **AppState** and produce ratatui widgets.

---

## 2. System Boundary Diagram

```
┌─────────────────────────── posterm process ───────────────────────────┐
│                                                                       │
│  ┌───────────────┐  ┌────────────────────────────────────────────┐   │
│  │  Config Layer  │  │            UI Layer (ratatui)              │   │
│  │  ─────────────  │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐  │   │
│  │  config.toml    │  │  │ Sidebar  │ │ Request  │ │ Response │  │   │
│  │  collections/   │  │  │Component │ │ Builder  │ │ Viewer   │  │   │
│  │  history.toml   │  │  │          │ │Component │ │Component │  │   │
│  └───────┬───────┘  │  └────┬─────┘ └────┬─────┘ └────┬─────┘  │   │
│          │           │       │            │            │          │   │
│          │           │       └────────────┼────────────┘          │   │
│          │           │                    │                       │   │
│          │           │              ┌─────▼──────┐               │   │
│          │           │              │  AppState   │               │   │
│          └───────────┼──────────────┤  (shared)   │               │   │
│                      │              └─────┬──────┘               │   │
│                      └────────────────────┼──────────────────────┘   │
│                                           │                          │
│  ┌────────────────────────────────────────▼──────────────────────┐   │
│  │                    HTTP Layer (reqwest)                        │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐  │   │
│  │  │ ClientPool   │  │ RequestExec  │  │ ResponseProcessor  │  │   │
│  │  │ (TLS, timeout│  │ (async spawn)│  │ (parse, stream)    │  │   │
│  │  │  redirect)   │  │              │  │                    │  │   │
│  │  └──────────────┘  └──────────────┘  └────────────────────┘  │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                           │                          │
│  ┌────────────────────────────────────────▼──────────────────────┐   │
│  │                 Persistence Layer (TOML + dirs)                │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐  │   │
│  │  │ ConfigStore  │  │ CollectionDB │  │   HistoryStore     │  │   │
│  │  └──────────────┘  └──────────────┘  └────────────────────┘  │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
          │                                           │
          ▼                                           ▼
   ┌──────────────┐                           ┌──────────────┐
   │   Filesystem  │                           │  HTTP Server │
   │  (~/.config/  │                           │  (remote)    │
   │   posterm/)   │                           └──────────────┘
   └──────────────┘
```

---

## 3. Component Architecture

### 3.1 Component Trait

All UI elements implement a common `Component` trait:

```
trait Component {
    fn init(action_tx: ActionSender) -> Result<Self>
    fn handle_key_event(key: KeyEvent, state: &AppState) -> Option<Action>
    fn handle_action(action: &Action, state: &mut AppState) -> Option<Action>
    fn render(frame: &mut Frame, area: Rect, state: &AppState)
}
```

### 3.2 Component Tree

```
App
├── LayoutManager              // Detects terminal size, selects layout mode
│   ├── Sidebar                // Collections tree + History list
│   │   ├── CollectionTree     // Expandable tree of named collections
│   │   └── HistoryList        // Reverse-chronological request history
│   ├── RequestBuilder         // Top-right pane (or full-screen in small mode)
│   │   ├── UrlBar             // Method selector + URL input
│   │   ├── QueryParamsEditor  // Key-value table, syncs with URL
│   │   ├── HeadersEditor      // Key-value table for custom headers
│   │   ├── AuthPanel          // Bearer / Basic auth helpers
│   │   └── BodyEditor         // JSON / Form body editor with format toggle
│   ├── ResponseViewer         // Bottom-right pane
│   │   ├── MetadataBar        // Status code, time, size
│   │   ├── ResponseBody       // Formatted (syntax-highlighted) body view
│   │   ├── RawView            // Unprocessed response bytes
│   │   ├── ResponseHeaders    // Response header table
│   │   └── SearchBar          // Text search with match navigation
│   └── StatusBar              // Context-sensitive keybinding hints
└── OverlayManager             // Manages modal overlays (sidebar on small/medium)
```

### 3.3 Component Responsibilities

| Component | Responsibility |
|---|---|
| **App** | Owns the event loop, action channel, AppState, and component tree. Dispatches events to focused component. |
| **LayoutManager** | Reads terminal dimensions from resize events, computes layout mode (small/medium/large), allocates `Rect` areas to child components. |
| **Sidebar** | Renders collection tree and history. Emits actions: `LoadRequest`, `SelectCollection`, `CreateCollection`, `DeleteRequest`. |
| **CollectionTree** | Renders expandable/collapsible folder tree. Handles arrow-key navigation, Enter to select, and CRUD actions. |
| **HistoryList** | Renders history entries. Handles selection to reload a historical request into the builder. |
| **RequestBuilder** | Container for the request editing components. Manages its internal tab state (Params / Headers / Auth / Body). |
| **UrlBar** | Text input for URL, dropdown for HTTP method. Parses URL to extract/sync query params. |
| **QueryParamsEditor** | Editable key-value table. Emits `SyncUrlFromParams` action. Handles `SyncParamsFromUrl` action. |
| **HeadersEditor** | Editable key-value table for request headers. Emits header changes to state. |
| **AuthPanel** | UI for Bearer token input and Basic auth username/password. Computes and emits the `Authorization` header value. |
| **BodyEditor** | Text area for body content. Toggle between JSON and form-encoded modes. Auto-sets `Content-Type`. |
| **ResponseViewer** | Container for response display components. Manages its internal tab state (Body / Headers / Raw / Search). |
| **MetadataBar** | Static display of status code (color-coded), elapsed time, body size. |
| **ResponseBody** | Syntax-highlighted formatted view with virtualized scrolling for large payloads. |
| **RawView** | Verbatim response bytes displayed in a scrollable text area. |
| **ResponseHeaders** | Read-only key-value table of response headers. |
| **SearchBar** | Text input with incremental search. Highlights matches in ResponseBody/RawView. Supports next/prev match navigation. |
| **StatusBar** | Reads the current focus context and displays relevant keybinding hints. |
| **OverlayManager** | In small/medium layout modes, renders the Sidebar as a floating overlay panel on top of the request builder area. |

---

## 4. Action System

All state mutations flow through a centralized Action enum dispatched over an `mpsc::UnboundedChannel`.

### 4.1 Action Categories

```
Action
├── Navigation
│   ├── FocusNext              // Tab — move focus to next pane/field
│   ├── FocusPrev              // Shift+Tab — move focus to previous pane/field
│   ├── FocusPane(PaneId)      // Direct focus to specific pane
│   ├── ToggleSidebar          // Show/hide sidebar overlay
│   └── ToggleRequestResponse  // Small mode: swap between request and response
│
├── Request Building
│   ├── SetMethod(HttpMethod)
│   ├── SetUrl(String)
│   ├── SyncUrlFromParams      // Rebuild URL from query param table
│   ├── SyncParamsFromUrl      // Rebuild query param table from URL
│   ├── SetQueryParam(usize, Key, Value)
│   ├── AddQueryParam
│   ├── RemoveQueryParam(usize)
│   ├── SetHeader(usize, Key, Value)
│   ├── AddHeader
│   ├── RemoveHeader(usize)
│   ├── SetAuthMode(AuthMode)  // None, Bearer, Basic
│   ├── SetAuthToken(String)
│   ├── SetAuthCredentials(username, password)
│   ├── SetBodyFormat(BodyFormat)  // JSON, FormEncoded
│   ├── SetBodyContent(String)
│   ├── SetTimeout(Option<Duration>)
│   ├── SetFollowRedirects(bool)
│   └── SetSkipTls(bool)
│
├── Request Execution
│   ├── SendRequest             // Trigger HTTP request
│   ├── CancelRequest(RequestId)
│   ├── RequestStarted(RequestId)
│   ├── RequestCompleted(RequestId, Response)
│   ├── RequestFailed(RequestId, ErrorInfo)
│   └── RequestCancelled(RequestId)
│
├── Response Viewing
│   ├── SetResponseTab(ResponseTab)  // Body, Headers, Raw, Search
│   ├── ScrollResponse(Direction, Amount)
│   ├── SearchInResponse(String)
│   ├── NextSearchMatch
│   └── PrevSearchMatch
│
├── Collections
│   ├── CreateCollection(name)
│   ├── RenameCollection(id, name)
│   ├── DeleteCollection(id)
│   ├── SaveRequestToCollection(collection_id, request)
│   ├── RenameRequest(collection_id, request_id, name)
│   ├── DeleteRequest(collection_id, request_id)
│   ├── ReorderRequest(collection_id, request_id, new_position)
│   └── LoadRequest(collection_id, request_id)
│
├── History
│   ├── LoadFromHistory(history_id)
│   └── ClearHistory
│
├── System
│   ├── Tick
│   ├── Render
│   ├── Resize(width, height)
│   └── Quit
│
└── Persistence
    ├── SaveCollections
    ├── SaveHistory
    └── SaveConfig
```

---

## 5. State Architecture

### 5.1 AppState Structure

```
AppState
├── focus: FocusTarget          // Which component currently has keyboard focus
├── layout_mode: LayoutMode     // Small, Medium, Large
├── terminal_size: (u16, u16)   // Current cols x rows
├── sidebar_visible: bool       // Overlay visibility in small/medium mode
│
├── request: RequestState
│   ├── method: HttpMethod
│   ├── url: String
│   ├── query_params: Vec<(String, String, bool)>  // key, value, enabled
│   ├── headers: Vec<(String, String, bool)>        // key, value, enabled
│   ├── auth_mode: AuthMode
│   ├── auth_token: Option<String>
│   ├── auth_username: Option<String>
│   ├── auth_password: Option<String>
│   ├── body_format: BodyFormat
│   ├── body_content: String
│   ├── timeout_override: Option<Duration>
│   ├── follow_redirects: bool
│   └── skip_tls: bool
│
├── response: Option<ResponseState>
│   ├── status_code: u16
│   ├── status_text: String
│   ├── elapsed_ms: u64
│   ├── body_size_bytes: u64
│   ├── headers: Vec<(String, String)>
│   ├── body_raw: StreamingBuffer      // Virtualized buffer for large responses
│   ├── body_formatted: Option<String> // Lazy-computed formatted version
│   ├── content_type: String
│   ├── active_tab: ResponseTab
│   ├── scroll_offset: usize
│   ├── search_query: Option<String>
│   ├── search_matches: Vec<(usize, usize)>  // (line, col)
│   └── search_cursor: usize                  // Index into search_matches
│
├── in_flight: HashMap<RequestId, InFlightRequest>
│   └── InFlightRequest
│       ├── cancel_token: CancellationToken
│       ├── started_at: Instant
│       └── method_url: (HttpMethod, String)
│
├── collections: Vec<Collection>
│   └── Collection
│       ├── id: Uuid
│       ├── name: String
│       ├── expanded: bool       // UI state: tree node open/closed
│       └── requests: Vec<SavedRequest>
│           └── SavedRequest
│               ├── id: Uuid
│               ├── name: String
│               └── request: RequestState   // Full request definition
│
├── history: Vec<HistoryEntry>
│   └── HistoryEntry
│       ├── id: Uuid
│       ├── timestamp: DateTime
│       ├── method: HttpMethod
│       ├── url: String
│       ├── status_code: Option<u16>
│       └── request: RequestState  // Full snapshot (sans sensitive headers)
│
└── config: AppConfig
    ├── default_timeout_secs: u64
    ├── history_limit: usize
    ├── tls_skip_ips: Vec<IpNetwork>
    ├── follow_redirects_default: bool
    └── persist_sensitive_headers: bool
```

### 5.2 State Ownership

- **AppState** is owned by the **App** struct and passed as `&AppState` (immutable) to `render()` and as `&mut AppState` to `handle_action()`.
- No component holds a reference to AppState between frames — components are stateless renderers of shared state.
- Component-local transient UI state (e.g., cursor position within a text input, dropdown open/closed) lives inside the component struct, not in AppState.

---

## 6. HTTP Layer Architecture

### 6.1 Client Configuration

A `reqwest::Client` is constructed at startup with global defaults from config:

```
ClientBuilder
  .timeout(config.default_timeout_secs)
  .redirect(Policy::limited(10))   // or Policy::none() based on config
  .user_agent("posterm/0.1.0")
  .build()
```

For requests that need TLS verification disabled, a second `Client` instance is constructed with `.danger_accept_invalid_certs(true)`.

### 6.2 Request Execution Flow

```
User presses Send
       │
       ▼
Action::SendRequest
       │
       ▼
App::handle_action:
  1. Build reqwest::Request from RequestState
  2. Generate unique RequestId
  3. Create CancellationToken
  4. Store InFlightRequest in state.in_flight
  5. Emit Action::RequestStarted(id)
  6. Clone action_tx channel
  7. tokio::spawn async task:
       │
       ▼
  ┌─ async task ────────────────────────────┐
  │  select! {                              │
  │    response = client.execute(request)   │
  │    _ = cancel_token.cancelled()         │
  │  }                                      │
  │                                         │
  │  On success:                            │
  │    action_tx.send(RequestCompleted)     │
  │  On error:                              │
  │    action_tx.send(RequestFailed)        │
  │  On cancel:                             │
  │    action_tx.send(RequestCancelled)     │
  └─────────────────────────────────────────┘
```

### 6.3 Response Streaming Strategy

For large responses:
1. The async task reads the response body in chunks using `response.chunk()`.
2. Each chunk is sent via the action channel: `Action::ResponseChunk(id, bytes)`.
3. The `StreamingBuffer` in AppState appends chunks and maintains a line index for virtualized rendering.
4. The ResponseBody component only renders lines visible in the current viewport, reading from the `StreamingBuffer` by line range.

### 6.4 TLS Skip Logic

```
When building the reqwest Request:
  IF request.skip_tls == true:
    Use the "danger" client (invalid certs accepted)
  ELSE IF request URL host IP is in config.tls_skip_ips:
    Use the "danger" client
  ELSE:
    Use the default strict client
```

---

## 7. Persistence Layer

### 7.1 Directory Structure

Platform-appropriate paths via the `dirs` crate:

```
~/.config/posterm/         (Linux — XDG_CONFIG_HOME)
~/Library/Application Support/posterm/  (macOS)
%APPDATA%/posterm/         (Windows)
│
├── config.toml            // Global settings
├── history.toml           // Request history
└── collections/
    ├── my-api.toml        // One file per collection
    └── another-api.toml
```

### 7.2 Persistence Strategy

| Data | Trigger | Method |
|---|---|---|
| **Config** | On change via settings UI, or first launch | Write full `config.toml` |
| **Collections** | On any collection mutation (add/edit/delete request) | Write the affected collection file |
| **History** | After each request completes | Append to `history.toml`; rewrite on eviction |

### 7.3 Write Safety

- All writes use **atomic file operations**: write to a `.tmp` file, then `rename()` over the target. This prevents corruption on crash.
- Persistence operations are **debounced**: rapid successive mutations (e.g., typing in a name) batch into a single write after 500ms of inactivity.

### 7.4 TOML Schema Overview

**config.toml:**
```
[general]
default_timeout_secs = 30
history_limit = 200
follow_redirects = true
persist_sensitive_headers = false

[tls]
skip_verification_ips = ["127.0.0.0/8", "10.0.0.0/8", "192.168.0.0/16"]
```

**history.toml:**
```
[[entries]]
id = "uuid"
timestamp = "2026-03-03T12:30:00Z"
method = "GET"
url = "https://api.example.com/users"
status_code = 200

[[entries.request]]
# Full RequestState snapshot (excluding sensitive headers)
```

**collections/my-api.toml:**
```
[collection]
id = "uuid"
name = "My API"

[[requests]]
id = "uuid"
name = "List Users"

[requests.definition]
method = "GET"
url = "https://api.example.com/users"
# ... full request state
```

---

## 8. Technology Stack

| Layer | Technology | Rationale |
|---|---|---|
| **Language** | Rust (edition 2024) | Memory safety, performance, strong type system |
| **TUI Framework** | ratatui + crossterm | De-facto standard for Rust TUI apps. Cross-platform terminal backend. |
| **Async Runtime** | tokio | Required by reqwest. Industry standard async runtime for Rust. |
| **HTTP Client** | reqwest | Ergonomic async HTTP client with TLS, redirect, timeout, streaming support. |
| **Serialization** | serde + toml | TOML parsing/generation with derive macros. |
| **Syntax Highlighting** | syntect | Terminal-compatible syntax highlighting for JSON, XML, HTML. |
| **Error Handling** | color-eyre | Rich error reports with context and backtraces for debugging. |
| **Platform Dirs** | dirs | Cross-platform config/data directory resolution. |
| **UUID Generation** | uuid | Unique IDs for collections, requests, and history entries. |
| **Date/Time** | chrono | Timestamp formatting for history entries. |
| **IP Parsing** | ipnet | CIDR range parsing for TLS skip whitelist. |

### 8.1 Dependency Summary

```
[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
syntect = "5"
color-eyre = "0.6"
dirs = "6"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
ipnet = { version = "2", features = ["serde"] }
tokio-util = "0.7"   # CancellationToken
unicode-width = "0.2" # Proper terminal text width calculation
```

---

## 9. Responsive Layout System

### 9.1 Layout Mode Detection

```
fn determine_layout_mode(cols: u16, rows: u16) -> LayoutMode:
  IF cols < 80 OR rows < 24:
    return LayoutMode::Small
  ELSE IF cols < 120:
    return LayoutMode::Medium
  ELSE:
    return LayoutMode::Large
```

### 9.2 Layout Allocation

**Large Mode (>= 120 cols, >= 24 rows):**
```
┌─── 30 cols ───┬──── remaining cols ─────────┐
│               │                              │
│   Sidebar     │   Request Builder (50%)      │
│   (fixed)     │                              │
│               ├──────────────────────────────│
│               │   Response Viewer (50%)      │
│               │                              │
├───────────────┴──────────────────────────────│
│                  Status Bar (1 row)          │
└──────────────────────────────────────────────┘
```

**Medium Mode (>= 80 cols, >= 24 rows, < 120 cols):**
```
┌──────────── full width ─────────────────────┐
│   Request Builder (50% height)              │
│                                              │
├──────────────────────────────────────────────│
│   Response Viewer (50% height)              │
│                                              │
├──────────────────────────────────────────────│
│   Status Bar (1 row)                        │
└──────────────────────────────────────────────┘
  + Sidebar as overlay (triggered by hotkey)
```

**Small Mode (< 80 cols OR < 24 rows):**
```
┌──────────── full width ─────────────────────┐
│                                              │
│   Request Builder OR Response Viewer         │
│   (toggle via hotkey)                        │
│                                              │
├──────────────────────────────────────────────│
│   Status Bar (1 row)                        │
└──────────────────────────────────────────────┘
  + Sidebar as full-screen overlay
```

---

## 10. Keyboard Navigation Model

### 10.1 Focus System

Focus flows through a ordered list of focusable targets. `Tab` advances forward, `Shift+Tab` goes backward.

**Large mode focus order:**
```
Sidebar → UrlBar → [Active Request Tab content] → [Active Response Tab content]
```

**Medium/Small mode focus order:**
```
UrlBar → [Active Request Tab content] → [Active Response Tab content]
```

### 10.2 Global Keybindings (always active)

| Key | Action |
|---|---|
| `Ctrl+S` | Send request |
| `Ctrl+C` | Cancel in-flight request (or quit if nothing in-flight) |
| `Ctrl+Q` | Quit application |
| `Tab` | Focus next pane/field |
| `Shift+Tab` | Focus previous pane/field |
| `Ctrl+B` | Toggle sidebar overlay (small/medium mode) |
| `Ctrl+R` | Toggle between request and response (small mode) |
| `?` | Show/hide help overlay |

### 10.3 Context-Sensitive Keybindings

| Context | Key | Action |
|---|---|---|
| Sidebar focused | `↑/↓` | Navigate items |
| Sidebar focused | `Enter` | Load selected request |
| Sidebar focused | `←/→` | Collapse/expand collection |
| Sidebar focused | `n` | New collection |
| Sidebar focused | `d` | Delete selected item |
| Key-value editor | `↑/↓` | Navigate rows |
| Key-value editor | `Enter` | Edit selected cell |
| Key-value editor | `a` | Add new row |
| Key-value editor | `d` | Delete selected row |
| Key-value editor | `Space` | Toggle row enabled/disabled |
| Text input | `←/→` | Move cursor |
| Text input | `Esc` | Exit editing mode |
| Response body | `↑/↓/PgUp/PgDn` | Scroll |
| Response body | `/` | Open search |
| Search active | `Enter/n` | Next match |
| Search active | `N` | Previous match |
| Search active | `Esc` | Close search |

---

## 11. Error Handling Strategy

### 11.1 Error Categories

| Category | Source | Handling |
|---|---|---|
| **Network errors** | DNS, connection refused, timeout | Display in response pane with error type, message, and elapsed time |
| **TLS errors** | Certificate validation failure | Display in response pane with suggestion to toggle TLS verification |
| **Parse errors** | Malformed URL, invalid header value | Inline validation message near the offending field, prevent send |
| **Response errors** | Non-2xx status codes | Display normally — these are valid responses, not app errors |
| **Persistence errors** | Disk write failure, corrupted TOML | Show notification banner; continue operating with in-memory state |
| **Internal errors** | Panics, unexpected states | Caught by color-eyre; clean terminal restore before exit with error report |

### 11.2 Error Display

- Network/TLS errors render in the Response Viewer pane in place of a response body.
- Validation errors render inline next to the invalid field (red text).
- Persistence errors render as a temporary notification bar above the status bar.

---

## 12. Architectural Decisions Record

### ADR-001: Component-Action Pattern over MVC

**Decision:** Use a Component-Action (Elm-inspired) architecture with a centralized Action enum and unidirectional data flow.

**Alternatives Considered:** MVC, actor model.

**Rationale:** This is the idiomatic ratatui pattern, used by the official ratatui templates and most production TUI apps. It provides clear separation of concerns, makes state transitions explicit and traceable, and maps naturally to ratatui's immediate-mode rendering.

### ADR-002: reqwest over curl-rust

**Decision:** Use reqwest as the HTTP client.

**Alternatives Considered:** curl-rust (libcurl bindings), hyper, ureq.

**Rationale:** reqwest is the most popular Rust HTTP client with native async/tokio integration, built-in TLS (rustls or native-tls), redirect policies, streaming response bodies, and per-request timeout overrides. It avoids the C dependency of libcurl while providing equivalent functionality.

### ADR-003: TOML over JSON for persistence

**Decision:** Use TOML for all persisted data.

**Alternatives Considered:** JSON, YAML.

**Rationale:** TOML is human-readable and hand-editable, common in the Rust ecosystem (Cargo.toml), and serde-compatible. Users who want to version-control their collections or tweak config by hand benefit from TOML's clean syntax.

### ADR-004: syntect for syntax highlighting

**Decision:** Use syntect for response body syntax highlighting.

**Alternatives Considered:** tree-sitter, manual regex-based highlighting.

**Rationale:** syntect provides Sublime Text-quality syntax highlighting with built-in grammars for JSON, XML, HTML and many others. It supports terminal color output and is well-maintained. tree-sitter is more powerful but adds significant complexity for this use case.

### ADR-005: Streaming buffer for large responses

**Decision:** Use a streaming chunked buffer with virtualized rendering instead of loading entire responses into memory.

**Alternatives Considered:** Full in-memory load with a size cap.

**Rationale:** Enables handling responses up to 50MB+ without proportional memory usage. The virtualized renderer only processes and syntax-highlights lines visible in the viewport, keeping CPU and memory usage bounded regardless of response size.

### ADR-006: Two reqwest::Client instances for TLS

**Decision:** Maintain two `reqwest::Client` instances — one strict (default) and one permissive (danger_accept_invalid_certs).

**Alternatives Considered:** Single client with per-request TLS configuration.

**Rationale:** reqwest configures TLS at the Client level, not per-request. Two pre-built clients avoid the overhead of constructing a new Client for every request while supporting both TLS modes.

---

## 13. Security Considerations

| Concern | Mitigation |
|---|---|
| **Sensitive headers in history** | Authorization and Cookie headers are stripped from history by default. Opt-in via config. |
| **TLS bypass** | TLS skip is disabled by default. Per-request toggle requires explicit user action. IP whitelist is configurable but defaults to empty (user must opt-in). |
| **Credential storage** | Auth credentials in saved collection requests are stored in plaintext TOML on disk. File permissions should be user-only (0600). |
| **Terminal restore** | On panic or unexpected exit, the terminal is restored to its original state via a panic hook (crossterm cleanup). |
