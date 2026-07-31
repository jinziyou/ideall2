//! Bounded HTML page metadata fetcher used to enrich captured bookmarks.
//!
//! The adapter is intentionally small: it downloads at most [`FetchOptions::max_body`]
//! bytes, scans only the `<head>` range, and never executes scripts or follows
//! non-HTTP(S) schemes. Parsing is best-effort; any network or markup problem
//! maps to [`WebMetaError`] so callers can decide how silent to be.

use std::fmt;
use std::io::Read;
use std::time::Duration;

use ureq::ResponseExt;
use url::Url;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_BODY: u64 = 512 * 1024;
const DEFAULT_MAX_REDIRECTS: u32 = 3;
const MAX_TITLE_CHARS: usize = 300;
const MAX_DESCRIPTION_CHARS: usize = 500;

/// Tunables for [`fetch_page_meta_with_options`]; defaults are 5s / 512KB / 3 redirects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FetchOptions {
    pub timeout: Duration,
    pub max_body: u64,
    pub max_redirects: u32,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_body: DEFAULT_MAX_BODY,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }
}

/// Metadata extracted from a bookmarkable page. Every field except
/// `canonical_url` stays `None` when the page does not provide it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageMeta {
    pub canonical_url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub favicon: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebMetaError {
    InvalidUrl,
    UnsupportedScheme(String),
    Network(String),
    Http(u16),
}

impl fmt::Display for WebMetaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => write!(formatter, "invalid page URL"),
            Self::UnsupportedScheme(scheme) => {
                write!(formatter, "unsupported page URL scheme `{scheme}`")
            }
            Self::Network(message) => write!(formatter, "page fetch failed: {message}"),
            Self::Http(status) => write!(formatter, "page returned HTTP {status}"),
        }
    }
}

impl std::error::Error for WebMetaError {}

pub fn fetch_page_meta(url: &str) -> Result<PageMeta, WebMetaError> {
    fetch_page_meta_with_options(url, &FetchOptions::default())
}

pub fn fetch_page_meta_with_options(
    url: &str,
    options: &FetchOptions,
) -> Result<PageMeta, WebMetaError> {
    let parsed = Url::parse(url).map_err(|_| WebMetaError::InvalidUrl)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(WebMetaError::UnsupportedScheme(parsed.scheme().to_owned()));
    }
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(options.timeout))
        .max_redirects(options.max_redirects)
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(parsed.as_str())
        .header("Accept", "text/html,application/xhtml+xml")
        .header(
            "User-Agent",
            "ideall/0.2 (+https://github.com/jinziyou/ideall)",
        )
        .call()
        .map_err(|error| WebMetaError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(WebMetaError::Http(status));
    }
    let final_url = response.get_uri().to_string();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_html =
        content_type.contains("text/html") || content_type.contains("application/xhtml+xml");
    if !is_html {
        return Ok(PageMeta {
            canonical_url: Some(final_url),
            ..PageMeta::default()
        });
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(options.max_body + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| WebMetaError::Network(error.to_string()))?;
    bytes.truncate(options.max_body as usize);
    let html = String::from_utf8_lossy(&bytes);
    Ok(parse_page_meta(&html, &final_url))
}

fn parse_page_meta(html: &str, final_url: &str) -> PageMeta {
    let lower = html.to_ascii_lowercase();
    // 只扫描 <head> 截断范围；缺失 <head>/</head> 时退化为整个有界文档。
    let head_start = lower
        .find("<head")
        .and_then(|index| lower[index..].find('>').map(|close| index + close + 1))
        .unwrap_or(0);
    let head_end = lower[head_start..]
        .find("</head>")
        .map(|index| head_start + index)
        .unwrap_or(html.len());
    let head = &html[head_start..head_end];
    let head_lower = &lower[head_start..head_end];

    let title_tag = find_tag_text(head, head_lower, "title")
        .and_then(|text| clean_text(&text, MAX_TITLE_CHARS));
    let mut og_title = None;
    let mut og_description = None;
    let mut description = None;
    let mut canonical_href = None;
    let mut icon_href = None;

    for tag in find_tags(head, head_lower, "meta") {
        let attributes = parse_tag_attributes(tag);
        let key = attribute_value(&attributes, "property")
            .or_else(|| attribute_value(&attributes, "name"))
            .map(|value| value.to_ascii_lowercase());
        let content = attribute_value(&attributes, "content");
        let (Some(key), Some(content)) = (key, content) else {
            continue;
        };
        match key.as_str() {
            "og:title" if og_title.is_none() => {
                og_title = clean_text(&content, MAX_TITLE_CHARS);
            }
            "og:description" if og_description.is_none() => {
                og_description = clean_text(&content, MAX_DESCRIPTION_CHARS);
            }
            "description" if description.is_none() => {
                description = clean_text(&content, MAX_DESCRIPTION_CHARS);
            }
            _ => {}
        }
    }
    for tag in find_tags(head, head_lower, "link") {
        let attributes = parse_tag_attributes(tag);
        let rel = attribute_value(&attributes, "rel").map(|value| value.to_ascii_lowercase());
        let href = attribute_value(&attributes, "href");
        let (Some(rel), Some(href)) = (rel, href) else {
            continue;
        };
        match rel.as_str() {
            "canonical" if canonical_href.is_none() => canonical_href = Some(href),
            "icon" | "shortcut icon" | "apple-touch-icon" if icon_href.is_none() => {
                icon_href = Some(href);
            }
            _ => {}
        }
    }

    PageMeta {
        canonical_url: canonical_href
            .and_then(|href| resolve_url(final_url, &href))
            .or_else(|| Some(final_url.to_owned())),
        title: og_title.or(title_tag),
        description: og_description.or(description),
        favicon: icon_href.and_then(|href| resolve_url(final_url, &href)),
    }
}

/// Returns the raw text of the first `<name>...</name>` element in `head`.
fn find_tag_text(head: &str, head_lower: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let start = head_lower.find(&open)?;
    let open_end = head_lower[start..]
        .find('>')
        .map(|close| start + close + 1)?;
    let close = format!("</{name}>");
    let end = head_lower[open_end..]
        .find(&close)
        .map(|index| open_end + index)?;
    (end > open_end).then(|| head[open_end..end].to_string())
}

/// Collects every `<name ...>` tag slice (up to and including `>`) in `head`.
fn find_tags<'a>(head: &'a str, head_lower: &str, name: &str) -> Vec<&'a str> {
    let needle = format!("<{name}");
    let mut tags = Vec::new();
    let mut from = 0;
    while let Some(position) = head_lower[from..].find(&needle) {
        let start = from + position;
        let after = start + needle.len();
        // 排除 <metadata> 这类同前缀标签名。
        if head_lower[after..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        {
            from = after;
            continue;
        }
        let Some(close) = head_lower[start..].find('>') else {
            break;
        };
        tags.push(&head[start..start + close + 1]);
        from = start + close + 1;
    }
    tags
}

fn parse_tag_attributes(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut attributes = Vec::new();
    let mut index = 0;
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'>' {
        index += 1;
    }
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'>' {
            break;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'=' | b'>' | b'/')
        {
            index += 1;
        }
        let name = tag[name_start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index < bytes.len() && matches!(bytes[index], b'"' | b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = tag[value_start..index].to_owned();
                index += 1;
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = tag[value_start..index].to_owned();
            }
        }
        if !name.is_empty() {
            attributes.push((name, value));
        }
    }
    attributes
}

fn attribute_value(attributes: &[(String, String)], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

fn resolve_url(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    Url::parse(base)
        .ok()?
        .join(href)
        .ok()
        .map(|url| url.to_string())
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find('&') {
        decoded.push_str(&rest[..position]);
        rest = &rest[position..];
        let (tail, replacement) = if let Some(tail) = rest.strip_prefix("&amp;") {
            (tail, '&')
        } else if let Some(tail) = rest.strip_prefix("&lt;") {
            (tail, '<')
        } else if let Some(tail) = rest.strip_prefix("&gt;") {
            (tail, '>')
        } else if let Some(tail) = rest.strip_prefix("&quot;") {
            (tail, '"')
        } else if let Some(tail) = rest.strip_prefix("&#39;") {
            (tail, '\'')
        } else if let Some(tail) = rest.strip_prefix("&nbsp;") {
            (tail, '\u{a0}')
        } else {
            decoded.push('&');
            rest = &rest[1..];
            continue;
        };
        decoded.push(replacement);
        rest = tail;
    }
    decoded.push_str(rest);
    decoded
}

fn clean_text(text: &str, max_chars: usize) -> Option<String> {
    let collapsed = decode_entities(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(max_chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::thread;

    /// 在 127.0.0.1 上按连接顺序回放预置响应的微型 HTTP server。
    struct TestServer {
        base_url: String,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn serve(responses: Vec<String>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let handle = thread::spawn(move || {
                for response in responses {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                    let mut buffer = [0_u8; 4096];
                    let mut request = Vec::new();
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        match stream.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(read) => request.extend_from_slice(&buffer[..read]),
                        }
                    }
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            });
            Self {
                base_url: format!("http://{address}"),
                handle: Some(handle),
            }
        }

        fn serve_with_delay(responses: Vec<String>, delay: Duration) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let handle = thread::spawn(move || {
                for response in responses {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    thread::sleep(delay);
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            });
            Self {
                base_url: format!("http://{address}"),
                handle: Some(handle),
            }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn html_response(content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn parses_complete_head_with_canonical_priority() {
        let body = concat!(
            "<html><head>",
            "<title>页面标题</title>",
            "<meta property=\"og:title\" content=\"OG 标题\">",
            "<meta name=\"description\" content=\"标签摘要\">",
            "<meta property=\"og:description\" content=\"OG 摘要\">",
            "<link rel=\"canonical\" href=\"https://example.com/canonical\">",
            "<link rel=\"icon\" href=\"/favicon.ico\">",
            "</head><body>正文</body></html>"
        );
        let server = TestServer::serve(vec![html_response("text/html; charset=utf-8", body)]);

        let meta = fetch_page_meta(&format!("{}/page", server.base_url)).unwrap();
        assert_eq!(
            meta.canonical_url.as_deref(),
            Some("https://example.com/canonical")
        );
        assert_eq!(meta.title.as_deref(), Some("OG 标题"));
        assert_eq!(meta.description.as_deref(), Some("OG 摘要"));
        assert_eq!(
            meta.favicon.as_deref(),
            Some(format!("{}/favicon.ico", server.base_url).as_str())
        );
    }

    #[test]
    fn falls_back_to_title_tag_and_plain_description() {
        let body = concat!(
            "<html><head>",
            "<title>  多  空白   标题 </title>",
            "<meta name=\"description\" content=\"普通摘要\">",
            "</head></html>"
        );
        let server = TestServer::serve(vec![html_response("text/html", body)]);

        let meta = fetch_page_meta(&server.base_url).unwrap();
        assert_eq!(meta.title.as_deref(), Some("多 空白 标题"));
        assert_eq!(meta.description.as_deref(), Some("普通摘要"));
        assert_eq!(
            meta.canonical_url.as_deref(),
            Some(format!("{}/", server.base_url).as_str())
        );
    }

    #[test]
    fn follows_redirect_chain_and_reports_final_url() {
        let final_body = "<html><head><title>最终页</title></head></html>";
        let server = TestServer::serve(vec![
            "HTTP/1.1 302 Found\r\nLocation: /step2\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            html_response("text/html", final_body),
        ]);

        let meta = fetch_page_meta(&format!("{}/start", server.base_url)).unwrap();
        assert_eq!(meta.title.as_deref(), Some("最终页"));
        assert_eq!(
            meta.canonical_url.as_deref(),
            Some(format!("{}/final", server.base_url).as_str())
        );
    }

    #[test]
    fn resolves_relative_favicon_against_final_url() {
        let body = concat!(
            "<html><head>",
            "<link rel=\"apple-touch-icon\" href=\"../assets/touch.png\">",
            "</head></html>"
        );
        let server = TestServer::serve(vec![html_response("text/html", body)]);

        let meta = fetch_page_meta(&format!("{}/a/b/page", server.base_url)).unwrap();
        assert_eq!(
            meta.favicon.as_deref(),
            Some(format!("{}/a/assets/touch.png", server.base_url).as_str())
        );
    }

    #[test]
    fn non_html_responses_only_report_the_final_url() {
        let server = TestServer::serve(vec![html_response("image/png", "\u{89}PNG-binary")]);

        let meta = fetch_page_meta(&format!("{}/icon.png", server.base_url)).unwrap();
        assert_eq!(
            meta.canonical_url.as_deref(),
            Some(format!("{}/icon.png", server.base_url).as_str())
        );
        assert_eq!(meta.title, None);
        assert_eq!(meta.description, None);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn malformed_html_without_head_is_best_effort() {
        let body = "<title>散落标题</title><p>没有 head 的文档<meta name=\"description\" content=\"散落摘要\">";
        let server = TestServer::serve(vec![html_response("text/html", body)]);

        let meta = fetch_page_meta(&server.base_url).unwrap();
        assert_eq!(meta.title.as_deref(), Some("散落标题"));
        assert_eq!(meta.description.as_deref(), Some("散落摘要"));
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn decodes_html_entities_in_title_and_description() {
        let body = concat!(
            "<html><head>",
            "<title>A &amp; B &lt;x&gt; &quot;q&quot; &#39;s&nbsp;end</title>",
            "<meta name=\"description\" content=\"Tom &amp; Jerry &lt;3\">",
            "</head></html>"
        );
        let server = TestServer::serve(vec![html_response("text/html", body)]);

        let meta = fetch_page_meta(&server.base_url).unwrap();
        assert_eq!(meta.title.as_deref(), Some("A & B <x> \"q\" 's end"));
        assert_eq!(meta.description.as_deref(), Some("Tom & Jerry <3"));
    }

    #[test]
    fn times_out_when_the_server_stalls() {
        let server = TestServer::serve_with_delay(
            vec![html_response("text/html", "<title>慢</title>")],
            Duration::from_millis(500),
        );
        let options = FetchOptions {
            timeout: Duration::from_millis(100),
            ..FetchOptions::default()
        };

        let result = fetch_page_meta_with_options(&server.base_url, &options);
        assert!(matches!(result, Err(WebMetaError::Network(_))));
    }

    #[test]
    fn rejects_non_http_schemes_and_unparseable_urls() {
        assert!(matches!(
            fetch_page_meta("ftp://example.com/file"),
            Err(WebMetaError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            fetch_page_meta("not a url"),
            Err(WebMetaError::InvalidUrl)
        ));
    }
}
