pub fn sanitize_terminal_text(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];

        if byte == 0x1b {
            index = skip_escape_sequence(bytes, index);
            continue;
        }

        if is_c1_control(bytes, index) {
            index += 2;
            continue;
        }

        if is_allowed_byte(byte) {
            out.push(byte);
        }

        index += 1;
    }

    String::from_utf8(out).unwrap_or_default()
}

fn is_allowed_byte(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r' | b'\t') || !matches!(byte, 0x00..=0x1f | 0x7f)
}

fn is_c1_control(bytes: &[u8], index: usize) -> bool {
    bytes.get(index) == Some(&0xc2)
        && bytes
            .get(index + 1)
            .is_some_and(|next| (0x80..=0x9f).contains(next))
}

fn skip_escape_sequence(bytes: &[u8], esc_index: usize) -> usize {
    let Some(next) = bytes.get(esc_index + 1).copied() else {
        return esc_index + 1;
    };

    match next {
        b'[' => skip_csi_sequence(bytes, esc_index + 2),
        b']' => skip_osc_sequence(bytes, esc_index + 2),
        b'P' | b'X' | b'^' | b'_' => skip_st_terminated_sequence(bytes, esc_index + 2),
        _ => (esc_index + 2).min(bytes.len()),
    }
}

fn skip_csi_sequence(bytes: &[u8], mut index: usize) -> usize {
    while let Some(byte) = bytes.get(index).copied() {
        if (0x40..=0x7e).contains(&byte) {
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn skip_osc_sequence(bytes: &[u8], mut index: usize) -> usize {
    while let Some(byte) = bytes.get(index).copied() {
        if byte == 0x07 {
            return index + 1;
        }

        if byte == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
            return index + 2;
        }

        index += 1;
    }

    bytes.len()
}

fn skip_st_terminated_sequence(bytes: &[u8], mut index: usize) -> usize {
    while let Some(byte) = bytes.get(index).copied() {
        if byte == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
            return index + 2;
        }
        index += 1;
    }

    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_text;

    #[test]
    fn strips_csi_escape_sequences() {
        let payload = "ok\u{1b}[31mred\u{1b}[0m done";
        assert_eq!(sanitize_terminal_text(payload), "okred done");
    }

    #[test]
    fn strips_osc_escape_sequences() {
        let payload = "safe\u{1b}]8;;https://evil.example\u{7}link\u{1b}]8;;\u{7} tail";
        assert_eq!(sanitize_terminal_text(payload), "safelink tail");
    }

    #[test]
    fn preserves_safe_whitespace_and_utf8() {
        let payload = "line1\nline2\t🙂\r\u{0008}x";
        assert_eq!(sanitize_terminal_text(payload), "line1\nline2\t🙂\rx");
    }

    #[test]
    fn strips_c1_control_characters() {
        let payload = "one\u{0085}two\u{009b}three\u{009f}four";
        assert_eq!(sanitize_terminal_text(payload), "onetwothreefour");
    }
}
