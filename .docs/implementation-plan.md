# posterm — Implementation Plan

> **Version:** 0.1.0
> **Last Updated:** 2026-03-03
> **Status:** APPROVED

---

## Overview

The implementation is divided into **8 phases**, each producing a working, testable increment. Phases are ordered by dependency — each phase builds on the previous one. Total estimated duration: ~4-5 weeks for a single developer.

---

## Phase 1: Foundation — TUI Shell & Event Loop

**Duration:** 3 days
**Objective:** Establish the core application skeleton with the event loop, terminal setup/teardown, action system, and a minimal rendering frame.

### Deliverables

1. **`main.rs`** — Entry point. Initializes tokio runtime, creates App, runs the event loop.
2. **`tui.rs`** — Terminal wrapper: enters raw mode, enables alternate screen, sets up crossterm backend for ratatui. Provides panic hook to restore terminal on crash.
3. **`event.rs`** — EventHandler struct using tokio::spawn + mpsc channel. Produces `Event::Key`, `Event::Tick`, `Event::Render`, `Event::Resize`.
4. **`action.rs`** — Initial Action enum with: `Tick`, `Render`, `Resize(u16, u16)`, `Quit`, `FocusNext`, `FocusPrev`.
5. **`state.rs`** — AppState with: `terminal_size`, `layout_mode`, `focus`, `should_quit`.
6. **`app.rs`** — App struct owning state, action channel, and event loop. Dispatches events → actions → components → render.
7. **`components/mod.rs`** — Component trait definition.
8. **`components/layout_manager.rs`** — Computes LayoutMode from terminal dimensions. Allocates Rect areas for panes.
9. **`components/status_bar.rs`** — Renders static keybinding hints at the bottom.

### Exit Criteria

- App starts, shows an empty frame with the status bar, and correct layout mode label (Small/Medium/Large).
- Resizing the terminal dynamically switches layout modes.
- `Ctrl+Q` cleanly exits, restoring the terminal.
- No panics on any key input.

### Quality Gates

- `cargo build` compiles with zero warnings.
- `cargo clippy` passes with no warnings.
- Manual test: start, resize, quit on macOS/Linux.

---

## Phase 2: Request Builder — URL, Method, and Headers

**Duration:** 4 days
**Objective:** Build the request composition interface: URL bar with method selector, headers editor, and query params editor with bidirectional URL sync.

### Deliverables

1. **`components/request_builder/mod.rs`** — Container component with tab bar (`Params | Headers | Auth | Body`). Manages active tab state.
2. **`components/request_builder/url_bar.rs`** — HTTP method dropdown + URL text input. Text cursor, insert/delete characters, Home/End.
3. **`components/request_builder/query_params.rs`** — Key-value table component. Add/edit/delete/toggle rows.
4. **`components/request_builder/headers_editor.rs`** — Key-value table component (reuse KeyValueEditor logic via shared helper).
5. **`util/url_parser.rs`** — URL ↔ query param sync: `parse_query_params()`, `rebuild_url_with_params()`.
6. **`state.rs`** updates — Add `RequestState` with method, url, query_params, headers.
7. **`action.rs`** updates — Add request building actions: `SetMethod`, `SetUrl`, `SyncUrlFromParams`, `SyncParamsFromUrl`, `SetHeader`, `AddHeader`, `RemoveHeader`, `SetQueryParam`, `AddQueryParam`, `RemoveQueryParam`.

### Exit Criteria

- User can select HTTP method via dropdown.
- User can type a URL with full text editing.
- User can switch between Params and Headers tabs.
- User can add, edit, enable/disable, and delete query parameters.
- Editing a query parameter updates the URL bar, and editing the URL updates the query params table.
- User can add, edit, and delete headers.
- Tab/Shift-Tab navigates between the URL bar, method selector, and the active tab's editor.
- All input respects Normal/Editing mode.

### Quality Gates

- `cargo clippy` clean.
- Manual test: compose a request with URL, query params, and custom headers. Verify URL ↔ param sync.

---

## Phase 3: Auth Panel & Body Editor

**Duration:** 3 days
**Objective:** Complete the request builder with authentication helpers and body editing.

### Deliverables

1. **`components/request_builder/auth_panel.rs`** — Auth mode selector (None/Bearer/Basic). Token input field. Username + password fields. Computes Authorization header.
2. **`components/request_builder/body_editor.rs`** — Multi-line text area for JSON. Key-value editor for Form-encoded. Format toggle (JSON/Form). Auto-sets Content-Type header.
3. **`state.rs`** updates — Add auth fields and body fields to RequestState.
4. **`action.rs`** updates — Add `SetAuthMode`, `SetAuthToken`, `SetAuthCredentials`, `SetBodyFormat`, `SetBodyContent`.

### Exit Criteria

- User can select Bearer auth, enter a token, and see the Authorization header auto-managed in the headers tab.
- User can select Basic auth, enter username/password, and see the Base64-encoded Authorization header.
- User can write a JSON body in the multi-line editor.
- User can switch to Form format and fill key-value pairs.
- Content-Type header is auto-set when body format changes.
- User can manually override the auto-set Content-Type.

### Quality Gates

- `cargo clippy` clean.
- Manual test: set Bearer auth → verify header. Set Basic auth → verify base64 encoding. Write JSON body → verify Content-Type.

---

## Phase 4: HTTP Execution & Response Display

**Duration:** 5 days
**Objective:** Implement the HTTP client layer and response viewer. This is the critical phase that enables the core end-to-end workflow.

### Deliverables

1. **`http/client.rs`** — HttpClientPool with strict and permissive reqwest::Client instances.
2. **`http/request_builder.rs`** — Converts RequestState → reqwest::Request. Merges headers, auth, body, query params.
3. **`http/response_processor.rs`** — Content type detection. Response metadata extraction.
4. **`http/mod.rs`** — `execute_request()` function: spawns tokio task, streams response chunks, sends actions.
5. **`util/streaming_buffer.rs`** — StreamingBuffer with chunk append, line index, get_lines() for viewport.
6. **`components/response_viewer/mod.rs`** — Container with tab bar (`Body | Headers | Raw | Search`).
7. **`components/response_viewer/metadata_bar.rs`** — Status code (color-coded), elapsed time, body size.
8. **`components/response_viewer/response_body.rs`** — Formatted body view with scrolling. Virtualized rendering from StreamingBuffer.
9. **`components/response_viewer/raw_view.rs`** — Raw unprocessed body text with scrolling.
10. **`components/response_viewer/response_headers.rs`** — Read-only header table.
11. **`state.rs`** updates — Add ResponseState, InFlightRequest, StreamingBuffer.
12. **`action.rs`** updates — Add `SendRequest`, `CancelRequest`, `RequestStarted`, `RequestCompleted`, `RequestFailed`, `RequestCancelled`, `ResponseChunk`, `ScrollResponse`, `SetResponseTab`.

### Exit Criteria

- User can compose a request and press Ctrl+S to send it.
- Loading indicator shows while request is in-flight.
- UI remains interactive during the request.
- User can cancel an in-flight request with Ctrl+C.
- Response displays: status code (color-coded), elapsed time, body size.
- Response body displays formatted (plain text, no syntax highlighting yet).
- User can switch between Body, Headers, and Raw tabs.
- Response headers display correctly.
- Raw view shows unprocessed response.
- Network errors display gracefully in the response pane.
- TLS errors display with a hint about toggling TLS verification.
- Per-request timeout and redirect toggles work.

### Quality Gates

- `cargo clippy` clean.
- Manual test: Send GET to httpbin.org/get → see response. Send POST with JSON body → verify echo. Test timeout with httpbin.org/delay/30. Test TLS skip with a self-signed local server. Test cancel mid-flight.

---

## Phase 5: Syntax Highlighting & Response Search

**Duration:** 3 days
**Objective:** Add syntax highlighting to the response body and implement in-response text search.

### Deliverables

1. **`highlight/mod.rs`** — `highlight_lines()` function using syntect. Maps content type to syntax grammar.
2. **`highlight/theme.rs`** — Terminal-native ANSI color mapping for syntect scopes.
3. **`components/response_viewer/response_body.rs`** updates — Integrate highlighting into the render pipeline. Lazy per-viewport highlighting with cache.
4. **`components/response_viewer/search_bar.rs`** — Search input field. Incremental search. Match highlighting in body/raw view. Next/prev match navigation.
5. **`state.rs`** updates — Add SearchState to ResponseState.
6. **`action.rs`** updates — Add `SearchInResponse`, `NextSearchMatch`, `PrevSearchMatch`.

### Exit Criteria

- JSON responses are syntax-highlighted with proper colors (strings green, numbers cyan, keys blue, etc.).
- XML and HTML responses are highlighted.
- Highlighting works correctly with terminal's native color scheme.
- Large responses (1MB+) highlight smoothly — only visible lines are processed.
- User can press `/` to open search bar in the response body.
- Search highlights all matches in the viewport.
- User can navigate between matches with `n` (next) and `N` (prev).
- Current match is distinctly highlighted vs other matches.
- Esc closes the search.

### Quality Gates

- `cargo clippy` clean.
- Manual test: GET a large JSON API response → verify highlighting and smooth scrolling. Search for a term → verify match highlighting and navigation.

---

## Phase 6: Collections & History Persistence

**Duration:** 4 days
**Objective:** Implement the persistence layer and sidebar UI for collections and history.

### Deliverables

1. **`persistence/atomic_write.rs`** — Atomic write utility (write to .tmp, rename).
2. **`persistence/config.rs`** — Load/save `config.toml`. Create defaults on first run.
3. **`persistence/collections.rs`** — Load/save collection TOML files. One file per collection.
4. **`persistence/history.rs`** — Load/save `history.toml`. Enforce history limit with FIFO eviction.
5. **`persistence/mod.rs`** — PersistenceManager with debounced saves.
6. **`components/sidebar/mod.rs`** — Sidebar container with collections section and history section.
7. **`components/sidebar/collection_tree.rs`** — Expandable tree UI. Create/rename/delete collections. Add/rename/reorder/delete requests.
8. **`components/sidebar/history_list.rs`** — History list UI. Select to reload into builder. Clear history.
9. **`components/overlay_manager.rs`** — Renders sidebar as overlay in small/medium layout modes.
10. **`state.rs`** updates — Add `collections`, `history`, `config`, `sidebar_visible`.
11. **`action.rs`** updates — Add all collection, history, and persistence actions.

### Exit Criteria

- On first launch, `config.toml` is created with defaults at the platform-appropriate path.
- User can create a new collection and see it in the sidebar.
- User can save the current request to a collection.
- User can select a saved request to load it into the builder.
- User can rename and delete collections and requests.
- Collections persist across app restarts.
- Every sent request is automatically recorded in history.
- History displays in reverse-chronological order with method, URL, and status code.
- Selecting a history entry reloads it into the builder.
- History respects the configured limit; oldest entries are evicted.
- History persists across restarts.
- Sidebar shows correctly in large mode (persistent panel).
- Sidebar shows as overlay in medium/small mode (Ctrl+B toggle).
- Sensitive headers (Authorization, Cookie) are NOT persisted in history by default.

### Quality Gates

- `cargo clippy` clean.
- Manual test: Create collection → save requests → restart app → verify data survives. Send 5+ requests → verify history. Resize terminal → verify sidebar overlay behavior.

---

## Phase 7: Concurrent Requests & In-Flight Management

**Duration:** 2 days
**Objective:** Support multiple concurrent in-flight requests with proper state isolation and cancellation.

### Deliverables

1. **`state.rs`** updates — `in_flight: HashMap<RequestId, InFlightRequest>` with cancel tokens.
2. **`http/mod.rs`** updates — Support multiple concurrent spawned tasks, each with its own RequestId.
3. **`components/response_viewer/metadata_bar.rs`** updates — Show which request's response is currently displayed.
4. **UI** updates — When a request completes, if the user has navigated to a different request in the sidebar, the response is stored but not auto-displayed (the user can switch to it).
5. **Cancellation** — Ctrl+C cancels only the currently-visible in-flight request, not all of them.

### Exit Criteria

- User can send a request, switch to a different collection entry, and send another request while the first is still in-flight.
- Both requests complete independently.
- Cancelling only cancels the actively-viewed request.
- In-flight indicator shows per-request status.
- No race conditions or panics with concurrent requests.

### Quality Gates

- `cargo clippy` clean.
- Manual test: Send request to httpbin.org/delay/10, send another to httpbin.org/get, verify both complete. Cancel one, verify the other completes.

---

## Phase 8: Polish, Edge Cases & Cross-Platform

**Duration:** 3 days
**Objective:** Handle edge cases, improve UX polish, and verify cross-platform compatibility.

### Deliverables

1. **Inline validation** — Red error messages for invalid URLs, malformed headers.
2. **Notification system** — Temporary notification bar for persistence errors and info messages. Auto-dismiss after 5 seconds.
3. **Help overlay** — `?` keybinding shows a full keybinding reference overlay.
4. **Empty states** — Friendly messages when collections are empty, no history exists, no response yet.
5. **Large response handling** — Verify virtualized rendering works smoothly for 10MB+ responses.
6. **Terminal edge cases** — Very small terminals (< 40 cols), rapid resize, alternate screen restore.
7. **Cross-platform testing** — Verify on macOS, Linux, and Windows. Ensure `dirs` paths resolve correctly on each.
8. **Config file permissions** — Set 0600 on config and collection files (Unix). No-op on Windows.
9. **Graceful degradation** — If syntect fails to load a grammar, fall back to plain text. If disk is read-only, operate in-memory with a warning.
10. **Startup performance** — Profile and optimize to meet the 200ms startup target. Lazy-load syntect grammars.

### Exit Criteria

- All 15 acceptance criteria from the requirements document are met.
- No panics on any known edge case: empty URL, huge response, corrupt TOML file, no network, terminal resize during request, rapid key mashing.
- App starts in < 200ms.
- All keybindings documented in the help overlay.
- Works on macOS, Linux, and Windows.

### Quality Gates

- `cargo clippy` clean.
- `cargo test` passes (unit tests for URL parser, base64 encoding, content type detection, StreamingBuffer, TOML serialization).
- Manual end-to-end test against httpbin.org.
- Cross-platform build verification.

---

## Phase Dependency Graph

```
Phase 1: Foundation
    │
    ▼
Phase 2: Request Builder (URL, Method, Headers, Params)
    │
    ├──────────────────┐
    ▼                  ▼
Phase 3: Auth+Body   Phase 4: HTTP Execution + Response Display
    │                  │
    └────────┬─────────┘
             ▼
Phase 5: Syntax Highlighting + Search
             │
             ▼
Phase 6: Collections + History + Persistence
             │
             ▼
Phase 7: Concurrent Requests
             │
             ▼
Phase 8: Polish + Edge Cases + Cross-Platform
```

**Critical Path:** Phases 1 → 2 → 4 → 5 → 6 → 8

Phase 3 (Auth+Body) can be developed in parallel with Phase 4 (HTTP Execution) after Phase 2 is complete.

---

## Risk Register

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| syntect startup is slow (grammar loading) | Medium | Medium | Lazy-load grammars on first highlight request, not at startup. Use `SyntaxSet::load_defaults_nonewlines()` which is faster. |
| StreamingBuffer memory overhead for 50MB responses | Medium | High | Implement chunk eviction: only keep chunks within a window around the viewport. Re-fetch from response cache if user scrolls back. |
| crossterm rendering inconsistencies across terminals | Low | Medium | Test on popular terminals (iTerm2, Alacritty, Windows Terminal, GNOME Terminal). Use only widely-supported ANSI sequences. |
| TOML file corruption on crash during write | Low | High | Atomic writes (write to .tmp, rename) prevent corruption. On load, if TOML parsing fails, back up the corrupt file and start fresh with a warning. |
| reqwest TLS configuration complexity | Low | Low | Two pre-built clients (strict + permissive) avoid per-request Client construction overhead. |
| Bidirectional URL ↔ param sync creates infinite loops | Medium | Medium | Use a sync guard flag: while syncing in one direction, suppress the other direction's sync trigger. |

---

## Testing Strategy

### Unit Tests (per-phase, embedded in modules)

| Module | Test Cases |
|---|---|
| `util/url_parser.rs` | Parse params from URL with special chars, empty params, no query string. Rebuild URL from params. Round-trip sync. |
| `util/streaming_buffer.rs` | Append chunks, verify line count and line content. Edge cases: empty chunks, chunks splitting mid-line, very long lines. |
| `http/request_builder.rs` | Build request from various RequestState configurations. Verify headers, body, auth, method, URL. |
| `http/response_processor.rs` | Content type detection for JSON, XML, HTML, unknown. Heuristic detection from body. |
| `persistence/config.rs` | Serialize/deserialize AppConfig to/from TOML. Default creation. |
| `persistence/collections.rs` | Serialize/deserialize Collection to/from TOML. |
| `persistence/history.rs` | Serialize/deserialize history. FIFO eviction at limit. Sensitive header redaction. |
| `highlight/theme.rs` | Verify ANSI color mapping produces valid ratatui Styles. |
| `state.rs` | RequestState default values. Auth header computation (Bearer, Basic base64). |

### Integration Tests

| Test | Description |
|---|---|
| End-to-end send | Compose request programmatically → execute → verify response state matches expected. |
| Persistence round-trip | Create AppState → save to disk → load from disk → verify equality. |
| URL sync round-trip | Set URL with params → parse → modify param → rebuild → verify URL. |

### Manual Test Checklist (Phase 8)

- [ ] Start app → verify layout for current terminal size
- [ ] Resize terminal through all three layout modes
- [ ] Compose GET request with URL + query params
- [ ] Verify URL ↔ param sync in both directions
- [ ] Add custom headers
- [ ] Set Bearer auth → verify Authorization header
- [ ] Set Basic auth → verify base64 encoding
- [ ] Write JSON body → verify Content-Type auto-set
- [ ] Send request → verify loading indicator
- [ ] View formatted response body with syntax highlighting
- [ ] View raw response body
- [ ] View response headers
- [ ] Search in response body → navigate matches
- [ ] Cancel an in-flight request
- [ ] Send concurrent requests → verify isolation
- [ ] Create collection → save request → verify persistence
- [ ] Load request from collection
- [ ] View and load from history
- [ ] Toggle TLS verification → test with self-signed server
- [ ] Test timeout with delayed endpoint
- [ ] Test with very large response (10MB+)
- [ ] Test with malformed URL → verify inline error
- [ ] Test help overlay (?)
- [ ] Quit and restart → verify all data persists
- [ ] Test on macOS, Linux, Windows
