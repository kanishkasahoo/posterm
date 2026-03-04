use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::highlight;
use crate::state::{AppState, ResponseSearchScope};

const MAX_HIGHLIGHTABLE_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_HIGHLIGHTABLE_RESPONSE_LINES: usize = 4_000;
const MAX_HIGHLIGHTABLE_LINE_BYTES: usize = 8 * 1024;
const MAX_PRETTY_PRINT_RESPONSE_BYTES: usize = 512 * 1024;

#[derive(Default)]
struct HighlightCache {
    request_id: Option<u64>,
    content_type: Option<String>,
    lines: HashMap<usize, CachedLine>,
}

struct CachedLine {
    hash: u64,
    line: Line<'static>,
}

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let inner_height = usize::from(area.height.saturating_sub(2).max(1));
    let inner_width = usize::from(area.width.saturating_sub(2).max(1));
    let view = body_view(state);
    let total_lines = view.total_lines();
    let max_vertical_scroll = total_lines.saturating_sub(inner_height);
    let vertical_scroll = state.response.scroll_offset.min(max_vertical_scroll);

    let mut rendered = Vec::with_capacity(inner_height);
    for row in 0..inner_height {
        let line_index = vertical_scroll + row;
        if line_index >= total_lines {
            break;
        }

        let source_line = view.line_at(line_index);
        let mut line = if view.highlightable && source_line.len() <= MAX_HIGHLIGHTABLE_LINE_BYTES {
            highlighted_line(line_index, view.content_type, source_line, state)
        } else {
            Line::from(source_line.to_string())
        };

        let line_matches = search_matches_for_line(line_index, state, ResponseSearchScope::Body);
        if !line_matches.is_empty() {
            line = apply_search_overlays(line, &line_matches);
        }

        if !state.response.wrap_lines {
            line = slice_line(&line, state.response.horizontal_scroll_offset, inner_width);
        }

        rendered.push(line);
    }

    if rendered.is_empty() {
        rendered.push(Line::from(""));
    }

    let mut paragraph = Paragraph::new(Text::from(rendered)).block(
        Block::default()
            .title(if state.response.wrap_lines {
                "Body (Wrap: On)"
            } else {
                "Body (Wrap: Off)"
            })
            .borders(Borders::ALL),
    );

    if state.response.wrap_lines {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }

    frame.render_widget(paragraph, area);
}

pub(crate) fn apply_search_overlays(
    base: Line<'static>,
    matches: &[(usize, usize, bool)],
) -> Line<'static> {
    if matches.is_empty() {
        return base;
    }

    let mut chars = Vec::new();
    for span in base.spans {
        for ch in span.content.chars() {
            chars.push((ch, span.style));
        }
    }

    for (start, end, is_current) in matches {
        let start_index = (*start).min(chars.len());
        let end_index = (*end).min(chars.len());
        if start_index >= end_index {
            continue;
        }
        let overlay = if *is_current {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().bg(Color::DarkGray)
        };
        for (_, style) in &mut chars[start_index..end_index] {
            *style = style.patch(overlay);
        }
    }

    if chars.is_empty() {
        return Line::from(String::new());
    }

    let mut spans = Vec::new();
    let mut current_style = chars[0].1;
    let mut current = String::new();
    for (ch, style) in chars {
        if style != current_style {
            spans.push(Span::styled(current, current_style));
            current = String::new();
            current_style = style;
        }
        current.push(ch);
    }
    spans.push(Span::styled(current, current_style));

    Line::from(spans)
}

fn highlighted_line(
    line_index: usize,
    content_type: Option<&str>,
    line: &str,
    state: &AppState,
) -> Line<'static> {
    let cache = cache();
    let mut guard = cache.lock().expect("response highlight cache poisoned");
    let request_id = state.response.last_request_id;
    let content_type_owned = content_type.map(str::to_string);

    if guard.request_id != request_id || guard.content_type != content_type_owned {
        guard.request_id = request_id;
        guard.content_type = content_type_owned;
        guard.lines.clear();
    }

    let hash = hash_line(line);
    if let Some(cached) = guard.lines.get(&line_index)
        && cached.hash == hash
    {
        return cached.line.clone();
    }

    let highlighted = highlight::highlight_lines(content_type, &[line.to_string()])
        .and_then(|mut lines| lines.pop())
        .unwrap_or_else(|| Line::from(line.to_string()));

    guard.lines.insert(
        line_index,
        CachedLine {
            hash,
            line: highlighted.clone(),
        },
    );

    highlighted
}

pub(crate) fn search_matches_for_line(
    line_index: usize,
    state: &AppState,
    scope: ResponseSearchScope,
) -> Vec<(usize, usize, bool)> {
    if state.response.search.query.is_empty() || state.response.search.scope != scope {
        return Vec::new();
    }

    state
        .response
        .search
        .matches
        .iter()
        .enumerate()
        .filter_map(|(match_index, entry)| {
            (entry.line_index == line_index).then_some((
                entry.start_char,
                entry.end_char,
                state.response.search.current_match == Some(match_index),
            ))
        })
        .collect()
}

fn slice_line(line: &Line<'static>, start: usize, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::from(String::new());
    }

    let mut chars = Vec::new();
    for span in &line.spans {
        for ch in span.content.chars() {
            chars.push((ch, span.style));
        }
    }

    if start >= chars.len() {
        return Line::from(String::new());
    }

    let end = (start + width).min(chars.len());
    let mut spans = Vec::new();
    let mut current_style = chars[start].1;
    let mut current = String::new();

    for (ch, style) in chars[start..end].iter().copied() {
        if style != current_style {
            spans.push(Span::styled(current, current_style));
            current = String::new();
            current_style = style;
        }
        current.push(ch);
    }
    spans.push(Span::styled(current, current_style));

    Line::from(spans)
}

fn hash_line(line: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut hasher);
    hasher.finish()
}

fn cache() -> &'static Mutex<HighlightCache> {
    static CACHE: OnceLock<Mutex<HighlightCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HighlightCache::default()))
}

enum BodyLines<'a> {
    Buffer(&'a crate::util::streaming_buffer::StreamingBuffer),
    Owned(Vec<String>),
}

struct BodyView<'a> {
    content_type: Option<&'a str>,
    highlightable: bool,
    lines: BodyLines<'a>,
    search_lines: Vec<String>,
}

impl BodyView<'_> {
    fn total_lines(&self) -> usize {
        match &self.lines {
            BodyLines::Buffer(buffer) => buffer.total_lines(),
            BodyLines::Owned(lines) => lines.len(),
        }
    }

    fn line_at(&self, index: usize) -> &str {
        match &self.lines {
            BodyLines::Buffer(buffer) => buffer.line(index).unwrap_or(""),
            BodyLines::Owned(lines) => lines.get(index).map(String::as_str).unwrap_or(""),
        }
    }
}

pub(crate) fn body_search_lines(state: &AppState) -> Vec<String> {
    body_view(state).search_lines
}

fn body_view(state: &AppState) -> BodyView<'_> {
    if let Some(error) = &state.response.last_error {
        return BodyView {
            content_type: None,
            highlightable: false,
            lines: BodyLines::Owned(vec![format!("Request failed: {error}")]),
            search_lines: Vec::new(),
        };
    }
    if state.response.cancelled {
        return BodyView {
            content_type: None,
            highlightable: false,
            lines: BodyLines::Owned(vec![String::from("Request cancelled.")]),
            search_lines: Vec::new(),
        };
    }
    if state.response.buffer.is_empty() && state.response.in_flight.is_some() {
        return BodyView {
            content_type: None,
            highlightable: false,
            lines: BodyLines::Owned(vec![String::from("Waiting for response data...")]),
            search_lines: Vec::new(),
        };
    }
    if state.response.buffer.is_empty() {
        return BodyView {
            content_type: None,
            highlightable: false,
            lines: BodyLines::Owned(vec![String::from("No response body available.")]),
            search_lines: Vec::new(),
        };
    }

    if state.response.in_flight.is_none()
        && state.response.buffer.total_bytes() <= MAX_PRETTY_PRINT_RESPONSE_BYTES
    {
        let content_type = state
            .response
            .metadata
            .as_ref()
            .and_then(|meta| meta.content_type.as_deref());

        if let Some(pretty_lines) = pretty_body_lines(state.response.buffer.as_text(), content_type)
        {
            let line_count = pretty_lines.len();
            return BodyView {
                content_type: Some("application/json"),
                highlightable: state.response.buffer.total_bytes()
                    <= MAX_HIGHLIGHTABLE_RESPONSE_BYTES
                    && line_count <= MAX_HIGHLIGHTABLE_RESPONSE_LINES,
                search_lines: pretty_lines.clone(),
                lines: BodyLines::Owned(pretty_lines),
            };
        }
    }

    BodyView {
        content_type: state
            .response
            .metadata
            .as_ref()
            .and_then(|meta| meta.content_type.as_deref()),
        highlightable: state.response.in_flight.is_none()
            && state.response.buffer.total_bytes() <= MAX_HIGHLIGHTABLE_RESPONSE_BYTES
            && state.response.buffer.total_lines() <= MAX_HIGHLIGHTABLE_RESPONSE_LINES,
        lines: BodyLines::Buffer(&state.response.buffer),
        search_lines: state
            .response
            .buffer
            .as_text()
            .split('\n')
            .map(str::to_string)
            .collect(),
    }
}

fn pretty_body_lines(raw_text: &str, content_type: Option<&str>) -> Option<Vec<String>> {
    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
    let maybe_json = content_type.contains("json")
        || (content_type.is_empty()
            && matches!(raw_text.trim_start().chars().next(), Some('{') | Some('[')));

    if !maybe_json {
        return None;
    }

    let value = serde_json::from_str::<serde_json::Value>(raw_text).ok()?;
    let pretty = serde_json::to_string_pretty(&value).ok()?;
    Some(pretty.split('\n').map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_HIGHLIGHTABLE_RESPONSE_BYTES, MAX_HIGHLIGHTABLE_RESPONSE_LINES, body_search_lines,
        body_view,
    };
    use crate::state::{AppState, HttpMethod, InFlightRequest, LayoutMode, ResponseMetadata};

    #[test]
    fn body_view_disables_highlighting_while_request_is_in_flight() {
        let mut state = AppState::new((120, 40), LayoutMode::Large);
        state.response.buffer.append_chunk(b"{\"ok\":true}");
        state.response.in_flight = Some(InFlightRequest {
            id: 1,
            method: HttpMethod::Get,
            url: String::from("https://example.test"),
            cancellation_requested: false,
        });

        let view = body_view(&state);
        assert!(!view.highlightable);
    }

    #[test]
    fn body_view_disables_highlighting_for_large_responses() {
        let mut state = AppState::new((120, 40), LayoutMode::Large);

        state
            .response
            .buffer
            .append_chunk("a".repeat(MAX_HIGHLIGHTABLE_RESPONSE_BYTES + 1).as_bytes());
        let oversized_bytes = body_view(&state);
        assert!(!oversized_bytes.highlightable);

        state.response.in_flight = None;
        state.response.buffer.clear();
        state.response.buffer.append_chunk(
            format!("{}\n", "line\n".repeat(MAX_HIGHLIGHTABLE_RESPONSE_LINES)).as_bytes(),
        );
        let oversized_lines = body_view(&state);
        assert!(!oversized_lines.highlightable);
    }

    #[test]
    fn body_search_lines_pretty_prints_json_content() {
        let mut state = AppState::new((120, 40), LayoutMode::Large);
        state
            .response
            .buffer
            .append_chunk(br#"{"z":1,"a":{"b":2}}"#);
        state.response.metadata = Some(ResponseMetadata {
            content_type: Some(String::from("application/json")),
            ..ResponseMetadata::default()
        });

        let lines = body_search_lines(&state);
        assert_eq!(lines.first().map(String::as_str), Some("{"));
        assert!(lines.iter().any(|line| line.contains("\"z\": 1")));
    }

    #[test]
    fn body_search_lines_falls_back_for_invalid_json() {
        let mut state = AppState::new((120, 40), LayoutMode::Large);
        state.response.buffer.append_chunk(br#"{"z":1"#);
        state.response.metadata = Some(ResponseMetadata {
            content_type: Some(String::from("application/json")),
            ..ResponseMetadata::default()
        });

        let lines = body_search_lines(&state);
        assert_eq!(lines, vec![String::from("{\"z\":1")]);
    }
}
