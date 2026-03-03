use ratatui::Frame;
use ratatui::layout::Rect;

use crate::components::request_builder::key_value_editor::render_editor;
use crate::state::RequestState;

pub struct HeadersEditor;

impl HeadersEditor {
    pub fn render(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
        render_editor(
            frame,
            area,
            "Headers",
            &request.headers,
            request.headers_editor,
            request.focus,
        );
    }
}
