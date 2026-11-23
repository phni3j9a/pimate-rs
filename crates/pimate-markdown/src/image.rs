use percent_encoding::percent_decode_str;
use pimate_protocol::MAX_PROMPT_IMAGE_BYTES;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Remote,
    Data,
    File,
}

pub(crate) fn web(s: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .any(|p| s.get(..p.len()).is_some_and(|v| v.eq_ignore_ascii_case(p)))
}
fn svg(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".svg") || p.ends_with(".svgz")
}

/// Classification only: never opens a URL or reads a file.
pub fn classify_image(src: &str) -> Option<ImageKind> {
    if src.is_empty() || src.bytes().any(|b| b < 32) {
        return None;
    }
    if src
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("data:"))
    {
        if src.len() > MAX_PROMPT_IMAGE_BYTES.div_ceil(3) * 4 + 32 {
            return None;
        }
        let (mime, data) = src[5..].split_once(',')?;
        let body = data.trim_end_matches('=');
        let pad = data.len() - body.len();
        return ([
            "image/gif;base64",
            "image/png;base64",
            "image/jpeg;base64",
            "image/webp;base64",
        ]
        .iter()
        .any(|m| mime.eq_ignore_ascii_case(m))
            && !body.is_empty()
            && data.len().is_multiple_of(4)
            && pad <= 2
            && data.len() / 4 * 3 - pad <= MAX_PROMPT_IMAGE_BYTES
            && body
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+/".contains(&c)))
        .then_some(ImageKind::Data);
    }
    if web(src) || src.starts_with("file://") {
        let u = Url::parse(src).ok()?;
        let path = u.path();
        let bytes = path.as_bytes();
        if bytes.iter().enumerate().any(|(i, &b)| {
            b == b'%'
                && !bytes
                    .get(i + 1..i + 3)
                    .is_some_and(|v| v.iter().all(u8::is_ascii_hexdigit))
        }) || svg(&percent_decode_str(path).decode_utf8().ok()?)
        {
            return None;
        }
        return if u.scheme() == "file" {
            (u.host().is_none()
                && u.query().unwrap_or("").is_empty()
                && u.fragment().unwrap_or("").is_empty())
            .then_some(ImageKind::File)
        } else {
            (u.host().is_some() && u.username().is_empty() && u.password().is_none())
                .then_some(ImageKind::Remote)
        };
    }
    let scheme = src.split_once(':').is_some_and(|(s, _)| {
        s.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+.-".contains(&c))
    });
    (!scheme && !src.starts_with("//") && !svg(src)).then_some(ImageKind::File)
}
