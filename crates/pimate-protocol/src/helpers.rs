use crate::*;
use std::collections::HashMap;

pub const PROTOCOL_VERSION: u64 = 1;
pub const HOST_PROTOCOL_VERSION: u64 = 1;
pub const DEFAULT_HOST_PORT: u16 = 47800;
#[repr(u16)]
pub enum HostCloseCode {
    AuthFailed = 4401,
    AuthTimeout = 4408,
    Protocol = 4400,
    Revoked = 4403,
}
pub const AUTH_PROVIDER_IDS: [AuthProviderId; 5] = [
    AuthProviderId::Openai,
    AuthProviderId::OpenaiCodex,
    AuthProviderId::Anthropic,
    AuthProviderId::Opencode,
    AuthProviderId::OpencodeGo,
];
pub const AUTH_MAX_WAIT_MS: u64 = 25_000;
pub const AUTH_FLOW_TTL_MS: u64 = 15 * 60 * 1000;
pub const AUTH_MAX_INPUT_LENGTH: usize = 8192;
pub const MAX_PROMPT_IMAGES: usize = 4;
pub const MAX_PROMPT_IMAGE_BYTES: usize = 512 * 1024;
pub const MAX_PROMPT_TOTAL_IMAGE_BYTES: usize = 600 * 1024;
/// UTF-16 code units, not UTF-8 bytes.
pub const MAX_IMAGE_PROMPT_TEXT: usize = 32 * 1024;

pub fn valid_prompt_images(value: &Value) -> bool {
    let Some(images) = value.as_array() else {
        return false;
    };
    let mut total = 0;
    images.len() <= MAX_PROMPT_IMAGES
        && images.iter().all(|i| {
            if i.get("type").and_then(Value::as_str) != Some("image")
                || !matches!(
                    i.get("mimeType").and_then(Value::as_str),
                    Some("image/jpeg" | "image/png" | "image/webp")
                )
            {
                return false;
            }
            let Some(data) = i.get("data").and_then(Value::as_str) else {
                return false;
            };
            if data.is_empty()
                || data.len() > MAX_PROMPT_IMAGE_BYTES.div_ceil(3) * 4
                || !data.len().is_multiple_of(4)
            {
                return false;
            }
            let raw = data.trim_end_matches('=');
            let pad = data.len() - raw.len();
            if pad > 2
                || !raw
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
            {
                return false;
            }
            let bytes = data.len() / 4 * 3 - pad;
            total += bytes;
            bytes <= MAX_PROMPT_IMAGE_BYTES && total <= MAX_PROMPT_TOTAL_IMAGE_BYTES
        })
}
pub fn same_prompt_images(a: &[PromptImage], b: &[PromptImage]) -> bool {
    a == b
}
pub fn is_subagent_active(state: SubagentState) -> bool {
    use SubagentState::*;
    matches!(state, Queued | Running | Waiting | Stopping)
}
pub fn count_subagents(runs: &[SubagentRun]) -> SubagentCounts {
    let mut counts = SubagentCounts {
        active: 0,
        waiting: 0,
        paused: 0,
        failed: 0,
    };
    for run in runs {
        for s in run
            .children
            .iter()
            .map(|c| c.state)
            .chain(run.children.is_empty().then_some(run.state))
        {
            counts.active += u64::from(is_subagent_active(s));
            match s {
                SubagentState::Waiting => counts.waiting += 1,
                SubagentState::Paused => counts.paused += 1,
                SubagentState::Failed => counts.failed += 1,
                _ => {}
            }
        }
    }
    counts
}
// ECMAScript whitespace excludes NEXT LINE and includes BOM.
fn js_space(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}
pub fn slash_command_name(text: &str) -> Option<&str> {
    let name = text.split(js_space).next()?.strip_prefix('/')?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:.-".contains(&b)))
    .then_some(name)
}

fn encode_component(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char)
        } else {
            write!(out, "%{b:02X}").unwrap()
        }
    }
    out
}
fn decode_component(s: &str) -> Option<String> {
    let mut bytes = s.bytes();
    let mut out = Vec::new();
    while let Some(b) = bytes.next() {
        out.push(if b == b'%' {
            let a = (bytes.next()? as char).to_digit(16)?;
            let b = (bytes.next()? as char).to_digit(16)?;
            (a * 16 + b) as u8
        } else {
            b
        })
    }
    String::from_utf8(out).ok()
}
pub fn format_pairing_link(l: &PairingLink) -> String {
    format!(
        "pimate://pair?h={}&c={}&n={}&id={}",
        encode_component(&l.address),
        encode_component(&l.code),
        encode_component(&l.host_name),
        encode_component(&l.host_id)
    )
}
pub fn parse_pairing_link(s: &str) -> Option<PairingLink> {
    let q = s.trim_matches(js_space).strip_prefix("pimate://pair?")?;
    if q.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let mut fields = HashMap::new();
    for part in q.split('&') {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        if let (Some(k), Some(v)) = (decode_component(k), decode_component(&v.replace('+', " "))) {
            fields.insert(k, v);
        }
    }
    let address = fields.remove("h")?;
    let code = fields.remove("c")?;
    let host_id = fields.remove("id")?;
    let (host, port) = address.rsplit_once(':')?;
    let valid_host = if let Some(ip) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        !ip.is_empty()
            && ip
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
    } else {
        !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    };
    if !valid_host
        || !(1..=5).contains(&port.len())
        || !port.bytes().all(|b| b.is_ascii_digit())
        || host_id.is_empty()
        || code.len() < 16
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let host_name = fields
        .remove("n")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| address.clone());
    Some(PairingLink {
        address,
        code,
        host_id,
        host_name,
    })
}

/// max_line bounds the unfinished tail, in UTF-16 units as in the TS decoder.
pub struct LineDecoder {
    buf: String,
    max_line: usize,
    units: usize,
}
impl Default for LineDecoder {
    fn default() -> Self {
        Self::new(16 * 1024 * 1024)
    }
}
impl LineDecoder {
    pub fn new(max_line: usize) -> Self {
        Self {
            buf: String::new(),
            max_line,
            units: 0,
        }
    }
    pub fn push(&mut self, chunk: &str, mut receive: impl FnMut(Result<Value, &str>)) {
        let mut lines = chunk.split('\n');
        let mut part = lines.next().unwrap();
        for next in lines {
            self.buf.push_str(part);
            if !self.buf.trim_matches(js_space).is_empty() {
                receive(decode_json(&self.buf).map_err(|_| self.buf.as_str()))
            }
            self.buf.clear();
            self.units = 0;
            part = next
        }
        self.buf.push_str(part);
        self.units += part.encode_utf16().count();
        if self.units > self.max_line {
            // JS slices the error preview at 200 UTF-16 units. Stop before a split surrogate.
            let mut units = 0;
            let end = self
                .buf
                .char_indices()
                .find_map(|(i, c)| {
                    units += c.len_utf16();
                    (units > 200).then_some(i)
                })
                .unwrap_or(self.buf.len());
            receive(Err(&self.buf[..end]));
            self.buf = String::new();
            self.units = 0;
        }
    }
}
pub fn encode_line(value: &impl Serialize) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

/// JSON.parse accepts lone UTF-16 surrogates in string values and keys. Rust strings
/// cannot hold them; replace only those escapes, never valid pairs or literal backslashes.
pub fn decode_json(text: &str) -> serde_json::Result<Value> {
    serde_json::from_str(text).or_else(|err| {
        let bytes = text.as_bytes();
        let unit = |i: usize| -> Option<u16> {
            let hex = bytes.get(i..i + 6)?.strip_prefix(b"\\u")?;
            hex.iter().try_fold(0u16, |acc, &b| {
                Some(acc * 16 + (b as char).to_digit(16)? as u16)
            })
        };
        let mut fixed = String::new();
        let mut start = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'\\' {
                i += 1;
                continue;
            }
            match unit(i) {
                Some(0xd800..=0xdbff) if matches!(unit(i + 6), Some(0xdc00..=0xdfff)) => i += 12,
                Some(0xd800..=0xdfff) => {
                    fixed.push_str(&text[start..i]);
                    fixed.push('\u{fffd}');
                    i += 6;
                    start = i;
                }
                _ => i += 2,
            }
        }
        if start == 0 {
            return Err(err);
        }
        fixed.push_str(&text[start..]);
        serde_json::from_str(&fixed)
    })
}
