use std::collections::HashMap;
use std::fmt;

use crate::persistence::{AppConfig, Collection, HistoryEntry};
use crate::util::streaming_buffer::StreamingBuffer;

// The Info variant is part of the public notification API and will be used by
// future callers dispatching ShowNotification actions.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdaterPhase {
    Idle,
    Checking,
    Downloading,
    UpToDate,
    PendingRestart,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterState {
    pub phase: UpdaterPhase,
    pub status_text: String,
    pub pending_version: Option<String>,
    pub in_progress: bool,
}

impl Default for UpdaterState {
    fn default() -> Self {
        Self {
            phase: UpdaterPhase::Idle,
            status_text: String::from("idle"),
            pending_version: None,
            in_progress: false,
        }
    }
}

pub const MAX_URL_LENGTH: usize = 2_048;
pub const MAX_KEY_LENGTH: usize = 256;
pub const MAX_VALUE_LENGTH: usize = 4_096;
pub const MAX_QUERY_PARAM_ROWS: usize = 128;
pub const MAX_HEADER_ROWS: usize = 128;
pub const MAX_AUTH_TOKEN_LENGTH: usize = MAX_VALUE_LENGTH;
pub const MAX_AUTH_USERNAME_LENGTH: usize = MAX_KEY_LENGTH;
pub const MAX_AUTH_PASSWORD_LENGTH: usize = MAX_VALUE_LENGTH;
pub const MAX_BODY_TEXT_LENGTH: usize = 64_000;
pub const MAX_BODY_FORM_ROWS: usize = 128;
pub const MAX_BUFFERED_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub const ALL: [Self; 7] = [
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Head,
        Self::Options,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL
            .iter()
            .position(|method| *method == self)
            .unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL
            .iter()
            .position(|method| *method == self)
            .unwrap_or(0);
        let next = if idx == 0 {
            Self::ALL.len() - 1
        } else {
            idx - 1
        };
        Self::ALL[next]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFocus {
    Method,
    Url,
    Tabs,
    Editor,
}

impl RequestFocus {
    pub fn next(self) -> Self {
        match self {
            Self::Method => Self::Url,
            Self::Url => Self::Tabs,
            Self::Tabs => Self::Editor,
            Self::Editor => Self::Method,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Method => Self::Editor,
            Self::Url => Self::Method,
            Self::Tabs => Self::Url,
            Self::Editor => Self::Tabs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTab {
    Params,
    Headers,
    Auth,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseTab {
    Body,
    Headers,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSearchScope {
    Body,
    Headers,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line_index: usize,
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    pub active: bool,
    pub query: String,
    pub scope: ResponseSearchScope,
    pub matches: Vec<SearchMatch>,
    pub current_match: Option<usize>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            active: false,
            query: String::new(),
            scope: ResponseSearchScope::Body,
            matches: Vec::new(),
            current_match: None,
        }
    }
}

impl ResponseTab {
    pub const ALL: [Self; 3] = [Self::Body, Self::Headers, Self::Raw];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let next = if idx == 0 {
            Self::ALL.len() - 1
        } else {
            idx - 1
        };
        Self::ALL[next]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Body => "Body",
            Self::Headers => "Headers",
            Self::Raw => "Raw",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    None,
    Bearer,
    Basic,
}

impl AuthMode {
    pub const ALL: [Self; 3] = [Self::None, Self::Bearer, Self::Basic];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0);
        let next = if idx == 0 {
            Self::ALL.len() - 1
        } else {
            idx - 1
        };
        Self::ALL[next]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Bearer => "Bearer",
            Self::Basic => "Basic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthField {
    Mode,
    Token,
    Username,
    Password,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthEditorState {
    pub active_field: AuthField,
    pub token_cursor: usize,
    pub username_cursor: usize,
    pub password_cursor: usize,
}

impl Default for AuthEditorState {
    fn default() -> Self {
        Self {
            active_field: AuthField::Mode,
            token_cursor: 0,
            username_cursor: 0,
            password_cursor: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFormat {
    Json,
    Form,
}

impl BodyFormat {
    pub fn next(self) -> Self {
        match self {
            Self::Json => Self::Form,
            Self::Form => Self::Json,
        }
    }

    pub fn prev(self) -> Self {
        self.next()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Form => "Form",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Form => "application/x-www-form-urlencoded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyField {
    Format,
    Json,
    Form,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyEditorState {
    pub active_field: BodyField,
    pub json_cursor: usize,
    pub json_scroll: usize,
    pub form_editor: KeyValueEditorState,
}

impl Default for BodyEditorState {
    fn default() -> Self {
        Self {
            active_field: BodyField::Format,
            json_cursor: 0,
            json_scroll: 0,
            form_editor: KeyValueEditorState::default(),
        }
    }
}

impl RequestTab {
    pub const ALL: [Self; 4] = [Self::Params, Self::Headers, Self::Auth, Self::Body];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let next = if idx == 0 {
            Self::ALL.len() - 1
        } else {
            idx - 1
        };
        Self::ALL[next]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Params => "Params",
            Self::Headers => "Headers",
            Self::Auth => "Auth",
            Self::Body => "Body",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyValueField {
    Key,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryParamToken {
    KeyValue,
    KeyOnly,
    EmptySegment,
}

impl KeyValueField {
    pub fn toggle(self) -> Self {
        match self {
            Self::Key => Self::Value,
            Self::Value => Self::Key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueRow {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

impl Default for KeyValueRow {
    fn default() -> Self {
        Self {
            enabled: true,
            key: String::new(),
            value: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyValueEditorState {
    pub selected_row: usize,
    pub active_field: KeyValueField,
    pub cursor: usize,
}

impl Default for KeyValueEditorState {
    fn default() -> Self {
        Self {
            selected_row: 0,
            active_field: KeyValueField::Key,
            cursor: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    UrlToParams,
    ParamsToUrl,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RequestState {
    pub method: HttpMethod,
    pub url: String,
    pub url_cursor: usize,
    pub active_tab: RequestTab,
    pub focus: RequestFocus,
    pub query_params: Vec<KeyValueRow>,
    pub query_param_tokens: Vec<QueryParamToken>,
    pub headers: Vec<KeyValueRow>,
    pub query_editor: KeyValueEditorState,
    pub headers_editor: KeyValueEditorState,
    pub auth_mode: AuthMode,
    pub auth_token: String,
    pub auth_username: String,
    pub auth_password: String,
    pub auth_editor: AuthEditorState,
    pub body_format: BodyFormat,
    pub body_json: String,
    pub body_form: Vec<KeyValueRow>,
    pub body_editor: BodyEditorState,
    pub managed_auth_header_index: Option<usize>,
    pub managed_content_type_header_index: Option<usize>,
    pub content_type_manual_override: bool,
    pub sync_guard: Option<SyncDirection>,
    pub url_error: Option<String>,
}

impl fmt::Debug for RequestState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestState")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("url_cursor", &self.url_cursor)
            .field("active_tab", &self.active_tab)
            .field("focus", &self.focus)
            .field("query_params", &self.query_params)
            .field("query_param_tokens", &self.query_param_tokens)
            .field("headers", &self.headers)
            .field("query_editor", &self.query_editor)
            .field("headers_editor", &self.headers_editor)
            .field("auth_mode", &self.auth_mode)
            .field("auth_token", &"<redacted>")
            .field("auth_username", &"<redacted>")
            .field("auth_password", &"<redacted>")
            .field("auth_editor", &self.auth_editor)
            .field("body_format", &self.body_format)
            .field("body_json", &self.body_json)
            .field("body_form", &self.body_form)
            .field("body_editor", &self.body_editor)
            .field("managed_auth_header_index", &self.managed_auth_header_index)
            .field(
                "managed_content_type_header_index",
                &self.managed_content_type_header_index,
            )
            .field(
                "content_type_manual_override",
                &self.content_type_manual_override,
            )
            .field("sync_guard", &self.sync_guard)
            .field("url_error", &self.url_error)
            .finish()
    }
}

impl Default for RequestState {
    fn default() -> Self {
        Self {
            method: HttpMethod::Get,
            url: String::new(),
            url_cursor: 0,
            active_tab: RequestTab::Params,
            focus: RequestFocus::Method,
            query_params: Vec::new(),
            query_param_tokens: Vec::new(),
            headers: Vec::new(),
            query_editor: KeyValueEditorState::default(),
            headers_editor: KeyValueEditorState::default(),
            auth_mode: AuthMode::None,
            auth_token: String::new(),
            auth_username: String::new(),
            auth_password: String::new(),
            auth_editor: AuthEditorState::default(),
            body_format: BodyFormat::Json,
            body_json: String::new(),
            body_form: Vec::new(),
            body_editor: BodyEditorState::default(),
            managed_auth_header_index: None,
            managed_content_type_header_index: None,
            content_type_manual_override: false,
            sync_guard: None,
            url_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlightRequest {
    pub id: u64,
    pub method: HttpMethod,
    pub url: String,
    pub cancellation_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponseMetadata {
    pub status_code: Option<u16>,
    pub reason_phrase: Option<String>,
    pub http_version: String,
    pub content_type: Option<String>,
    pub charset: Option<String>,
    pub is_textual: bool,
    pub content_length: Option<u64>,
    pub headers: Vec<(String, String)>,
    pub total_bytes: usize,
    pub duration_ms: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseState {
    pub active_tab: ResponseTab,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub wrap_lines: bool,
    pub buffer: StreamingBuffer,
    pub metadata: Option<ResponseMetadata>,
    pub in_flight: Option<InFlightRequest>,
    pub last_error: Option<String>,
    pub last_request_id: Option<u64>,
    pub cancelled: bool,
    pub truncated: bool,
    pub search: SearchState,
}

impl Default for ResponseState {
    fn default() -> Self {
        Self {
            active_tab: ResponseTab::Body,
            scroll_offset: 0,
            horizontal_scroll_offset: 0,
            wrap_lines: true,
            buffer: StreamingBuffer::new(MAX_BUFFERED_RESPONSE_BYTES),
            metadata: None,
            in_flight: None,
            last_error: None,
            last_request_id: None,
            cancelled: false,
            truncated: false,
            search: SearchState::default(),
        }
    }
}

/// Identifies which item is currently selected in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarItem {
    /// Nothing selected.
    None,
    /// A collection row (index into `AppState.collections`).
    Collection(usize),
    /// A request inside a collection.
    Request { collection: usize, request: usize },
    /// A history entry (index into `AppState.history`).
    HistoryEntry(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarPromptMode {
    CreateCollection,
    RenameCollection { index: usize },
    SaveRequestToCollection { collection_index: usize },
    RenameCollectionRequest { collection: usize, request: usize },
}

impl SidebarPromptMode {
    pub fn title(&self) -> &'static str {
        match self {
            Self::CreateCollection => "New collection",
            Self::RenameCollection { .. } => "Rename collection",
            Self::SaveRequestToCollection { .. } => "Save request as",
            Self::RenameCollectionRequest { .. } => "Rename saved request",
        }
    }

    pub fn cancel_label(&self) -> &'static str {
        match self {
            Self::CreateCollection => "Create collection",
            Self::RenameCollection { .. } => "Rename collection",
            Self::SaveRequestToCollection { .. } => "Save request",
            Self::RenameCollectionRequest { .. } => "Rename request",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarPromptState {
    pub mode: SidebarPromptMode,
    pub value: String,
}

#[derive(Clone)]
pub struct AppState {
    pub terminal_size: (u16, u16),
    pub layout_mode: LayoutMode,
    pub should_quit: bool,
    pub help_visible: bool,
    /// Scroll offset for the help modal (in lines).
    pub help_scroll: usize,
    pub request: RequestState,
    /// Display state for the *active* response context.
    pub response: ResponseState,
    /// Loaded collections.
    pub collections: Vec<Collection>,
    /// Request history, newest entry first.
    pub history: Vec<HistoryEntry>,
    /// Application configuration.
    pub config: AppConfig,
    /// Whether the sidebar is currently visible (relevant for Small/Medium modes).
    pub sidebar_visible: bool,
    /// Whether keyboard focus is inside the sidebar.
    pub sidebar_focused: bool,
    /// In Small layout mode, whether the response viewer (true) or request builder (false) is shown.
    pub small_mode_show_response: bool,
    /// Which item is highlighted in the sidebar.
    pub sidebar_selected_item: SidebarItem,
    /// Horizontal scroll offset for the collections tree labels.
    pub sidebar_collections_horizontal_offset: usize,
    /// Horizontal scroll offset for history list labels.
    pub sidebar_history_horizontal_offset: usize,
    /// Inline prompt for collection management actions.
    pub sidebar_prompt: Option<SidebarPromptState>,
    /// All currently in-flight requests across all contexts (authoritative source).
    pub in_flight_requests: HashMap<u64, InFlightRequest>,
    /// Active notification message and kind (if any).
    pub notification: Option<(String, NotificationKind)>,
    /// Remaining ticks before the notification auto-dismisses.
    pub notification_ticks_remaining: u8,
    /// Self-update lifecycle state.
    pub updater: UpdaterState,
}

impl PartialEq for AppState {
    fn eq(&self, other: &Self) -> bool {
        self.terminal_size == other.terminal_size
            && self.layout_mode == other.layout_mode
            && self.should_quit == other.should_quit
            && self.help_visible == other.help_visible
            && self.help_scroll == other.help_scroll
            && self.request == other.request
            && self.response == other.response
            && self.collections == other.collections
            && self.history == other.history
            && self.config == other.config
            && self.sidebar_visible == other.sidebar_visible
            && self.sidebar_focused == other.sidebar_focused
            && self.small_mode_show_response == other.small_mode_show_response
            && self.sidebar_selected_item == other.sidebar_selected_item
            && self.sidebar_collections_horizontal_offset
                == other.sidebar_collections_horizontal_offset
            && self.sidebar_history_horizontal_offset == other.sidebar_history_horizontal_offset
            && self.sidebar_prompt == other.sidebar_prompt
            && self.in_flight_count() == other.in_flight_count()
            && self.notification == other.notification
            && self.notification_ticks_remaining == other.notification_ticks_remaining
            && self.updater == other.updater
    }
}

impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState")
            .field("terminal_size", &self.terminal_size)
            .field("layout_mode", &self.layout_mode)
            .field("should_quit", &self.should_quit)
            .field("help_visible", &self.help_visible)
            .field("help_scroll", &self.help_scroll)
            .field("request", &self.request)
            .field("response", &self.response)
            .field("collections_count", &self.collections.len())
            .field("history_count", &self.history.len())
            .field("sidebar_visible", &self.sidebar_visible)
            .field("sidebar_focused", &self.sidebar_focused)
            .field("small_mode_show_response", &self.small_mode_show_response)
            .field(
                "sidebar_collections_horizontal_offset",
                &self.sidebar_collections_horizontal_offset,
            )
            .field(
                "sidebar_history_horizontal_offset",
                &self.sidebar_history_horizontal_offset,
            )
            .field("sidebar_prompt", &self.sidebar_prompt)
            .field("in_flight_count", &self.in_flight_count())
            .field("notification", &self.notification)
            .field(
                "notification_ticks_remaining",
                &self.notification_ticks_remaining,
            )
            .field("updater", &self.updater)
            .finish()
    }
}

impl AppState {
    pub fn new(terminal_size: (u16, u16), layout_mode: LayoutMode) -> Self {
        Self {
            terminal_size,
            layout_mode,
            should_quit: false,
            help_visible: false,
            help_scroll: 0,
            request: RequestState::default(),
            response: ResponseState::default(),
            collections: Vec::new(),
            history: Vec::new(),
            config: AppConfig::default(),
            sidebar_visible: false,
            sidebar_focused: false,
            small_mode_show_response: false,
            sidebar_selected_item: SidebarItem::None,
            sidebar_collections_horizontal_offset: 0,
            sidebar_history_horizontal_offset: 0,
            sidebar_prompt: None,
            in_flight_requests: HashMap::new(),
            notification: None,
            notification_ticks_remaining: 0,
            updater: UpdaterState::default(),
        }
    }

    /// Returns the `InFlightRequest` for the currently-displayed response context, if any.
    #[allow(dead_code)]
    pub fn active_in_flight(&self) -> Option<&InFlightRequest> {
        self.response.in_flight.as_ref()
    }

    /// How many requests are currently in flight (across all contexts)?
    pub fn in_flight_count(&self) -> usize {
        self.in_flight_requests.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, LayoutMode};

    #[test]
    fn request_debug_redacts_sensitive_auth_fields() {
        let mut state = AppState::new((120, 40), LayoutMode::Large);
        state.request.auth_token = String::from("token");
        state.request.auth_username = String::from("alice");
        state.request.auth_password = String::from("secret");

        let debug = format!("{:?}", state.request);
        assert!(debug.contains("auth_token: \"<redacted>\""));
        assert!(debug.contains("auth_username: \"<redacted>\""));
        assert!(debug.contains("auth_password: \"<redacted>\""));
        assert!(!debug.contains("auth_token: \"token\""));
        assert!(!debug.contains("auth_username: \"alice\""));
        assert!(!debug.contains("auth_password: \"secret\""));
    }
}
