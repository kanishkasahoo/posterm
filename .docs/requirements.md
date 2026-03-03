# posterm — Requirements Specification

> **Version:** 0.1.0
> **Last Updated:** 2026-03-03
> **Status:** APPROVED

---

## 1. Project Overview

**posterm** is a terminal-based (TUI) API client for crafting, sending, and inspecting HTTP requests — a keyboard-driven alternative to Postman that runs entirely inside a terminal emulator. It targets developers, SREs, and anyone who tests or debugs HTTP APIs.

### 1.1 Goals

- Provide a minimal yet powerful API testing experience within the terminal.
- Offer full request customization: method, URL, headers, query params, body.
- Display responses in both raw and formatted (syntax-highlighted) views.
- Persist requests into named collections and automatic history.
- Remain fast, lightweight, and respectful of terminal conventions.

### 1.2 Non-Goals (v1)

- Environment variables / variable substitution system.
- Import/export from Postman, Insomnia, or cURL formats.
- OAuth2 or other advanced auth flows.
- Scripting, pre-request hooks, or test assertions.
- Proxy configuration UI.

---

## 2. User Personas

| Persona | Description | Key Needs |
|---|---|---|
| Backend Developer | Builds and tests REST APIs daily | Fast request iteration, body editing, response inspection |
| Full-Stack Developer | Works across frontend and backend | Quick header customization, multiple concurrent requests |
| DevOps / SRE | Monitors and debugs production APIs | TLS toggle for internal services, response metadata, raw view |

---

## 3. Functional Requirements

### 3.1 Request Builder

| ID | Requirement |
|---|---|
| FR-REQ-001 | The system SHALL support the following HTTP methods: GET, POST, PUT, PATCH, DELETE. |
| FR-REQ-002 | The system SHALL provide a URL input field that accepts any valid HTTP or HTTPS URL. |
| FR-REQ-003 | The system SHALL provide a dedicated query-parameter editor with key-value pair rows. |
| FR-REQ-004 | WHEN a user edits query parameters in the dedicated editor, the system SHALL synchronize the changes into the URL bar bidirectionally. |
| FR-REQ-005 | WHEN a user edits query parameters directly in the URL bar, the system SHALL synchronize the changes into the dedicated editor bidirectionally. |
| FR-REQ-006 | The system SHALL provide a headers editor with key-value pair rows for adding, editing, and removing custom request headers. |
| FR-REQ-007 | The system SHALL support request bodies in JSON format (`application/json`). |
| FR-REQ-008 | The system SHALL support request bodies in URL-encoded form format (`application/x-www-form-urlencoded`). |
| FR-REQ-009 | WHEN a user selects a body format, the system SHALL automatically set the `Content-Type` header to the corresponding MIME type. |
| FR-REQ-010 | The system SHALL allow the user to manually override the auto-set `Content-Type` header. |

### 3.2 Authentication Helpers

| ID | Requirement |
|---|---|
| FR-AUTH-001 | The system SHALL provide a Bearer Token authentication helper that accepts a token string and automatically sets the `Authorization: Bearer <token>` header. |
| FR-AUTH-002 | The system SHALL provide a Basic Authentication helper that accepts a username and password and automatically sets the `Authorization: Basic <base64>` header. |
| FR-AUTH-003 | WHEN an auth helper is active, the system SHALL visually indicate which auth method is in use on the request builder. |
| FR-AUTH-004 | The system SHALL allow the user to disable the auth helper and manage the `Authorization` header manually. |

### 3.3 Request Execution

| ID | Requirement |
|---|---|
| FR-EXEC-001 | WHEN the user triggers "Send", the system SHALL dispatch the HTTP request asynchronously without blocking the UI. |
| FR-EXEC-002 | WHILE a request is in-flight, the system SHALL display a loading indicator in the response pane. |
| FR-EXEC-003 | WHILE a request is in-flight, the system SHALL allow the user to cancel the request. |
| FR-EXEC-004 | The system SHALL support multiple concurrent in-flight requests originating from different collection entries or tabs. |
| FR-EXEC-005 | The system SHALL support a global default request timeout that is user-configurable. |
| FR-EXEC-006 | The system SHALL allow per-request timeout overrides that take precedence over the global default. |
| FR-EXEC-007 | The system SHALL provide a toggleable option to follow HTTP redirects (3xx) automatically. |
| FR-EXEC-008 | WHEN redirect-following is enabled, the system SHALL follow redirects and display the final response. |
| FR-EXEC-009 | WHEN redirect-following is disabled, the system SHALL display the raw 3xx response including its headers. |

### 3.4 TLS / Certificate Handling

| ID | Requirement |
|---|---|
| FR-TLS-001 | The system SHALL enforce valid TLS certificate verification by default. |
| FR-TLS-002 | The system SHALL provide a per-request toggle to skip TLS certificate verification. |
| FR-TLS-003 | The system SHALL provide a global configuration option to whitelist IP addresses or CIDR ranges (e.g., `127.0.0.0/8`, `10.0.0.0/8`, `192.168.0.0/16`) that bypass TLS verification automatically. |

### 3.5 Response Viewer

| ID | Requirement |
|---|---|
| FR-RESP-001 | The system SHALL display the response body in a **formatted/pretty view** with syntax highlighting for JSON, XML, and HTML content types. |
| FR-RESP-002 | The system SHALL display the response body in a **raw view** showing exact bytes as received with no processing. |
| FR-RESP-003 | The system SHALL allow the user to toggle between formatted and raw views. |
| FR-RESP-004 | The system SHALL display all response headers in a dedicated panel. |
| FR-RESP-005 | The system SHALL display response metadata: HTTP status code, status text, response time (ms), and body size (bytes). |
| FR-RESP-006 | The system SHALL provide text search within the response body with match highlighting and navigation between matches. |
| FR-RESP-007 | WHEN a response body exceeds the visible viewport, the system SHALL use streaming/virtualized rendering — only buffering and rendering content visible in the current viewport. |
| FR-RESP-008 | The system SHALL NOT load an entire large response body into a single in-memory string for rendering purposes. |

### 3.6 Collections

| ID | Requirement |
|---|---|
| FR-COLL-001 | The system SHALL support named collections that group related requests into folders. |
| FR-COLL-002 | The system SHALL allow the user to create, rename, and delete collections. |
| FR-COLL-003 | The system SHALL allow the user to add, rename, reorder, and remove individual requests within a collection. |
| FR-COLL-004 | The system SHALL persist collections to disk in TOML format. |
| FR-COLL-005 | The system SHALL load persisted collections on startup. |
| FR-COLL-006 | WHEN the user modifies a collection (add/edit/delete request), the system SHALL auto-save the change to disk. |

### 3.7 Request History

| ID | Requirement |
|---|---|
| FR-HIST-001 | WHEN a request is sent, the system SHALL automatically record it in the request history with timestamp, method, URL, and status code. |
| FR-HIST-002 | The system SHALL display request history in reverse-chronological order. |
| FR-HIST-003 | The system SHALL allow the user to select a historical request and re-load it into the request builder. |
| FR-HIST-004 | The system SHALL enforce a configurable maximum history size (number of entries). |
| FR-HIST-005 | WHEN the history exceeds the configured maximum, the system SHALL evict the oldest entries first (FIFO). |
| FR-HIST-006 | The system SHALL persist request history to disk in TOML format. |
| FR-HIST-007 | The system SHALL allow the user to clear all history. |

### 3.8 Configuration

| ID | Requirement |
|---|---|
| FR-CONF-001 | The system SHALL store global configuration in a TOML file at a platform-appropriate config directory (e.g., `~/.config/posterm/config.toml`). |
| FR-CONF-002 | The system SHALL expose the following configurable settings: default timeout (seconds), history limit (count), TLS-skip IP whitelist, and redirect-follow default (boolean). |
| FR-CONF-003 | WHEN the config file does not exist on first launch, the system SHALL create it with sensible defaults. |

---

## 4. Non-Functional Requirements

### 4.1 Performance

| ID | Requirement |
|---|---|
| NFR-PERF-001 | The system SHALL start up and render the initial UI within 200ms on a modern machine. |
| NFR-PERF-002 | The system SHALL maintain 60fps rendering during normal UI interaction (scrolling, typing, tab switching). |
| NFR-PERF-003 | The system SHALL handle response bodies up to 50MB without crashing or significant UI degradation, using virtualized rendering. |

### 4.2 Reliability

| ID | Requirement |
|---|---|
| NFR-REL-001 | The system SHALL NOT crash on network errors, DNS failures, TLS errors, or malformed responses. All errors SHALL be displayed gracefully in the response pane. |
| NFR-REL-002 | The system SHALL NOT corrupt persisted data (collections, history, config) on unexpected termination. |

### 4.3 Usability

| ID | Requirement |
|---|---|
| NFR-USE-001 | The system SHALL be fully operable via keyboard using standard terminal keybindings (Arrow keys, Tab/Shift-Tab, Enter, Esc). |
| NFR-USE-002 | The system SHALL display a persistent keybinding help bar or footer showing context-sensitive shortcuts. |
| NFR-USE-003 | The system SHALL use the terminal's native color palette (no hardcoded RGB values) to respect the user's existing theme. |

### 4.4 Portability

| ID | Requirement |
|---|---|
| NFR-PORT-001 | The system SHALL compile and run on macOS, Linux, and Windows. |
| NFR-PORT-002 | The system SHALL use platform-appropriate directories for config and data storage (XDG on Linux, ~/Library on macOS, %APPDATA% on Windows). |

### 4.5 Security

| ID | Requirement |
|---|---|
| NFR-SEC-001 | The system SHALL NOT log or persist sensitive header values (Authorization, Cookie) in request history by default. |
| NFR-SEC-002 | The system SHALL provide an opt-in configuration to include sensitive headers in history. |

---

## 5. UI Layout Specification

### 5.1 Three-Pane Layout

```
+--[ Collections/History ]--+--[ Request Builder ]---------------------+
|                            |                                          |
|  > My API                  |  [GET v] [https://api.example.com/users ]|
|    GET /users              |                                          |
|    POST /users             |  [Params] [Headers] [Auth] [Body]       |
|    GET /users/:id          |  +--------------------------------------+|
|  > Another API             |  | key        | value                   ||
|    ...                     |  | page       | 1                       ||
|                            |  | limit      | 20                      ||
|  --- History ---           |  +--------------------------------------+|
|  12:30 GET /users  200     |                                          |
|  12:28 POST /users 201     +--[ Response ]----------------------------+
|  12:25 GET /health 200     |  200 OK  |  342ms  |  1.2 KB            |
|                            |  [Body] [Headers]  [Raw] [Search]       |
|                            |  +--------------------------------------+|
|                            |  | {                                    ||
|                            |  |   "users": [                         ||
|                            |  |     { "id": 1, "name": "Alice" }     ||
|                            |  |   ]                                  ||
|                            |  | }                                    ||
+----------------------------+--+--------------------------------------+|
| [Tab] Navigate  [Enter] Select  [Ctrl+S] Send  [Ctrl+C] Cancel  [?] Help |
+-----------------------------------------------------------------------+
```

### 5.2 Pane Descriptions

| Pane | Position | Purpose |
|---|---|---|
| **Sidebar** | Left, fixed-width (resizable) | Displays collection tree (expandable folders) and request history below. |
| **Request Builder** | Top-right | URL bar, method selector, tabbed sections for params/headers/auth/body. |
| **Response Viewer** | Bottom-right | Response metadata bar, tabbed sections for body/headers/raw/search. |
| **Status Bar** | Bottom, full-width | Context-sensitive keybinding hints. |

### 5.3 Responsive Layout Breakpoints

The UI SHALL adapt its layout based on the terminal's column width and row height. Three layout modes are defined:

#### Small Terminal (width < 80 cols OR height < 24 rows)

```
+--[ Request Builder / Response Viewer ]-------+
|                                               |
|  (Only ONE of request or response visible     |
|   at a time. Toggle with a keybinding.)       |
|                                               |
|  Sidebar: hidden. History/collections         |
|  accessible via overlay (hotkey).             |
|                                               |
+-----------------------------------------------+
| [keybinding hints]                            |
+-----------------------------------------------+
```

- The system SHALL show only the Request Builder OR Response Viewer at a time; the user toggles between them via a keybinding.
- The Sidebar (collections + history) SHALL be hidden and accessible as a full-screen overlay triggered by a keybinding.

#### Medium Terminal (width >= 80 cols AND height >= 24 rows, but width < 120 cols)

```
+--[ Request Builder ]-------------------------+
|  [GET v] [https://api.example.com/users     ]|
|  [Params] [Headers] [Auth] [Body]            |
|  +-------------------------------------------+
|  | key        | value                        |
|  +-------------------------------------------+
+--[ Response Viewer ]-------------------------+
|  200 OK  |  342ms  |  1.2 KB                |
|  [Body] [Headers] [Raw] [Search]             |
|  +-------------------------------------------+
|  | { "users": [...] }                        |
+----------------------------------------------+
| [keybinding hints]                           |
+----------------------------------------------+
```

- The system SHALL display the Request Builder (top) and Response Viewer (bottom) stacked vertically — no sidebar.
- The Sidebar (collections + history) SHALL be overlaid on the Request Builder pane when activated via a keybinding.

#### Large Terminal (width >= 120 cols AND height >= 24 rows)

- The system SHALL display the full three-pane layout as described in Section 5.1: sidebar on the left, request builder top-right, response viewer bottom-right.

#### Responsive Layout Requirements

| ID | Requirement |
|---|---|
| FR-LAYOUT-001 | The system SHALL detect terminal dimensions on startup and select the appropriate layout mode. |
| FR-LAYOUT-002 | WHEN the terminal is resized at runtime, the system SHALL re-evaluate the layout breakpoints and transition to the appropriate layout mode immediately. |
| FR-LAYOUT-003 | WHILE in small layout mode, the system SHALL show only the Request Builder or Response Viewer; the user toggles between them with a keybinding. |
| FR-LAYOUT-004 | WHILE in small or medium layout mode, the system SHALL render the Sidebar as an overlay panel triggered by a keybinding, overlaid on top of the Request Builder area. |
| FR-LAYOUT-005 | WHILE in large layout mode, the system SHALL render the full three-pane layout with the Sidebar persistently visible. |
| FR-LAYOUT-006 | WHEN transitioning between layout modes, the system SHALL preserve all in-progress request state (URL, headers, body, response data). |

---

## 6. Constraints

| Constraint | Detail |
|---|---|
| Language | Rust (edition 2024) |
| TUI Framework | ratatui |
| HTTP Client | reqwest |
| Async Runtime | tokio |
| Config/Persistence Format | TOML |
| Platform Support | macOS, Linux, Windows |

---

## 7. Success Criteria

| Criterion | Metric |
|---|---|
| Core workflow functional | User can compose, send, and inspect an HTTP request end-to-end |
| Persistence works | Collections and history survive app restarts |
| Performance acceptable | Startup < 200ms, UI stays responsive during large responses |
| Error resilience | No panics on network errors, malformed input, or unexpected responses |
| Keyboard navigable | Every action reachable via keyboard without mouse |

---

## 8. Acceptance Criteria Summary

1. User can select HTTP method (GET/POST/PUT/PATCH/DELETE) and enter a URL.
2. User can add/edit/remove query parameters via a dedicated editor; changes sync to the URL bar and vice versa.
3. User can add/edit/remove custom headers.
4. User can set Bearer Token or Basic Auth via helpers; `Authorization` header is auto-managed.
5. User can compose JSON or form-encoded request bodies.
6. User can send request and see a loading indicator; UI remains interactive.
7. User can cancel an in-flight request.
8. Response shows status code, time, size, formatted body, raw body, and headers.
9. User can search within response body.
10. User can create collections, add requests, and see them in the sidebar.
11. Request history is recorded automatically and visible in the sidebar.
12. All data persists across sessions in TOML files.
13. TLS verification is toggleable per-request; local IPs can be globally whitelisted.
14. Redirect following is toggleable.
15. App respects terminal colors and works on macOS, Linux, and Windows.
