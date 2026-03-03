use crate::state::{KeyValueRow, QueryParamToken};

pub fn parse_query_params(url: &str) -> (Vec<KeyValueRow>, Vec<QueryParamToken>) {
    let without_fragment = url.split_once('#').map_or(url, |(prefix, _)| prefix);
    let Some((_, query)) = without_fragment.split_once('?') else {
        return (Vec::new(), Vec::new());
    };

    let mut rows = Vec::new();
    let mut tokens = Vec::new();

    for segment in query.split('&') {
        if segment.is_empty() {
            rows.push(KeyValueRow::default());
            tokens.push(QueryParamToken::EmptySegment);
            continue;
        }

        if let Some((raw_key, raw_value)) = segment.split_once('=') {
            rows.push(KeyValueRow {
                enabled: true,
                key: percent_decode_preserving_invalid(raw_key),
                value: percent_decode_preserving_invalid(raw_value),
            });
            tokens.push(QueryParamToken::KeyValue);
        } else {
            rows.push(KeyValueRow {
                enabled: true,
                key: percent_decode_preserving_invalid(segment),
                value: String::new(),
            });
            tokens.push(QueryParamToken::KeyOnly);
        }
    }

    (rows, tokens)
}

pub fn rebuild_url_with_params(
    base_url_or_url: &str,
    params: &[KeyValueRow],
    tokens: &[QueryParamToken],
) -> String {
    let (without_fragment, fragment) = match base_url_or_url.split_once('#') {
        Some((prefix, suffix)) => (prefix, Some(suffix)),
        None => (base_url_or_url, None),
    };

    let base = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(prefix, _)| prefix);

    let segments = params
        .iter()
        .enumerate()
        .filter(|(_, row)| row.enabled)
        .filter_map(|(idx, row)| {
            let token = tokens
                .get(idx)
                .copied()
                .unwrap_or(QueryParamToken::KeyValue);
            match token {
                QueryParamToken::EmptySegment => Some(String::new()),
                QueryParamToken::KeyOnly => {
                    if row.key.is_empty() {
                        None
                    } else {
                        Some(percent_encode_query_component(&row.key))
                    }
                }
                QueryParamToken::KeyValue => {
                    if row.key.is_empty() && row.value.is_empty() {
                        None
                    } else {
                        Some(format!(
                            "{}={}",
                            percent_encode_query_component(&row.key),
                            percent_encode_query_component(&row.value)
                        ))
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let has_segments = !segments.is_empty();
    let query = segments.join("&");

    let mut rebuilt = String::from(base);
    if has_segments {
        rebuilt.push('?');
        rebuilt.push_str(&query);
    }

    if let Some(fragment) = fragment {
        rebuilt.push('#');
        rebuilt.push_str(fragment);
    }

    rebuilt
}

fn percent_decode_preserving_invalid(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut index = 0;

    while index < raw.len() {
        if raw[index] == b'%' {
            if index + 2 < raw.len() && is_hex(raw[index + 1]) && is_hex(raw[index + 2]) {
                bytes.push((hex_value(raw[index + 1]) << 4) | hex_value(raw[index + 2]));
                index += 3;
                continue;
            }

            bytes.push(b'%');
            index += 1;
            continue;
        }

        bytes.push(raw[index]);
        index += 1;
    }

    match String::from_utf8(bytes) {
        Ok(decoded) => decoded,
        Err(_) => input.to_string(),
    }
}

fn percent_encode_query_component(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(nibble_to_hex(byte >> 4));
            output.push(nibble_to_hex(byte & 0x0f));
        }
    }
    output
}

fn is_hex(value: u8) -> bool {
    value.is_ascii_hexdigit()
}

fn hex_value(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'A' + (value - 10)),
        _ => '0',
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_query_params, rebuild_url_with_params};
    use crate::state::{KeyValueRow, QueryParamToken};

    #[test]
    fn parses_query_params_from_url() {
        let (rows, tokens) = parse_query_params("https://example.com/path?a=1&b=two+words");
        assert_eq!(rows.len(), 2);
        assert_eq!(tokens.len(), 2);
        assert_eq!(rows[0].key, "a");
        assert_eq!(rows[0].value, "1");
        assert_eq!(rows[1].key, "b");
        assert_eq!(rows[1].value, "two+words");
    }

    #[test]
    fn rebuilds_url_with_enabled_params_only() {
        let params = vec![
            KeyValueRow {
                enabled: true,
                key: String::from("name"),
                value: String::from("posterm cli"),
            },
            KeyValueRow {
                enabled: false,
                key: String::from("skip"),
                value: String::from("1"),
            },
        ];

        let url = rebuild_url_with_params(
            "https://example.com/api?old=1#frag",
            &params,
            &[QueryParamToken::KeyValue, QueryParamToken::KeyValue],
        );
        assert_eq!(url, "https://example.com/api?name=posterm%20cli#frag");
    }

    #[test]
    fn round_trips_plus_and_encoded_plus_safely() {
        let (rows, tokens) = parse_query_params("https://example.com/path?a=+&b=%2B");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, "+");
        assert_eq!(rows[1].value, "+");

        let rebuilt = rebuild_url_with_params("https://example.com/path", &rows, &tokens);
        assert_eq!(rebuilt, "https://example.com/path?a=%2B&b=%2B");
    }

    #[test]
    fn preserves_malformed_percent_sequences_without_panicking() {
        let (rows, tokens) = parse_query_params("https://example.com/path?bad=%ZZ&tail=%");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, "%ZZ");
        assert_eq!(rows[1].value, "%");

        let rebuilt = rebuild_url_with_params("https://example.com/path", &rows, &tokens);
        assert_eq!(rebuilt, "https://example.com/path?bad=%25ZZ&tail=%25");
    }

    #[test]
    fn handles_urls_without_query() {
        let (rows, tokens) = parse_query_params("https://example.com/path#frag");
        assert!(rows.is_empty());
        assert!(tokens.is_empty());

        let rebuilt = rebuild_url_with_params("https://example.com/path#frag", &rows, &tokens);
        assert_eq!(rebuilt, "https://example.com/path#frag");
    }

    #[test]
    fn preserves_key_only_and_empty_value_distinction() {
        let (rows, tokens) = parse_query_params("https://example.com/path?flag&empty=");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            tokens,
            vec![QueryParamToken::KeyOnly, QueryParamToken::KeyValue]
        );

        let rebuilt = rebuild_url_with_params("https://example.com/path", &rows, &tokens);
        assert_eq!(rebuilt, "https://example.com/path?flag&empty=");
    }

    #[test]
    fn preserves_empty_query_segments_without_panicking() {
        let (rows, tokens) = parse_query_params("https://example.com/path?a=1&&b=2");
        assert_eq!(rows.len(), 3);
        assert_eq!(tokens[1], QueryParamToken::EmptySegment);

        let rebuilt = rebuild_url_with_params("https://example.com/path", &rows, &tokens);
        assert_eq!(rebuilt, "https://example.com/path?a=1&&b=2");
    }
}
