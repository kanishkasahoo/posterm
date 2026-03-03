use crate::state::{HttpMethod, KeyValueRow};

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
    SetHeader { index: usize, row: KeyValueRow },
    AddHeader,
    RemoveHeader(usize),
    SetQueryParam { index: usize, row: KeyValueRow },
    AddQueryParam,
    RemoveQueryParam(usize),
}
