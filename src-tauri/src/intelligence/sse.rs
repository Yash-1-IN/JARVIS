//! Minimal server-sent-events parser. Chunks may split anywhere, including
//! inside a UTF-8 character, so bytes are buffered until a full event arrives.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((end, sep_len)) = find_blank_line(&self.buf) {
            let raw: Vec<u8> = self.buf.drain(..end + sep_len).collect();
            let text = String::from_utf8_lossy(&raw[..end]);
            if let Some(ev) = parse_block(&text) {
                out.push(ev);
            }
        }
        out
    }
}

fn find_blank_line(b: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"\r\n\r\n") {
            return Some((i, 4));
        }
        if b[i..].starts_with(b"\n\n") {
            return Some((i, 2));
        }
        i += 1;
    }
    None
}

fn parse_block(text: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.starts_with(':') || line.is_empty() {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data.push(value),
            _ => {}
        }
    }
    if data.is_empty() && event.is_none() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let mut p = SseParser::default();
        assert!(p.push(b"event: ping\nda").is_empty());
        let evs = p.push(b"ta: {\"a\":1}\n\nevent: x\ndata: 2\n\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("ping"));
        assert_eq!(evs[0].data, "{\"a\":1}");
        assert_eq!(evs[1].data, "2");
    }

    #[test]
    fn handles_crlf_comments_and_split_utf8() {
        let mut p = SseParser::default();
        let full = "data: caf\u{e9}\r\n\r\n: keepalive\n\n".as_bytes().to_vec();
        let (a, b) = full.split_at(9); // splits the two-byte 'é'
        assert!(p.push(a).is_empty());
        let evs = p.push(b);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "caf\u{e9}");
    }
}
