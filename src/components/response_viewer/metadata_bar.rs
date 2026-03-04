use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::state::AppState;

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let status_span = status_span(state);
    let elapsed_ms = state
        .response
        .metadata
        .as_ref()
        .map(|metadata| metadata.duration_ms)
        .unwrap_or(0);
    let body_size = state
        .response
        .metadata
        .as_ref()
        .map(|metadata| metadata.total_bytes)
        .unwrap_or_else(|| state.response.buffer.total_bytes());
    let truncated = state
        .response
        .metadata
        .as_ref()
        .map(|metadata| metadata.truncated)
        .unwrap_or(state.response.truncated);

    let compact = area.width < 52;
    let very_compact = area.width < 34;

    let mut spans = vec![if very_compact {
        Span::raw("S:")
    } else {
        Span::raw("Status: ")
    }];
    spans.push(status_span);

    if !very_compact {
        spans.push(Span::raw(if compact { " | T:" } else { "  |  Time: " }));
        spans.push(Span::raw(format!("{elapsed_ms} ms")));
    }

    if !compact {
        spans.push(Span::raw("  |  Size: "));
        spans.push(Span::raw(format_bytes(body_size)));
    }

    if truncated && !very_compact {
        spans.push(Span::raw(if compact { " | " } else { "  |  " }));
        spans.push(Span::styled(
            if compact { "TRUNC" } else { "TRUNCATED" },
            Style::default().fg(Color::Yellow),
        ));
    }

    let line = Line::from(spans);

    frame.render_widget(Paragraph::new(line), area);
}

fn status_span(state: &AppState) -> Span<'static> {
    if state.response.in_flight.is_some() {
        return Span::styled("SENDING", Style::default().fg(Color::Cyan));
    }

    if state.response.cancelled {
        return Span::styled("CANCELLED", Style::default().fg(Color::Yellow));
    }

    if state.response.last_error.is_some() {
        return Span::styled("ERROR", Style::default().fg(Color::Red));
    }

    let Some(metadata) = &state.response.metadata else {
        return Span::styled("IDLE", Style::default().fg(Color::DarkGray));
    };

    let Some(status_code) = metadata.status_code else {
        return Span::styled("UNKNOWN", Style::default().fg(Color::DarkGray));
    };

    let color = match status_code {
        200..=299 => Color::Green,
        300..=399 => Color::Yellow,
        400..=499 => Color::LightRed,
        500..=599 => Color::Red,
        _ => Color::White,
    };

    Span::styled(status_code.to_string(), Style::default().fg(color))
}

fn format_bytes(size: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;

    if (size as f64) >= MB {
        format!("{:.2} MB", (size as f64) / MB)
    } else if (size as f64) >= KB {
        format!("{:.1} KB", (size as f64) / KB)
    } else {
        format!("{size} B")
    }
}
