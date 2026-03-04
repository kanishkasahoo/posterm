use crate::persistence::HistoryEntry;
use crate::state::{AuthMode, BodyFormat, HttpMethod, KeyValueRow, ResponseMetadata, ResponseTab};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyContent {
    Json(String),
    SetFormRow { index: usize, row: KeyValueRow },
    AddFormRow,
    RemoveFormRow(usize),
}

// The new collection/history/sidebar/persistence variants are part of the Phase 6 API
// and will be dispatched by future UI components. Allow dead_code on the enum level.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Tick,
    Render,
    Resize(u16, u16),
    Quit,
    FocusNext,
    FocusPrev,
    SetMethod(HttpMethod),
    SetUrl(String),
    SyncUrlFromParams,
    SyncParamsFromUrl,
    SetHeader {
        index: usize,
        row: KeyValueRow,
    },
    AddHeader,
    RemoveHeader(usize),
    SetQueryParam {
        index: usize,
        row: KeyValueRow,
    },
    AddQueryParam,
    RemoveQueryParam(usize),
    SetAuthMode(AuthMode),
    SetAuthToken(String),
    SetAuthCredentials {
        username: String,
        password: String,
    },
    SetBodyFormat(BodyFormat),
    SetBodyContent(BodyContent),
    SendRequest,
    CancelRequest,
    RequestStarted {
        request_id: u64,
        method: HttpMethod,
        url: String,
    },
    RequestCompleted {
        request_id: u64,
        metadata: ResponseMetadata,
    },
    RequestFailed {
        request_id: u64,
        error: String,
    },
    RequestCancelled {
        request_id: u64,
    },
    ResponseChunk {
        request_id: u64,
        chunk: Vec<u8>,
    },
    ScrollResponse(i16),
    ScrollResponseHorizontal(i16),
    ToggleResponseWrap,
    SetResponseTab(ResponseTab),
    OpenResponseSearch,
    CloseResponseSearch,
    SearchInResponse(String),
    NextSearchMatch,
    PrevSearchMatch,
    ToggleHelp,
    CloseHelp,
    ScrollHelp(i16),
    ToggleSmallModePane,

    // ── Collections ──────────────────────────────────────────────────────────
    CreateCollection {
        name: String,
    },
    RenameCollection {
        index: usize,
        name: String,
    },
    DeleteCollection(usize),
    ToggleCollectionExpanded(usize),
    SaveRequestToCollection {
        collection_index: usize,
        name: String,
    },
    RenameCollectionRequest {
        collection: usize,
        request: usize,
        name: String,
    },
    DeleteCollectionRequest {
        collection: usize,
        request: usize,
    },
    LoadCollectionRequest {
        collection: usize,
        request: usize,
    },

    // ── History ───────────────────────────────────────────────────────────────
    RecordHistory(Box<HistoryEntry>),
    LoadFromHistory(usize),
    ClearHistory,

    // ── Sidebar navigation ────────────────────────────────────────────────────
    ToggleSidebar,
    SidebarFocusNext,
    SidebarFocusPrev,
    SidebarSelect,
    SidebarClose,

    // ── Persistence ───────────────────────────────────────────────────────────
    PersistenceError(String),

    // ── Notifications ─────────────────────────────────────────────────────────
    ShowNotification {
        message: String,
        kind: crate::state::NotificationKind,
    },
    DismissNotification,
}
