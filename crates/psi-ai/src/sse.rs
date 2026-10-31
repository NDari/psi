//! Server-sent events: `data:` lines, events separated by a blank line.

#[derive(Debug, Default)]
pub(crate) struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    /// Feeds bytes and returns the data of every complete event.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = find_event_end(&self.buf) {
            let (event, rest_start) = end;
            let raw = String::from_utf8_lossy(&self.buf[..event]).into_owned();
            self.buf.drain(..rest_start);
            let data: Vec<&str> = raw
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(|d| d.strip_prefix(' ').unwrap_or(d))
                .collect();
            if !data.is_empty() {
                out.push(data.join("\n"));
            }
        }
        out
    }

    /// Whatever is left when the stream ends (an event without its blank line).
    pub fn finish(&mut self) -> Vec<String> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        self.buf.extend_from_slice(b"\n\n");
        self.push(&[])
    }
}

/// (end of the event, start of what follows) for the first "\n\n" or "\r\n\r\n".
fn find_event_end(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|i| (i, i + 2));
    let crlf = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, i + 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_split_across_pushes() {
        let mut d = SseDecoder::default();
        assert!(d.push(b"data: {\"a\"").is_empty());
        assert_eq!(d.push(b":1}\n\ndata: [DONE]\n\n"), ["{\"a\":1}", "[DONE]"]);
    }

    #[test]
    fn crlf_and_trailing_event() {
        let mut d = SseDecoder::default();
        assert_eq!(d.push(b"data: x\r\n\r\ndata: y"), ["x"]);
        assert_eq!(d.finish(), ["y"]);
    }
}
