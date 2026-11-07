//! Splits the URLs an app is opened with (custom schemes such as
//! `myapp://note/42`, universal links such as `https://example.com/note/42`)
//! into parts the app can route on. Small and dependency-free: it accepts
//! what iOS and Android hand an app, not every RFC 3986 corner.

/// Longest URL accepted.
pub const MAX_URL_LEN: usize = 8 * 1024;

/// A URL split into decoded parts. See the `DeepLink` record for meanings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLink {
    pub scheme: String,
    pub host: Option<String>,
    pub path: Vec<String>,
    pub query: Vec<(String, String)>,
    pub fragment: Option<String>,
    pub route: Vec<String>,
}

/// Parses `url`, or explains why it is not one.
pub fn parse(url: &str) -> Result<ParsedLink, String> {
    if url.is_empty() {
        return Err("the URL is empty".into());
    }
    if url.len() > MAX_URL_LEN {
        return Err(format!("the URL is longer than {MAX_URL_LEN} bytes"));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("the URL contains spaces or control characters".into());
    }
    let (scheme, rest) = url
        .split_once(':')
        .ok_or_else(|| "the URL has no scheme (expected scheme:...)".to_string())?;
    let mut chars = scheme.chars();
    let scheme_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_ok {
        return Err(format!("{scheme:?} is not a URL scheme"));
    }
    let scheme = scheme.to_ascii_lowercase();

    let (rest, fragment) = match rest.split_once('#') {
        Some((rest, fragment)) => (rest, Some(decode(fragment, false)?)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, parse_query(query)?),
        None => (rest, Vec::new()),
    };
    let (host, path) = match rest.strip_prefix("//") {
        Some(hier) => {
            let (authority, path) = match hier.find('/') {
                Some(i) => hier.split_at(i),
                None => (hier, ""),
            };
            // Drop user info and port: apps route on the host name.
            let host = authority.rsplit('@').next().unwrap_or("");
            let host = match host.strip_prefix('[') {
                Some(v6) => v6.split(']').next().unwrap_or(""),
                None => host.split(':').next().unwrap_or(""),
            };
            let host = decode(host, false)?.to_ascii_lowercase();
            ((!host.is_empty()).then_some(host), path)
        }
        None => (None, rest),
    };
    let path = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| decode(segment, false))
        .collect::<Result<Vec<_>, _>>()?;

    let route = if matches!(scheme.as_str(), "http" | "https") {
        path.clone()
    } else {
        host.iter().cloned().chain(path.iter().cloned()).collect()
    };
    Ok(ParsedLink {
        scheme,
        host,
        path,
        query,
        fragment,
        route,
    })
}

fn parse_query(query: &str) -> Result<Vec<(String, String)>, String> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Ok((decode(name, true)?, decode(value, true)?))
        })
        .collect()
}

/// Percent-decodes `text` (and `+` as a space in query parts).
fn decode(text: &str, plus_is_space: bool) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or_else(|| format!("bad percent escape in {text:?}"))?;
                out.push(hex);
                i += 3;
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| format!("{text:?} does not decode to UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn custom_scheme_routes_on_host_and_path() {
        let link = parse("MyApp://Note/42/?tab=edit&q=a+b%20c#top").unwrap();
        assert_eq!(link.scheme, "myapp");
        assert_eq!(link.host.as_deref(), Some("note"));
        assert_eq!(link.path, strings(&["42"]));
        assert_eq!(link.route, strings(&["note", "42"]));
        assert_eq!(
            link.query,
            vec![
                ("tab".to_string(), "edit".to_string()),
                ("q".to_string(), "a b c".to_string())
            ]
        );
        assert_eq!(link.fragment.as_deref(), Some("top"));
    }

    #[test]
    fn universal_link_routes_on_path() {
        let link = parse("https://user@Example.com:8443/note/42%2F7?flag").unwrap();
        assert_eq!(link.host.as_deref(), Some("example.com"));
        assert_eq!(link.path, strings(&["note", "42/7"]));
        assert_eq!(link.route, strings(&["note", "42/7"]));
        assert_eq!(link.query, vec![("flag".to_string(), String::new())]);
        assert_eq!(
            parse("https://[::1]:80/x").unwrap().host.as_deref(),
            Some("::1")
        );
    }

    #[test]
    fn links_without_authority() {
        let link = parse("myapp:settings/privacy").unwrap();
        assert_eq!(link.host, None);
        assert_eq!(link.route, strings(&["settings", "privacy"]));
        let link = parse("myapp://").unwrap();
        assert_eq!(link.host, None);
        assert!(link.route.is_empty());
        let link = parse("myapp:///only/path").unwrap();
        assert_eq!(link.host, None);
        assert_eq!(link.route, strings(&["only", "path"]));
    }

    #[test]
    fn rejects_what_is_not_a_url() {
        for bad in [
            "",
            "no-scheme",
            "1abc://x",
            "my app://x",
            "myapp://x/%zz",
            "myapp://x/%e2%28",
            "myapp://x\n",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(parse(&format!("myapp://{}", "a".repeat(MAX_URL_LEN))).is_err());
    }
}
