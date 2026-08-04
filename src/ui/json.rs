//! Response body rendering: JSON pretty-print + syntax highlight.

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

use crate::http::ResponseData;
use crate::model::human_size;
use crate::theme;

const MAX_RENDER: usize = 2 * 1024 * 1024;
const MAX_HIGHLIGHT: usize = 400 * 1024;

/// Build the cached, styled body text for a response. Returns (text, line count).
pub fn response_text(data: &ResponseData) -> (Text<'static>, usize) {
    if data.body.is_empty() {
        let line = Line::styled("(empty body)", Style::new().fg(theme::DIM));
        return (Text::from(line), 1);
    }
    let truncated = data.body.len() > MAX_RENDER;
    let slice = &data.body[..data.body.len().min(MAX_RENDER)];
    let text_str = String::from_utf8_lossy(slice).into_owned();

    let content_type = data
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default();
    let looks_json = content_type.contains("json")
        || matches!(text_str.trim_start().chars().next(), Some('{') | Some('['));

    let mut lines: Vec<Line<'static>> = if looks_json && slice.len() <= MAX_HIGHLIGHT {
        match serde_json::from_str::<serde_json::Value>(&text_str) {
            Ok(v) => {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or(text_str);
                highlight_json(&pretty)
            }
            Err(_) => plain(&text_str),
        }
    } else {
        plain(&text_str)
    };

    if truncated {
        lines.push(Line::styled(
            format!("… output truncated ({} total)", human_size(data.body.len())),
            Style::new().fg(theme::YELLOW),
        ));
    }
    let n = lines.len();
    (Text::from(lines), n)
}

fn plain(s: &str) -> Vec<Line<'static>> {
    let style = Style::new().fg(theme::FG);
    s.lines()
        .map(|l| Line::styled(l.to_string(), style))
        .collect()
}

/// Number of screen lines the highlighter produced (test helper).
#[cfg(test)]
fn rendered(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect()
}

pub fn highlight_json(src: &str) -> Vec<Line<'static>> {
    let key_s = Style::new().fg(theme::CYAN);
    let str_s = Style::new().fg(theme::GREEN);
    let num_s = Style::new().fg(theme::ORANGE);
    let lit_s = Style::new().fg(theme::PURPLE);
    let punct_s = Style::new().fg(theme::DIM);
    let plain_s = Style::new().fg(theme::FG);

    src.lines()
        .map(|raw| {
            let chars: Vec<char> = raw.chars().collect();
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut i = 0;
            while i < chars.len() {
                let start = i;
                let c = chars[i];
                if c == '"' {
                    i += 1;
                    while i < chars.len() {
                        match chars[i] {
                            '\\' => i += 2,
                            '"' => {
                                i += 1;
                                break;
                            }
                            _ => i += 1,
                        }
                    }
                    let end = i.min(chars.len());
                    let s: String = chars[start..end].iter().collect();
                    let mut j = end;
                    while j < chars.len() && chars[j] == ' ' {
                        j += 1;
                    }
                    let is_key = chars.get(j) == Some(&':');
                    spans.push(Span::styled(s, if is_key { key_s } else { str_s }));
                } else if c.is_ascii_digit()
                    || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
                {
                    i += 1;
                    while i < chars.len()
                        && (chars[i].is_ascii_digit() || "+-.eE".contains(chars[i]))
                    {
                        i += 1;
                    }
                    let s: String = chars[start..i].iter().collect();
                    spans.push(Span::styled(s, num_s));
                } else if c.is_ascii_alphabetic() {
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let s: String = chars[start..i].iter().collect();
                    let style = if s == "true" || s == "false" || s == "null" {
                        lit_s
                    } else {
                        plain_s
                    };
                    spans.push(Span::styled(s, style));
                } else {
                    while i < chars.len()
                        && chars[i] != '"'
                        && !chars[i].is_ascii_alphanumeric()
                        && chars[i] != '-'
                    {
                        i += 1;
                    }
                    if i == start {
                        i += 1; // lone '-' etc: consume one char to guarantee progress
                    }
                    let s: String = chars[start..i].iter().collect();
                    spans.push(Span::styled(s, punct_s));
                }
            }
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp(content_type: &str, body: &str) -> ResponseData {
        ResponseData {
            status: 200,
            reason: "OK".into(),
            version: "HTTP/1.1".into(),
            headers: vec![("content-type".into(), content_type.into())],
            body: body.as_bytes().to_vec(),
            elapsed_ms: 1,
        }
    }

    #[test]
    fn highlighting_never_drops_or_duplicates_characters() {
        // Every input line must round-trip exactly, whatever the token mix.
        for src in [
            r#"{"a": 1, "b": [true, null], "c": -2.5e3}"#,
            r#"{"escaped": "quote \" and \\ backslash"}"#,
            "{}",
            "[]",
            "  - not json at all -  ",
            "{\"unicode\": \"héllo ✓ 日本\"}",
        ] {
            let out = rendered(&highlight_json(src));
            assert_eq!(out.join("\n"), src, "round-trip failed for {src:?}");
        }
    }

    #[test]
    fn highlighter_terminates_on_pathological_input() {
        // A lone '-' and unterminated strings used to risk a stalled scanner.
        for src in ["-", "\"unterminated", "---", "{\"k\": \""] {
            let out = rendered(&highlight_json(src));
            assert_eq!(out.join("\n"), src);
        }
    }

    #[test]
    fn keys_strings_numbers_and_literals_get_distinct_colours() {
        let line = &highlight_json(r#"{"k": "v", "n": 12, "b": true}"#)[0];
        let colour_of = |text: &str| {
            line.spans
                .iter()
                .find(|s| s.content.contains(text))
                .unwrap_or_else(|| panic!("no span for {text:?}"))
                .style
                .fg
                .unwrap()
        };
        assert_eq!(colour_of("\"k\""), theme::CYAN, "keys");
        assert_eq!(colour_of("\"v\""), theme::GREEN, "string values");
        assert_eq!(colour_of("12"), theme::ORANGE, "numbers");
        assert_eq!(colour_of("true"), theme::PURPLE, "literals");
    }

    #[test]
    fn json_bodies_are_pretty_printed_with_original_key_order() {
        let (text, lines) = response_text(&resp("application/json", r#"{"z":1,"a":{"y":2}}"#));
        let flat: Vec<String> = text
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(lines > 1, "expanded over several lines");
        let joined = flat.join("\n");
        assert!(joined.contains("\"z\": 1"), "reflowed:\n{joined}");
        assert!(
            joined.find("\"z\"") < joined.find("\"a\""),
            "key order preserved:\n{joined}"
        );
    }

    #[test]
    fn non_json_bodies_render_verbatim() {
        let (text, lines) = response_text(&resp("text/plain", "line one\nline two"));
        assert_eq!(lines, 2);
        let first: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(first, "line one");
    }

    #[test]
    fn malformed_json_falls_back_to_plain_text() {
        // A half-received SSE/JSON chunk must still render rather than vanish.
        let (text, lines) = response_text(&resp("application/json", "{\"partial\": tru"));
        assert_eq!(lines, 1);
        let only: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(only, "{\"partial\": tru");
    }

    #[test]
    fn an_empty_body_says_so() {
        let (_, lines) = response_text(&resp("application/json", ""));
        assert_eq!(lines, 1);
    }

    #[test]
    fn bodies_are_detected_as_json_without_a_content_type() {
        let (text, lines) = response_text(&resp("", r#"{"sniffed":true}"#));
        assert!(lines > 1, "sniffed and pretty-printed");
        let joined: String = text
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("\"sniffed\": true"));
    }
}
