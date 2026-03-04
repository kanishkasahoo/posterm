pub mod theme;

use std::sync::OnceLock;

use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

pub fn highlight_lines(content_type: Option<&str>, lines: &[String]) -> Option<Vec<Line<'static>>> {
    let syntax = syntax_for_content_type(content_type)?;
    let mut highlighter = HighlightLines::new(syntax, terminal_theme());
    let mut highlighted = Vec::with_capacity(lines.len());

    for line in lines {
        let Ok(regions) = highlighter.highlight_line(line, syntax_set()) else {
            return None;
        };
        let spans = regions
            .into_iter()
            .map(|(style, text)| Span::styled(text.to_string(), theme::syntect_to_ratatui(style)))
            .collect::<Vec<_>>();
        highlighted.push(Line::from(spans));
    }

    Some(highlighted)
}

fn syntax_for_content_type(content_type: Option<&str>) -> Option<&'static SyntaxReference> {
    let ct = content_type?.to_ascii_lowercase();
    if ct.contains("json") {
        syntax_set().find_syntax_by_extension("json")
    } else if ct.contains("xml") {
        syntax_set().find_syntax_by_extension("xml")
    } else if ct.contains("html") {
        syntax_set().find_syntax_by_extension("html")
    } else {
        None
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_nonewlines)
}

fn terminal_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let mut set = ThemeSet::load_defaults();
        set.themes
            .remove("base16-ocean.dark")
            .or_else(|| set.themes.into_values().next())
            .unwrap_or_default()
    })
}
