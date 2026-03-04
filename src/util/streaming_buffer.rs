use crate::util::terminal_sanitize::sanitize_terminal_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingBuffer {
    text: String,
    line_starts: Vec<usize>,
    total_received_bytes: usize,
    buffered_bytes: usize,
    max_buffered_bytes: usize,
    truncated: bool,
}

impl StreamingBuffer {
    pub fn new(max_buffered_bytes: usize) -> Self {
        Self {
            text: String::new(),
            line_starts: vec![0],
            total_received_bytes: 0,
            buffered_bytes: 0,
            max_buffered_bytes,
            truncated: false,
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.line_starts.clear();
        self.line_starts.push(0);
        self.total_received_bytes = 0;
        self.buffered_bytes = 0;
        self.truncated = false;
    }

    pub fn append_chunk(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }

        self.total_received_bytes = self.total_received_bytes.saturating_add(chunk.len());

        if self.truncated {
            return;
        }

        let remaining = self.max_buffered_bytes.saturating_sub(self.buffered_bytes);
        if remaining == 0 {
            self.truncated = true;
            return;
        }

        let accepted = chunk.len().min(remaining);
        if accepted < chunk.len() {
            self.truncated = true;
        }

        self.buffered_bytes = self.buffered_bytes.saturating_add(accepted);

        let start_offset = self.text.len();
        let decoded = String::from_utf8_lossy(&chunk[..accepted]);
        let sanitized = sanitize_terminal_text(decoded.as_ref());
        self.text.push_str(&sanitized);

        for (index, byte) in self.text.as_bytes()[start_offset..].iter().enumerate() {
            if *byte == b'\n' {
                self.line_starts.push(start_offset + index + 1);
            }
        }
    }

    pub fn total_bytes(&self) -> usize {
        self.total_received_bytes
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn total_lines(&self) -> usize {
        self.line_starts.len().max(1)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn as_text(&self) -> &str {
        &self.text
    }

    pub fn line(&self, index: usize) -> Option<&str> {
        if self.line_starts.is_empty() || index >= self.line_starts.len() {
            return None;
        }

        let start = self.line_starts[index];
        let end = if index + 1 < self.line_starts.len() {
            self.line_starts[index + 1].saturating_sub(1)
        } else {
            self.text.len()
        };

        self.text.get(start..end)
    }
}

impl Default for StreamingBuffer {
    fn default() -> Self {
        Self::new(512 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::StreamingBuffer;

    #[test]
    fn appends_chunks_and_tracks_lines() {
        let mut buffer = StreamingBuffer::new(1024);
        buffer.append_chunk(b"hello\nwor");
        buffer.append_chunk(b"ld\n");

        assert_eq!(buffer.total_lines(), 3);
        assert_eq!(buffer.as_text(), "hello\nworld\n");
        assert_eq!(buffer.total_bytes(), 12);
        assert!(!buffer.is_truncated());
    }

    #[test]
    fn handles_invalid_utf8_without_panicking() {
        let mut buffer = StreamingBuffer::new(1024);
        buffer.append_chunk(&[0x66, 0x6f, 0x80, 0x6f]);

        assert_eq!(buffer.total_lines(), 1);
        assert_eq!(buffer.as_text(), "fo\u{fffd}o");
        assert_eq!(buffer.total_bytes(), 4);
    }

    #[test]
    fn truncates_at_max_buffered_bytes() {
        let mut buffer = StreamingBuffer::new(5);
        buffer.append_chunk(b"1234");
        buffer.append_chunk(b"56789");

        assert_eq!(buffer.as_text(), "12345");
        assert_eq!(buffer.total_bytes(), 9);
        assert!(buffer.is_truncated());
    }

    #[test]
    fn strips_escape_sequences_from_streamed_body() {
        let mut buffer = StreamingBuffer::new(1024);
        buffer.append_chunk(b"ok\x1b[31mred\x1b[0m");

        assert_eq!(buffer.as_text(), "okred");
    }
}
