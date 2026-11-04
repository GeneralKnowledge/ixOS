//! A small, blocking Server-Sent Events reader.
//!
//! Implements the parts of the [WHATWG event-stream format] a chat client
//! needs: `event`, `data` (multi-line), `id` and `retry` fields, comment
//! lines, LF and CRLF line endings, and a leading UTF-8 BOM. Comment lines
//! are surfaced as [`SseItem::Comment`] rather than dropped, because servers
//! send them as heartbeats and the reader's caller uses each heartbeat as a
//! chance to notice cancellation.
//!
//! Lone CR line endings (allowed by the spec, used by nobody) are not
//! supported: a CR is only stripped before an LF.
//!
//! [WHATWG event-stream format]: https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation

use std::io::{self, BufRead, Read};

/// Lines longer than this fail the stream instead of growing a buffer.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// One dispatched event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, or `"message"` when the event had none.
    pub event: String,
    /// The `data:` lines joined with `\n`.
    pub data: String,
    /// The last `id:` seen on the stream so far (it persists across events,
    /// as in the spec).
    pub last_event_id: Option<String>,
    /// A `retry:` value seen in this event, in milliseconds.
    pub retry_ms: Option<u64>,
}

/// What the reader yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseItem {
    Event(SseEvent),
    /// A `:` comment line, conventionally a heartbeat. Carries the text after
    /// the colon (one leading space removed).
    Comment(String),
}

/// Incremental parser: feed it lines, it hands back items. Independent of
/// any I/O so it can be tested line by line.
#[derive(Debug, Default)]
pub struct SseParser {
    event: Option<String>,
    /// `None` until a `data:` line arrives. `data:` with an empty value
    /// still counts: it dispatches an event with empty data.
    data: Option<String>,
    last_event_id: Option<String>,
    retry_ms: Option<u64>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Processes one line, without its line terminator.
    pub fn push_line(&mut self, line: &str) -> Option<SseItem> {
        if line.is_empty() {
            return self.dispatch().map(SseItem::Event);
        }
        if let Some(comment) = line.strip_prefix(':') {
            return Some(SseItem::Comment(
                comment.strip_prefix(' ').unwrap_or(comment).to_string(),
            ));
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => match self.data.as_mut() {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => self.data = Some(value.to_string()),
            },
            "id" if !value.contains('\0') => self.last_event_id = Some(value.to_string()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                self.retry_ms = value.parse().ok();
            }
            _ => {} // Unknown fields are ignored, per the spec.
        }
        None
    }

    /// Called at end of stream. Per the spec an event that was not
    /// terminated by a blank line is discarded, so this only resets state.
    pub fn finish(&mut self) {
        self.event = None;
        self.data = None;
        self.retry_ms = None;
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = self.event.take();
        let retry_ms = self.retry_ms.take();
        let data = self.data.take()?;
        Some(SseEvent {
            event: event
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| "message".into()),
            data,
            last_event_id: self.last_event_id.clone(),
            retry_ms,
        })
    }
}

/// Reads [`SseItem`]s from a byte stream, blocking on each read.
pub struct SseReader<R> {
    reader: R,
    parser: SseParser,
    line: Vec<u8>,
    first_line: bool,
    done: bool,
}

impl<R: BufRead> SseReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            parser: SseParser::new(),
            line: Vec::new(),
            first_line: true,
            done: false,
        }
    }

    /// Reads one raw line into `self.line`, without its terminator.
    /// `Ok(false)` at end of stream.
    fn read_line(&mut self) -> io::Result<bool> {
        self.line.clear();
        let limit = (MAX_LINE_BYTES + 2) as u64; // + CRLF
        let n = (&mut self.reader)
            .take(limit)
            .read_until(b'\n', &mut self.line)?;
        if n == 0 {
            return Ok(false);
        }
        let terminated = self.line.last() == Some(&b'\n');
        if terminated {
            self.line.pop();
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
        } else if n as u64 == limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("SSE line longer than {MAX_LINE_BYTES} bytes"),
            ));
        }
        if self.first_line {
            self.first_line = false;
            if self.line.starts_with("\u{feff}".as_bytes()) {
                self.line.drain(..3);
            }
        }
        Ok(true)
    }
}

impl<R: BufRead> Iterator for SseReader<R> {
    type Item = io::Result<SseItem>;

    /// `None` once the stream has ended (cleanly or after an error).
    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            match self.read_line() {
                Ok(true) => {
                    let line = match std::str::from_utf8(&self.line) {
                        Ok(line) => line,
                        Err(err) => {
                            self.done = true;
                            return Some(Err(io::Error::new(io::ErrorKind::InvalidData, err)));
                        }
                    };
                    if let Some(item) = self.parser.push_line(line) {
                        return Some(Ok(item));
                    }
                }
                Ok(false) => {
                    self.parser.finish();
                    self.done = true;
                }
                Err(err) => {
                    self.done = true;
                    return Some(Err(err));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(raw: &str) -> Vec<SseItem> {
        SseReader::new(raw.as_bytes())
            .collect::<io::Result<Vec<_>>>()
            .unwrap()
    }

    fn event(event: &str, data: &str) -> SseItem {
        SseItem::Event(SseEvent {
            event: event.into(),
            data: data.into(),
            last_event_id: None,
            retry_ms: None,
        })
    }

    #[test]
    fn parses_named_and_default_events() {
        assert_eq!(
            items("event: message\ndata: {\"a\":1}\n\ndata: plain\n\n"),
            vec![event("message", "{\"a\":1}"), event("message", "plain")]
        );
        assert_eq!(items("event: room\ndata: x\n\n"), vec![event("room", "x")]);
    }

    #[test]
    fn joins_multiline_data_and_handles_crlf() {
        assert_eq!(
            items("data: one\r\ndata: two\r\ndata\r\n\r\n"),
            vec![event("message", "one\ntwo\n")]
        );
    }

    #[test]
    fn empty_data_still_dispatches() {
        assert_eq!(items("data:\n\n"), vec![event("message", "")]);
        assert_eq!(items("data:\ndata:\n\n"), vec![event("message", "\n")]);
    }

    #[test]
    fn data_may_contain_colons_and_keeps_extra_spaces() {
        assert_eq!(items("data:  a: b\n\n"), vec![event("message", " a: b")]);
    }

    #[test]
    fn comments_are_surfaced_as_heartbeats() {
        assert_eq!(
            items(": ping\n:\ndata: x\n\n"),
            vec![
                SseItem::Comment("ping".into()),
                SseItem::Comment(String::new()),
                event("message", "x")
            ]
        );
    }

    #[test]
    fn blank_lines_without_data_dispatch_nothing() {
        assert_eq!(items("\n\nevent: x\n\n"), vec![]);
    }

    #[test]
    fn id_persists_and_retry_is_per_event() {
        let got = items("id: 7\nretry: 1500\ndata: a\n\ndata: b\n\nretry: soon\ndata: c\n\n");
        let SseItem::Event(a) = &got[0] else { panic!() };
        let SseItem::Event(b) = &got[1] else { panic!() };
        let SseItem::Event(c) = &got[2] else { panic!() };
        assert_eq!(a.last_event_id.as_deref(), Some("7"));
        assert_eq!(a.retry_ms, Some(1500));
        assert_eq!(b.last_event_id.as_deref(), Some("7"));
        assert_eq!(b.retry_ms, None);
        assert_eq!(c.retry_ms, None);
    }

    #[test]
    fn unterminated_final_event_is_discarded() {
        assert_eq!(
            items("data: a\n\ndata: partial"),
            vec![event("message", "a")]
        );
    }

    #[test]
    fn leading_bom_is_ignored() {
        assert_eq!(items("\u{feff}data: x\n\n"), vec![event("message", "x")]);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        assert_eq!(items("foo: bar\ndata: x\n\n"), vec![event("message", "x")]);
    }

    #[test]
    fn overlong_line_fails_the_stream() {
        let raw = format!("data: {}\n\n", "x".repeat(MAX_LINE_BYTES + 10));
        let mut reader = SseReader::new(raw.as_bytes());
        let err = reader.next().unwrap().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(reader.next().is_none());
    }

    #[test]
    fn invalid_utf8_fails_the_stream() {
        let raw: &[u8] = b"data: \xff\n\n";
        let mut reader = SseReader::new(raw);
        assert!(reader.next().unwrap().is_err());
        assert!(reader.next().is_none());
    }

    #[test]
    fn parser_works_line_by_line() {
        let mut parser = SseParser::new();
        assert_eq!(parser.push_line("event: message"), None);
        assert_eq!(parser.push_line("data: hi"), None);
        assert_eq!(parser.push_line(""), Some(event("message", "hi")));
    }
}
