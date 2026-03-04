use ratatui::style::{Color, Modifier, Style};
use syntect::highlighting::{Color as SyntectColor, FontStyle, Style as SyntectStyle};

pub fn syntect_to_ratatui(style: SyntectStyle) -> Style {
    let mut mapped = Style::default().fg(ansi_color(style.foreground));

    if style.font_style.contains(FontStyle::BOLD) {
        mapped = mapped.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        mapped = mapped.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        mapped = mapped.add_modifier(Modifier::UNDERLINED);
    }

    mapped
}

fn ansi_color(color: SyntectColor) -> Color {
    const PALETTE: &[(Color, u8, u8, u8)] = &[
        (Color::Black, 0, 0, 0),
        (Color::Red, 205, 49, 49),
        (Color::Green, 13, 188, 121),
        (Color::Yellow, 229, 229, 16),
        (Color::Blue, 36, 114, 200),
        (Color::Magenta, 188, 63, 188),
        (Color::Cyan, 17, 168, 205),
        (Color::Gray, 229, 229, 229),
        (Color::DarkGray, 102, 102, 102),
        (Color::LightRed, 241, 76, 76),
        (Color::LightGreen, 35, 209, 139),
        (Color::LightYellow, 245, 245, 67),
        (Color::LightBlue, 59, 142, 234),
        (Color::LightMagenta, 214, 112, 214),
        (Color::LightCyan, 41, 184, 219),
        (Color::White, 255, 255, 255),
    ];

    let (r, g, b) = (i32::from(color.r), i32::from(color.g), i32::from(color.b));
    let mut best = Color::White;
    let mut best_distance = i32::MAX;

    for (candidate, pr, pg, pb) in PALETTE {
        let dr = r - i32::from(*pr);
        let dg = g - i32::from(*pg);
        let db = b - i32::from(*pb);
        let distance = dr * dr + dg * dg + db * db;
        if distance < best_distance {
            best_distance = distance;
            best = *candidate;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::syntect_to_ratatui;
    use ratatui::style::Modifier;
    use syntect::highlighting::{Color, FontStyle, Style};

    #[test]
    fn maps_syntect_style_to_terminal_style() {
        let style = Style {
            foreground: Color {
                r: 0,
                g: 255,
                b: 0,
                a: 0xFF,
            },
            background: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0xFF,
            },
            font_style: FontStyle::BOLD | FontStyle::UNDERLINE,
        };

        let mapped = syntect_to_ratatui(style);
        assert!(mapped.add_modifier.contains(Modifier::BOLD));
        assert!(mapped.add_modifier.contains(Modifier::UNDERLINED));
    }
}
