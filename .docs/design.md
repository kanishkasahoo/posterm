# posterm — Detailed Design

> **Version:** 0.1.0
> **Last Updated:** 2026-03-03
> **Status:** APPROVED

---

## 1. Module Structure

```
src/
├── main.rs                    // Entry point: parse args, init tokio, run App
├── app.rs                     // App struct, event loop, action dispatch
├── action.rs                  // Action enum (all possible state transitions)
├── state.rs                   // AppState struct and sub-state types
├── event.rs                   // Event enum, EventHandler (crossterm reader)
├── tui.rs                     // Terminal setup/teardown, Tui wrapper struct
│
├── components/                // UI Components (implement Component trait)
│   ├── mod.rs                 // Component trait definition
│   ├── layout_manager.rs      // Responsive layout computation
│   ├── sidebar/
│   │   ├── mod.rs             // Sidebar container
│   │   ├── collection_tree.rs // Expandable collection folder tree
│   │   └── history_list.rs    // History entry list
│   ├── request_builder/
│   │   ├── mod.rs             // Request builder container + tab management
│   │   ├── url_bar.rs         // Method selector + URL text input
│   │   ├── query_params.rs    // Key-value editor for query params
│   │   ├── headers_editor.rs  // Key-value editor for headers
│   │   ├── auth_panel.rs      // Bearer / Basic auth UI
│   │   └── body_editor.rs     // JSON / Form body editor
│   ├── response_viewer/
│   │   ├── mod.rs             // Response viewer container + tab management
│   │   ├── metadata_bar.rs    // Status, time, size display
│   │   ├── response_body.rs   // Formatted + syntax-highlighted body
│   │   ├── raw_view.rs        // Raw response bytes
│   │   ├── response_headers.rs// Response header table
│   │   └── search_bar.rs      // Text search with match navigation
│   ├── status_bar.rs          // Keybinding hints footer
│   └── overlay_manager.rs     // Modal overlay rendering
│
├── http/                      // HTTP client layer
│   ├── mod.rs                 // Public API: execute_request, cancel_request
│   ├── client.rs              // Client pool (strict + permissive)
│   ├── request_builder.rs     // Convert RequestState → reqwest::Request
│   └── response_processor.rs  // Parse response, chunk streaming, content detection
│
├── persistence/               // Disk I/O layer
│   ├── mod.rs                 // Public API
│   ├── config.rs              // Load/save config.toml
│   ├── collections.rs         // Load/save collection TOML files
│   ├── history.rs             // Load/save history.toml
│   └── atomic_write.rs        // Atomic write-to-tmp-then-rename utility
│
├── highlight/                 // Syntax highlighting
│   ├── mod.rs                 // Public API: highlight_content(content, content_type)
│   └── theme.rs               // Terminal-native color mapping for syntect
│
└── util/                      // Shared utilities
    ├── mod.rs
    ├── streaming_buffer.rs    // Chunked append-only buffer with line index
    └── url_parser.rs          // URL ↔ query param bidirectional sync
```

---

## 2. Data Model Specifications

### 2.1 Core Types

```
HttpMethod: enum { GET, POST, PUT, PATCH, DELETE }

AuthMode: enum { None, Bearer, Basic }

BodyFormat: enum { None, Json, FormEncoded }

LayoutMode: enum { Small, Medium, Large }

FocusTarget: enum {
    Sidebar,
    UrlBar,
    MethodSelector,
    QueryParams,
    Headers,
    AuthPanel,
    BodyEditor,
    ResponseBody,
    ResponseHeaders,
    RawView,
    SearchBar,
}

RequestTab: enum { Params, Headers, Auth, Body }

ResponseTab: enum { Body, Headers, Raw, Search }

PaneId: enum { Sidebar, RequestBuilder, ResponseViewer }
```

### 2.2 RequestState

```
RequestState {
    method: HttpMethod           // Default: GET
    url: String                  // Default: ""
    query_params: Vec<KeyValueRow>
    headers: Vec<KeyValueRow>
    auth_mode: AuthMode          // Default: None
    auth_token: String           // Default: "" (used when auth_mode == Bearer)
    auth_username: String        // Default: "" (used when auth_mode == Basic)
    auth_password: String        // Default: "" (used when auth_mode == Basic)
    body_format: BodyFormat      // Default: None
    body_content: String         // Default: ""
    timeout_override: Option<u64>   // Seconds; None = use global default
    follow_redirects: bool       // Default: from config
    skip_tls: bool               // Default: false
}

KeyValueRow {
    key: String
    value: String
    enabled: bool               // Default: true
}
```

### 2.3 ResponseState

```
ResponseState {
    request_id: Uuid
    status_code: u16
    status_text: String           // e.g., "OK", "Not Found"
    elapsed_ms: u64
    body_size_bytes: u64
    headers: Vec<(String, String)>
    body: StreamingBuffer         // Chunked buffer for large bodies
    content_type: ContentType     // Parsed from Content-Type header
    active_tab: ResponseTab       // Default: Body
    scroll_offset: usize          // Line offset for virtualized rendering
    search: SearchState
}

ContentType: enum {
    Json,
    Xml,
    Html,
    PlainText,
    Other(String),
}

SearchState {
    query: String
    matches: Vec<SearchMatch>     // Pre-computed match locations
    cursor: usize                 // Which match is currently highlighted
    active: bool                  // Is the search bar open?
}

SearchMatch {
    line: usize
    col_start: usize
    col_end: usize
}
```

### 2.4 StreamingBuffer

```
StreamingBuffer {
    chunks: Vec<Vec<u8>>         // Raw byte chunks as received
    total_bytes: usize
    line_index: Vec<LineSpan>    // Index mapping line number → chunk + offset
    complete: bool               // Has the full response been received?
}

LineSpan {
    chunk_idx: usize
    byte_offset: usize
    byte_len: usize
}

Methods:
    append(chunk: Vec<u8>)       // Appends chunk and updates line_index
    line_count() -> usize
    get_line(n: usize) -> &str   // Returns the nth line by looking up LineSpan
    get_lines(start: usize, count: usize) -> Vec<&str>  // Viewport range
    as_full_string() -> String   // For small responses; concatenates all chunks
    total_bytes() -> usize
```

### 2.5 Collection

```
Collection {
    id: Uuid
    name: String
    expanded: bool                // UI-only; not persisted
    requests: Vec<SavedRequest>
}

SavedRequest {
    id: Uuid
    name: String
    request: RequestState
}
```

### 2.6 HistoryEntry

```
HistoryEntry {
    id: Uuid
    timestamp: DateTime<Utc>
    method: HttpMethod
    url: String
    status_code: Option<u16>     // None if request failed
    elapsed_ms: Option<u64>
    request: RequestState         // Snapshot at time of send (sensitive headers redacted)
}
```

### 2.7 AppConfig

```
AppConfig {
    default_timeout_secs: u64     // Default: 30
    history_limit: usize          // Default: 200
    tls_skip_ips: Vec<IpNet>      // Default: [] (empty)
    follow_redirects: bool        // Default: true
    persist_sensitive_headers: bool // Default: false
}
```

---

## 3. Component Trait Specification

### 3.1 Trait Definition

```
Component trait:
    fn init(&mut self, action_tx: UnboundedSender<Action>) -> Result<()>
        Called once after construction. Stores the action channel sender for
        emitting actions.

    fn handle_key_event(&mut self, key: KeyEvent, state: &AppState) -> Option<Action>
        Called when this component has focus and a key event occurs.
        Returns Some(Action) to dispatch, or None if the key was consumed
        internally (e.g., cursor movement within a text input).

    fn handle_action(&mut self, action: &Action, state: &mut AppState) -> Option<Action>
        Called for every dispatched action. Component updates AppState if
        the action is relevant. Returns Some(Action) to chain a follow-up
        action, or None.

    fn render(&self, frame: &mut Frame, area: Rect, state: &AppState)
        Called on every render tick. Component draws its widgets into the
        given area using the current AppState. Must be pure — no side effects.

    fn focusable(&self) -> bool
        Returns true if this component can receive keyboard focus.
        Default: true.

    fn focus_id(&self) -> FocusTarget
        Returns this component's FocusTarget identifier for the focus system.
```

### 3.2 Component Lifecycle

```
1. Construction:  component = Component::new(...)
2. Init:          component.init(action_tx.clone())
3. Event Loop:
   a. Event arrives → if component has focus → component.handle_key_event(key, &state)
   b. Action dispatched → for each component → component.handle_action(&action, &mut state)
   c. Render tick → for each component → component.render(frame, area, &state)
4. Teardown:      implicit via Drop (no special cleanup needed)
```

---

## 4. Key Component Designs

### 4.1 UrlBar Component

**State (component-local):**
```
UrlBar {
    cursor_pos: usize            // Character position in URL string
    editing: bool                // Is the text input active?
    method_dropdown_open: bool   // Is the method dropdown expanded?
}
```

**Rendering:**
- Left section: method selector button showing current method (e.g., `[GET ▾]`), color-coded by method.
- Right section: full-width text input for the URL.
- When method dropdown is open, render a floating list of methods below the button.

**Method Colors:**
| Method | Color |
|---|---|
| GET | Green |
| POST | Yellow |
| PUT | Blue |
| PATCH | Cyan |
| DELETE | Red |

**Key Handling:**
- When method selector focused: `↑/↓` or `Enter` to cycle/select method.
- When URL input focused: standard text editing (insert character, backspace, delete, `←/→` cursor, `Home/End`).
- On URL change: parse query string, emit `Action::SyncParamsFromUrl`.

### 4.2 KeyValueEditor Component (shared by QueryParams, Headers)

**State (component-local):**
```
KeyValueEditor {
    selected_row: usize
    selected_col: Column          // Key | Value | EnabledToggle
    editing: bool                 // Is a cell being edited?
    cursor_pos: usize             // Cursor within the editing cell
    scroll_offset: usize          // For scrolling long lists
}

Column: enum { Key, Value, Enabled }
```

**Rendering:**
```
┌──────────────────┬──────────────────────────┬───┐
│ Key              │ Value                    │ ✓ │
├──────────────────┼──────────────────────────┼───┤
│ page             │ 1                        │ ✓ │  ← selected row highlighted
│ limit            │ 20                       │ ✓ │
│ sort             │ name                     │   │  ← disabled (dimmed)
│                  │                          │   │  ← empty row (placeholder for "add")
└──────────────────┴──────────────────────────┴───┘
```

**Key Handling:**
- `↑/↓`: move between rows.
- `←/→`: move between Key/Value/Enabled columns.
- `Enter`: start editing the selected cell (if Key or Value).
- `Space`: toggle Enabled when on the checkbox column.
- `a`: append a new empty row.
- `d`: delete selected row (with confirmation if non-empty).
- `Esc`: exit editing mode.

### 4.3 BodyEditor Component

**State (component-local):**
```
BodyEditor {
    cursor_line: usize
    cursor_col: usize
    editing: bool
    scroll_offset: usize
}
```

**Rendering:**
- Top bar: format toggle `[JSON] [Form]` — active tab highlighted.
- Below: multi-line text area.
  - In JSON mode: line numbers on the left, syntax highlighting for JSON tokens.
  - In Form mode: key-value editor (reuses KeyValueEditor component) with the form data serialized to `key=value&...` format internally.

**Key Handling:**
- Standard multi-line text editing: arrow keys, Enter for newline, Backspace/Delete.
- `Ctrl+F`: format/pretty-print JSON (when in JSON mode).
- Format toggle: `Ctrl+1` for JSON, `Ctrl+2` for Form.

### 4.4 AuthPanel Component

**Rendering:**
```
Auth Mode: [None] [Bearer] [Basic]

--- When Bearer selected ---
Token: [____________________________________]

--- When Basic selected ---
Username: [____________________________]
Password: [****************************]
```

**Behavior:**
- Switching auth mode emits `Action::SetAuthMode`.
- When Bearer is active, the component computes `Authorization: Bearer <token>` and emits it as a managed header.
- When Basic is active, the component base64-encodes `username:password` and emits `Authorization: Basic <encoded>`.
- The managed Authorization header is shown in the headers editor as read-only/dimmed with a label "(managed by auth)".

### 4.5 ResponseBody Component (Virtualized)

**Rendering Strategy:**

```
Given:
  viewport_height = area.height - 2  // minus borders/tabs
  scroll_offset = state.response.scroll_offset

Steps:
  1. Determine visible line range:
     start_line = scroll_offset
     end_line = min(scroll_offset + viewport_height, buffer.line_count())

  2. Fetch visible lines from StreamingBuffer:
     lines = buffer.get_lines(start_line, viewport_height)

  3. Apply syntax highlighting (if formatted view):
     highlighted_lines = highlighter.highlight_lines(lines, content_type)

  4. Apply search match highlighting (if search active):
     For each visible line, check if any SearchMatch falls within.
     Apply reverse-video or bright background to matched spans.

  5. Render lines into the area with ratatui's Paragraph or custom Spans.

  6. Render scroll indicator on right edge showing position within full document.
```

**Scroll Indicator:**
- A thin vertical bar on the right edge of the viewport.
- Height proportional to `viewport_height / total_lines`.
- Position proportional to `scroll_offset / total_lines`.

### 4.6 Sidebar Component

**Rendering:**
```
┌─ Collections ─────────┐
│ ▼ My API              │   ← expanded folder
│   ● GET /users        │   ← selected (highlighted)
│   ● POST /users       │
│   ● GET /users/:id    │
│ ▶ Another API         │   ← collapsed folder
│                        │
├─ History ─────────────┤
│ 12:30 GET /users  200 │   ← green status
│ 12:28 POST /users 201 │   ← green status
│ 12:25 GET /health 500 │   ← red status
│ 12:20 DELETE /u/3 204 │   ← green status
└────────────────────────┘
```

**Tree Navigation:**
- `↑/↓`: move selection through the flat projection of the tree (collapsed folders skip children).
- `→` on a collapsed folder: expand it.
- `←` on an expanded folder: collapse it.
- `←` on a request inside a folder: move selection to parent folder.
- `Enter` on a request: emit `Action::LoadRequest` to populate the request builder.
- `Enter` on a history entry: emit `Action::LoadFromHistory`.

**Status Code Color Coding:**
| Range | Color |
|---|---|
| 2xx | Green |
| 3xx | Yellow |
| 4xx | Red |
| 5xx | Bright Red / Bold |
| Error (no code) | Dim / Gray |

---

## 5. HTTP Layer Detailed Design

### 5.1 Client Pool

```
HttpClientPool {
    strict_client: reqwest::Client     // Default: TLS verified
    permissive_client: reqwest::Client // danger_accept_invalid_certs
    config: Arc<AppConfig>
}

Methods:
    new(config: &AppConfig) -> Self
        Builds both clients with:
          - timeout from config.default_timeout_secs
          - redirect policy from config.follow_redirects
          - user_agent: "posterm/{version}"
          - strict: default TLS
          - permissive: danger_accept_invalid_certs(true)

    get_client(&self, request_state: &RequestState) -> &reqwest::Client
        IF request_state.skip_tls:
            return &self.permissive_client
        IF url host IP is in config.tls_skip_ips:
            return &self.permissive_client
        ELSE:
            return &self.strict_client
```

### 5.2 Request Builder (HTTP)

```
build_request(
    client: &reqwest::Client,
    state: &RequestState,
    config: &AppConfig
) -> Result<reqwest::Request>

Steps:
  1. Parse URL from state.url.
  2. Merge enabled query_params into the URL.
  3. Set HTTP method from state.method.
  4. Add enabled headers from state.headers.
  5. Compute and add Authorization header based on state.auth_mode:
     - Bearer: "Bearer {state.auth_token}"
     - Basic: "Basic {base64(state.auth_username:state.auth_password)}"
  6. Set body based on state.body_format:
     - Json: set body as string, Content-Type: application/json
     - FormEncoded: parse body as key=value pairs, set as form body
     - None: no body
  7. Set per-request timeout if state.timeout_override is Some.
  8. Configure redirect policy: follow if state.follow_redirects, otherwise none.
  9. Build and return the reqwest::Request.
```

### 5.3 Request Execution

```
execute_request(
    client_pool: &HttpClientPool,
    request_state: &RequestState,
    request_id: Uuid,
    action_tx: UnboundedSender<Action>,
    cancel_token: CancellationToken,
)

Spawns a tokio task that:
  1. Selects the appropriate client from the pool.
  2. Builds the reqwest::Request.
  3. Executes with tokio::select!:
     - Branch A: response = client.execute(request).await
     - Branch B: cancel_token.cancelled().await
  4. On Branch A success:
     a. Read response status, headers, elapsed time.
     b. Stream body in chunks:
        LOOP:
          chunk = response.chunk().await
          IF chunk is Some(bytes):
            action_tx.send(Action::ResponseChunk(request_id, bytes))
          ELSE:
            break (body complete)
     c. action_tx.send(Action::RequestCompleted(request_id, metadata))
  5. On Branch A error:
     action_tx.send(Action::RequestFailed(request_id, error_info))
  6. On Branch B (cancelled):
     action_tx.send(Action::RequestCancelled(request_id))
```

### 5.4 Response Content Type Detection

```
detect_content_type(headers: &HeaderMap) -> ContentType

Steps:
  1. Read "content-type" header value.
  2. Parse MIME type:
     - "application/json" or "+json" suffix → ContentType::Json
     - "application/xml" or "text/xml" or "+xml" suffix → ContentType::Xml
     - "text/html" → ContentType::Html
     - "text/plain" → ContentType::PlainText
     - anything else → ContentType::Other(raw_value)
  3. If no content-type header: attempt heuristic detection:
     - First non-whitespace char is '{' or '[' → Json
     - First non-whitespace char is '<' → Xml or Html (check for <!DOCTYPE html or <html)
     - Otherwise → PlainText
```

---

## 6. Persistence Layer Detailed Design

### 6.1 Atomic Write Utility

```
atomic_write(path: &Path, content: &[u8]) -> Result<()>

Steps:
  1. Generate temp path: path.with_extension("tmp")
  2. Write content to temp path.
  3. Set file permissions to 0600 (user-only, for security).
  4. Rename temp path to target path (atomic on most filesystems).
  5. On rename failure: attempt direct write as fallback.
```

### 6.2 Debounced Persistence

```
PersistenceManager {
    config_path: PathBuf
    collections_dir: PathBuf
    history_path: PathBuf
    debounce_timer: HashMap<PersistTarget, Instant>
    debounce_interval: Duration  // 500ms
}

PersistTarget: enum { Config, Collection(Uuid), History }

Methods:
    schedule_save(&mut self, target: PersistTarget)
        Records current time for the target.

    flush_pending(&mut self, state: &AppState) -> Result<()>
        For each target in debounce_timer:
            IF elapsed since last schedule > debounce_interval:
                Perform the actual write.
                Remove from debounce_timer.
```

The event loop calls `flush_pending` on every Tick event (every 250ms), so writes happen at most 500ms after the last change.

### 6.3 Config TOML Schema

```
# config.toml

[general]
default_timeout_secs = 30
history_limit = 200
follow_redirects = true
persist_sensitive_headers = false

[tls]
skip_verification_ips = ["127.0.0.0/8", "10.0.0.0/8"]
```

### 6.4 Collection TOML Schema

```
# collections/my-api.toml

[collection]
id = "550e8400-e29b-41d4-a716-446655440000"
name = "My API"

[[requests]]
id = "6ba7b810-9dad-11d1-80b4-00c04fd430c8"
name = "List Users"

[requests.definition]
method = "GET"
url = "https://api.example.com/users"
body_format = "None"
body_content = ""
follow_redirects = true
skip_tls = false

[[requests.definition.query_params]]
key = "page"
value = "1"
enabled = true

[[requests.definition.query_params]]
key = "limit"
value = "20"
enabled = true

[[requests.definition.headers]]
key = "Accept"
value = "application/json"
enabled = true

[requests.definition.auth]
mode = "Bearer"
token = "eyJhbGciOiJIUzI1NiIs..."
username = ""
password = ""
```

### 6.5 History TOML Schema

```
# history.toml

[[entries]]
id = "7c9e6679-7425-40de-944b-e07fc1f90ae7"
timestamp = 2026-03-03T12:30:00Z
method = "GET"
url = "https://api.example.com/users"
status_code = 200
elapsed_ms = 342

[entries.request]
method = "GET"
url = "https://api.example.com/users?page=1&limit=20"
body_format = "None"
body_content = ""
follow_redirects = true
skip_tls = false
# auth and sensitive headers omitted by default
```

---

## 7. Syntax Highlighting Design

### 7.1 Highlighting Pipeline

```
highlight_content(content: &str, content_type: ContentType) -> Vec<HighlightedLine>

Steps:
  1. Select syntect SyntaxReference based on content_type:
     - Json → "JSON"
     - Xml → "XML"
     - Html → "HTML"
     - PlainText → "Plain Text"
     - Other → "Plain Text" (fallback)
  2. Create syntect HighlightIterator with a terminal-native theme.
  3. For each line:
     a. Run highlighter to produce styled spans.
     b. Map syntect styles to ratatui Style (using terminal ANSI colors).
     c. Produce HighlightedLine { spans: Vec<(Style, String)> }.
  4. Return the full list.

HighlightedLine {
    spans: Vec<(ratatui::style::Style, String)>
}
```

### 7.2 Terminal-Native Color Mapping

To respect the user's terminal theme, syntect scope colors are mapped to the 16 standard ANSI colors:

| Syntect Scope | Terminal Color |
|---|---|
| String | Green |
| Number | Cyan |
| Keyword / Boolean | Yellow |
| Property key (JSON) | Blue |
| Punctuation (braces, brackets) | Default foreground |
| Comment | Dark gray / Dim |
| Error / Invalid | Red |
| Tag name (HTML/XML) | Magenta |
| Attribute name | Yellow |
| Attribute value | Green |

### 7.3 Lazy Highlighting

For large responses, highlighting is performed **lazily per viewport**:
1. Only lines within the current viewport range are highlighted.
2. A cache stores highlighted lines by line number.
3. When the user scrolls, newly-visible lines are highlighted on demand.
4. The cache is bounded (e.g., 2x viewport size) — lines scrolled far past are evicted.

---

## 8. URL ↔ Query Param Bidirectional Sync

### 8.1 URL → Params (SyncParamsFromUrl)

```
parse_query_params(url: &str) -> Vec<KeyValueRow>

Steps:
  1. Parse URL using the url crate (or manual split on '?').
  2. Split query string on '&'.
  3. For each segment, split on first '=' to get key and value.
  4. URL-decode both key and value.
  5. Return as Vec<KeyValueRow> with enabled = true.
```

### 8.2 Params → URL (SyncUrlFromParams)

```
rebuild_url_with_params(base_url: &str, params: &[KeyValueRow]) -> String

Steps:
  1. Parse the current URL and strip its existing query string.
  2. Filter params to only enabled entries.
  3. URL-encode each key and value.
  4. Join with '&' to form the query string.
  5. Append '?' + query string to the base URL.
  6. If no enabled params, return base URL without '?'.
```

### 8.3 Conflict Resolution

- When the URL changes (user types in the URL bar), the query params table is **fully replaced** by the parsed result.
- When a query param changes (user edits the table), the URL's query portion is **rebuilt** from the table, but the URL's path and fragment are preserved.
- Editing is debounced by 300ms to avoid thrashing during rapid typing.

---

## 9. Focus and Input System Design

### 9.1 Focus Manager

```
FocusManager {
    focus_order: Vec<FocusTarget>    // Computed from current layout mode
    current_index: usize
}

Methods:
    next() -> FocusTarget
        Advances current_index by 1 (wrapping).

    prev() -> FocusTarget
        Decrements current_index by 1 (wrapping).

    set(target: FocusTarget)
        Finds target in focus_order and sets current_index.

    current() -> FocusTarget
        Returns focus_order[current_index].

    rebuild(layout_mode: LayoutMode, request_tab: RequestTab, response_tab: ResponseTab)
        Rebuilds focus_order based on layout mode and active tabs.
        E.g., in Large mode:
          [Sidebar, UrlBar, QueryParams/Headers/Auth/Body, ResponseBody/ResponseHeaders/RawView/SearchBar]
```

### 9.2 Input Modes

The app operates in two input modes:

```
InputMode: enum { Normal, Editing }
```

**Normal Mode:** Key events are handled as navigation commands (Tab, arrows, hotkeys). Characters are NOT inserted into text fields.

**Editing Mode:** Entered when the user presses Enter on a focusable text field. Key events are handled as text input. Esc returns to Normal mode.

This prevents accidental text input while navigating and provides clear modal feedback.

---

## 10. Error Display Specifications

### 10.1 Network Error in Response Pane

```
┌─ Response ────────────────────────────────────────┐
│  ✗ ERROR  |  Connection Refused  |  12ms          │
│                                                    │
│  ┌─ Error Details ──────────────────────────────┐ │
│  │ Could not connect to https://api.example.com │ │
│  │                                               │ │
│  │ Error: connection refused (os error 61)       │ │
│  │ Host: api.example.com:443                     │ │
│  │ Elapsed: 12ms                                 │ │
│  └───────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────┘
```

### 10.2 TLS Error with Hint

```
┌─ Response ────────────────────────────────────────┐
│  ✗ TLS ERROR  |  Certificate Invalid  |  45ms     │
│                                                    │
│  ┌─ Error Details ──────────────────────────────┐ │
│  │ TLS handshake failed for https://localhost    │ │
│  │                                               │ │
│  │ Error: invalid certificate: self-signed       │ │
│  │                                               │ │
│  │ Hint: Toggle TLS verification off for this    │ │
│  │ request, or add this host to the TLS skip     │ │
│  │ whitelist in config.                          │ │
│  └───────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────┘
```

### 10.3 Inline Validation

Invalid URL or header values show a red error message directly below the offending field:

```
[GET v] [htp://not-valid-url           ]
         ⚠ Invalid URL: unknown scheme "htp"
```

---

## 11. Notification System

For non-critical messages (persistence errors, info messages), a temporary notification bar appears above the status bar:

```
┌──────────────────────────────────────────────┐
│  ⚠ Failed to save collection: permission     │
│    denied. Operating with in-memory data.    │
└──────────────────────────────────────────────┘
│ [Tab] Navigate  [Enter] Select  [Ctrl+S] Send │
```

Notifications auto-dismiss after 5 seconds or can be dismissed with any keypress.
