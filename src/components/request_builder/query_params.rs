use ratatui::Frame;
use ratatui::layout::Rect;

use crate::components::request_builder::key_value_editor::render_editor;
use crate::state::RequestState;

pub struct QueryParams;

impl QueryParams {
    pub fn render(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
        render_editor(
            frame,
            area,
            "Query Params",
            &request.query_params,
            request.query_editor,
            request.focus,
        );
    }
}
