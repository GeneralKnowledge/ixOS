//! Just enough HTTP/1.1 to talk to WebDriverAgent: one request per
//! connection (`Connection: close`), JSON bodies.

use std::io::{Read, Write};

use crate::{Result, UiError};

/// Sends one HTTP request to WebDriverAgent and returns the status code and
/// body. The real transport opens a usbmuxd connection to the runner's port
/// on the phone; tests use a canned one.
pub trait HttpTransport {
    fn request(&mut self, method: &str, path: &str, body: Option<&[u8]>) -> Result<(u16, Vec<u8>)>;
}

/// Writes a request on `stream` and reads the whole response.
pub fn exchange(
    stream: &mut (impl Read + Write),
    method: &str,
    path: &str,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>)> {
    stream.write_all(&encode_request(method, path, body))?;
    stream.flush()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw)
}

pub fn encode_request(method: &str, path: &str, body: Option<&[u8]>) -> Vec<u8> {
    let body = body.unwrap_or_default();
    let mut out = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost:8100\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if method != "GET" {
        out.push_str("Content-Type: application/json\r\n");
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

pub fn parse_response(raw: &[u8]) -> Result<(u16, Vec<u8>)> {
    let bad = |why: &str| UiError::Wda(format!("bad HTTP response: {why}"));
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| bad("no header end"))?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| bad("headers not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| bad("no status"))?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.eq_ignore_ascii_case("chunked")
        {
            chunked = true;
        }
    }
    let rest = &raw[split + 4..];
    let body = if chunked {
        dechunk(rest).ok_or_else(|| bad("broken chunked body"))?
    } else if let Some(length) = length {
        rest.get(..length)
            .ok_or_else(|| bad("body shorter than Content-Length"))?
            .to_vec()
    } else {
        rest.to_vec()
    };
    Ok((status, body))
}

fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let end = data.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        data = &data[end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(data.get(..size)?);
        data = data.get(size + 2..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_post_with_length() {
        let req = encode_request("POST", "/session", Some(b"{}"));
        assert_eq!(
            String::from_utf8(req).unwrap(),
            "POST /session HTTP/1.1\r\nHost: localhost:8100\r\nConnection: close\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}"
        );
    }

    #[test]
    fn parses_length_and_chunked_bodies() {
        let (status, body) =
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}trailing").unwrap();
        assert_eq!((status, body.as_slice()), (200, b"{}".as_slice()));

        let (status, body) = parse_response(
            b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!((status, body.as_slice()), (404, b"abcde".as_slice()));
    }

    #[test]
    fn exchange_round_trips_over_a_stream() {
        struct Canned(std::io::Cursor<Vec<u8>>, Vec<u8>);
        impl Read for Canned {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buf)
            }
        }
        impl Write for Canned {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.1.extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut stream = Canned(
            std::io::Cursor::new(b"HTTP/1.1 200 OK\r\n\r\n{\"value\":1}".to_vec()),
            Vec::new(),
        );
        let (status, body) = exchange(&mut stream, "GET", "/status", None).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{\"value\":1}");
        assert!(stream.1.starts_with(b"GET /status HTTP/1.1\r\n"));
    }
}
