pub const MAX_URL_LENGTH: usize = 2_048;
pub const MAX_KEY_LENGTH: usize = 256;
pub const MAX_VALUE_LENGTH: usize = 4_096;
pub const MAX_QUERY_PARAM_ROWS: usize = 128;
pub const MAX_HEADER_ROWS: usize = 128;

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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub sync_guard: Option<SyncDirection>,
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
            sync_guard: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub terminal_size: (u16, u16),
    pub layout_mode: LayoutMode,
    pub should_quit: bool,
    pub request: RequestState,
}

impl AppState {
    pub fn new(terminal_size: (u16, u16), layout_mode: LayoutMode) -> Self {
        Self {
            terminal_size,
            layout_mode,
            should_quit: false,
            request: RequestState::default(),
        }
    }
}
