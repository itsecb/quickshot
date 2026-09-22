//! Find sensitive text in OCR output (IPs, emails, hostnames, secrets, …) and map each hit to a
//! pixel rectangle so the editor can pixelate it. Pure logic: no I/O, fully unit-tested.

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use crate::geom::Rect;
use crate::ocr::OcrLine;

/// Extra pixels around each hit so anti-aliased glyph edges are covered too.
const PAD: i32 = 3;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RedactMatch {
    pub kind: &'static str,
    pub text: String,
    pub rect: Rect,
}

/// Byte range inside one OCR line.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
}

fn detectors() -> &'static [(&'static str, Regex)] {
    static DETECTORS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    DETECTORS.get_or_init(|| {
        [
            // order matters when hits overlap: the earlier detector names the merged hit
            ("private key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
            ("credentials", r"(?i)\b[a-z][a-z0-9+.\-]*://(?P<v>[^\s:/@]+:[^\s@/]+)@"),
            (
                "secret",
                r#"(?i)\b(?:password|passwd|pwd|passphrase|secret|client[_ -]?secret|token|api[_ -]?key|apikey|access[_ -]?key|auth[_ -]?token|private[_ -]?key)\s*[:=]\s*(?P<v>[^\s;,'"]+)"#,
            ),
            ("AWS key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
            ("JWT", r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}"),
            ("email", r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b"),
            ("GUID", r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b"),
            ("SID", r"\bS-1-5-21(?:-\d+){3,4}\b"),
            (
                "MAC",
                r"(?i)\b(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}\b|\b[0-9a-f]{4}\.[0-9a-f]{4}\.[0-9a-f]{4}\b",
            ),
            (
                "IP",
                r"\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(?:/\d{1,2})?\b",
            ),
            (
                "IPv6",
                r"(?i)(?:^|[^0-9a-z:])(?P<v>[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7}(?:%[0-9a-z]+)?)",
            ),
            (
                "hostname",
                r"(?i)\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,24}\b",
            ),
            ("token", r"[A-Za-z0-9_\-]{24,}|[A-Za-z0-9+/]{30,}={0,2}"),
        ]
        .into_iter()
        .map(|(kind, re)| (kind, Regex::new(re).expect("built-in redact pattern")))
        .collect()
    })
}

const INTERNAL_TLDS: &[&str] = &[
    "local",
    "lan",
    "corp",
    "internal",
    "intra",
    "intranet",
    "home",
    "ad",
    "localdomain",
    "arpa",
    "private",
];

const FILE_EXTENSIONS: &[&str] = &[
    "exe", "dll", "sys", "msi", "txt", "log", "json", "xml", "yml", "yaml", "ini", "cfg", "conf",
    "config", "toml", "lock", "ps1", "psm1", "sh", "bat", "cmd", "vbs", "reg", "py", "rs", "js",
    "ts", "css", "html", "htm", "md", "csv", "sql", "db", "mdf", "ldf", "bak", "old", "tmp", "png",
    "jpg", "jpeg", "gif", "svg", "ico", "bmp", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "zip", "gz", "tar", "rar", "iso", "vhd", "vhdx", "vmdk", "cer", "crt", "pem", "pfx", "key",
    "jar", "war", "so", "mp4", "net",
];

/// Dotted names worth hiding: internal TLDs, or host.domain.tld (not bare example.com / file.txt).
fn plausible_host(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let labels: Vec<&str> = lower.split('.').collect();
    let Some(tld) = labels.last() else {
        return false;
    };
    if labels.iter().any(|l| l.is_empty()) || FILE_EXTENSIONS.contains(tld) {
        return false;
    }
    INTERNAL_TLDS.contains(tld) || (labels.len() >= 3 && labels[0] != "www")
}

/// Colon-separated hex that is an IPv6 address rather than a clock time or a label.
fn plausible_ipv6(s: &str) -> bool {
    let addr = s.split('%').next().unwrap_or(s);
    let groups: Vec<&str> = addr.split(':').collect();
    let hex_digits = addr.chars().filter(|c| c.is_ascii_hexdigit()).count();
    let has_letter = addr.chars().any(|c| c.is_ascii_alphabetic());
    hex_digits >= 3 && (addr.contains("::") || has_letter || groups.len() >= 5)
}

fn entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

/// Long, random-looking strings: needs upper, lower and digits, and high entropy.
fn looks_like_secret(s: &str) -> bool {
    let upper = s.chars().any(|c| c.is_ascii_uppercase());
    let lower = s.chars().any(|c| c.is_ascii_lowercase());
    let digit = s.chars().any(|c| c.is_ascii_digit());
    upper && lower && digit && entropy(s) >= 3.5
}

/// A user pattern: `re:<regex>`, or a literal (case-insensitive) that redacts the whole token
/// containing it, e.g. `contoso.com` hides `srv01.corp.contoso.com`.
enum Custom {
    Regex(Regex),
    Literal(String),
}

fn parse_custom(patterns: &[String]) -> Vec<Custom> {
    patterns
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .filter_map(|p| match p.strip_prefix("re:") {
            Some(re) => match Regex::new(re) {
                Ok(r) => Some(Custom::Regex(r)),
                Err(e) => {
                    log::warn!("ignoring invalid redact pattern {p:?}: {e}");
                    None
                }
            },
            None => Some(Custom::Literal(p.to_ascii_lowercase())),
        })
        .collect()
}

fn is_token_char(c: char) -> bool {
    !(c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';'
        ))
}

fn find_spans(text: &str, custom: &[Custom]) -> Vec<Span> {
    let mut spans = Vec::new();
    for (kind, re) in detectors() {
        for caps in re.captures_iter(text) {
            let Some(m) = caps.name("v").or_else(|| caps.get(0)) else {
                continue;
            };
            let s = m.as_str();
            let keep = match *kind {
                "hostname" => plausible_host(s),
                "IPv6" => plausible_ipv6(s),
                "token" => looks_like_secret(s),
                _ => true,
            };
            if keep && !s.is_empty() {
                spans.push(Span {
                    start: m.start(),
                    end: m.end(),
                    kind,
                });
            }
        }
    }
    // ASCII lower-casing keeps byte offsets identical to `text`
    let lower = text.to_ascii_lowercase();
    for c in custom {
        match c {
            Custom::Regex(re) => spans.extend(re.find_iter(text).map(|m| Span {
                start: m.start(),
                end: m.end(),
                kind: "custom",
            })),
            Custom::Literal(lit) => {
                for (at, _) in lower.match_indices(lit.as_str()) {
                    let start = text[..at]
                        .char_indices()
                        .rev()
                        .find(|(_, ch)| !is_token_char(*ch))
                        .map(|(i, ch)| i + ch.len_utf8())
                        .unwrap_or(0);
                    let end = text[at..]
                        .char_indices()
                        .find(|(_, ch)| !is_token_char(*ch))
                        .map(|(i, _)| at + i)
                        .unwrap_or(text.len());
                    spans.push(Span {
                        start,
                        end,
                        kind: "custom",
                    });
                }
            }
        }
    }
    merge(spans)
}

/// Merge overlapping spans; the one that starts first (then the earlier detector) names the result.
fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by_key(|s| s.start);
    let mut out: Vec<Span> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(last) if s.start < last.end => last.end = last.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

fn chars_in(text: &str, from: usize, to: usize) -> usize {
    text[from..to].chars().count()
}

/// Horizontal slice of `bbox` covering characters `[a, b)` of `n`.
fn slice(bbox: Rect, a: usize, b: usize, n: usize) -> Rect {
    let n = n.max(1) as f64;
    let w = bbox.width as f64;
    let x0 = bbox.x + (w * a as f64 / n).floor() as i32;
    let x1 = bbox.x + (w * b as f64 / n).ceil() as i32;
    Rect::new(x0, bbox.y, (x1 - x0).max(1) as u32, bbox.height)
}

/// Pixel rect for a byte span: from word boxes when the engine gave them, else a proportional
/// slice of the line box (good for monospace consoles).
fn span_rect(line: &OcrLine, span: Span) -> Option<Rect> {
    let text = line.text.as_str();
    let mut placed = Vec::with_capacity(line.words.len());
    let mut cursor = 0;
    for w in &line.words {
        match text[cursor..].find(w.text.as_str()) {
            Some(pos) if !w.text.is_empty() => {
                let start = cursor + pos;
                let end = start + w.text.len();
                placed.push((start, end, w));
                cursor = end;
            }
            _ => {
                placed.clear();
                break;
            }
        }
    }
    let rect = if placed.is_empty() {
        slice(
            line.bbox,
            chars_in(text, 0, span.start),
            chars_in(text, 0, span.end),
            text.chars().count(),
        )
    } else {
        let parts = placed
            .iter()
            .filter(|(s, e, _)| *s < span.end && span.start < *e)
            .map(|(s, e, w)| {
                let from = span.start.max(*s);
                let to = span.end.min(*e);
                slice(
                    w.bbox,
                    chars_in(text, *s, from),
                    chars_in(text, *s, to),
                    chars_in(text, *s, *e),
                )
            });
        Rect::union_all(parts)?
    };
    if rect.is_empty() {
        return None;
    }
    Some(Rect::new(
        rect.x - PAD,
        rect.y - PAD,
        rect.width + 2 * PAD as u32,
        rect.height + 2 * PAD as u32,
    ))
}

/// Pixelate `rect` in place with `block`-sized squares (for captures that never reach the
/// editor, e.g. a rule that auto-redacts and copies). Clipped to the image.
pub fn pixelate(img: &mut image::RgbaImage, rect: Rect, block: u32) {
    let block = block.max(2);
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.right().max(0) as u32).min(img.width());
    let y1 = (rect.bottom().max(0) as u32).min(img.height());
    let mut by = y0;
    while by < y1 {
        let bh = block.min(y1 - by);
        let mut bx = x0;
        while bx < x1 {
            let bw = block.min(x1 - bx);
            let mut sum = [0u32; 4];
            for y in by..by + bh {
                for x in bx..bx + bw {
                    for (s, c) in sum.iter_mut().zip(img.get_pixel(x, y).0) {
                        *s += c as u32;
                    }
                }
            }
            let n = bw * bh;
            let avg = image::Rgba(sum.map(|s| (s / n) as u8));
            for y in by..by + bh {
                for x in bx..bx + bw {
                    img.put_pixel(x, y, avg);
                }
            }
            bx += bw;
        }
        by += bh;
    }
}

/// Every sensitive-looking hit in `lines`, as pixel rects in the same space as the OCR input.
pub fn find_sensitive(lines: &[OcrLine], custom_patterns: &[String]) -> Vec<RedactMatch> {
    let custom = parse_custom(custom_patterns);
    let mut out = Vec::new();
    for line in lines {
        for span in find_spans(&line.text, &custom) {
            if let Some(rect) = span_rect(line, span) {
                out.push(RedactMatch {
                    kind: span.kind,
                    text: line.text[span.start..span.end].to_string(),
                    rect,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::OcrWord;

    fn hits(text: &str) -> Vec<(&'static str, String)> {
        find_spans(text, &[])
            .into_iter()
            .map(|s| (s.kind, text[s.start..s.end].to_string()))
            .collect()
    }

    fn texts(text: &str) -> Vec<String> {
        hits(text).into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn finds_network_identifiers() {
        assert_eq!(
            hits("Server 10.20.30.40 is up"),
            vec![("IP", "10.20.30.40".to_string())]
        );
        assert_eq!(texts("route 192.168.0.0/16 via"), vec!["192.168.0.0/16"]);
        assert_eq!(
            texts("Physical Address. . . : 00-1A-2B-3C-4D-5E"),
            vec!["00-1A-2B-3C-4D-5E"]
        );
        assert_eq!(
            texts("gw fe80::1c2d:3e4f%12 ok"),
            vec!["fe80::1c2d:3e4f%12"]
        );
        assert_eq!(hits("backup at 10:30:00 done"), vec![]);
        assert_eq!(hits("version 1.2.3 released"), vec![]);
    }

    #[test]
    fn finds_people_and_ids() {
        assert_eq!(
            hits("mail jane.doe@contoso.com now"),
            vec![("email", "jane.doe@contoso.com".to_string())]
        );
        assert_eq!(
            texts("id 3f2504e0-4f89-11d3-9a0c-0305e82c3301"),
            vec!["3f2504e0-4f89-11d3-9a0c-0305e82c3301"]
        );
        assert_eq!(
            texts("owner S-1-5-21-3623811015-3361044348-30300820-1013"),
            vec!["S-1-5-21-3623811015-3361044348-30300820-1013"]
        );
    }

    #[test]
    fn hostnames_skip_files_and_bare_domains() {
        assert_eq!(
            texts("ping srv01.corp.contoso.com"),
            vec!["srv01.corp.contoso.com"]
        );
        assert_eq!(texts("join dc01.contoso.local"), vec!["dc01.contoso.local"]);
        assert_eq!(hits("open notes.txt and example.com"), vec![]);
        assert_eq!(hits("see www.microsoft.com"), vec![]);
    }

    #[test]
    fn secrets_redact_only_the_value() {
        assert_eq!(
            hits("password: Hunter2!"),
            vec![("secret", "Hunter2!".to_string())]
        );
        assert_eq!(
            texts("Server=db;User Id=sa;Password=S3cr3t;"),
            vec!["S3cr3t"]
        );
        assert_eq!(
            hits("https://admin:pa55@host/x"),
            vec![("credentials", "admin:pa55".to_string())]
        );
        assert_eq!(
            texts("key AKIAIOSFODNN7EXAMPLE end"),
            vec!["AKIAIOSFODNN7EXAMPLE"]
        );
        assert_eq!(
            texts("Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N"),
            vec!["eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N"]
        );
        assert_eq!(
            texts("AccountKey x9Qm2LkP7vR4tW8zB1nC5dF3gH6jK0sA"),
            vec!["x9Qm2LkP7vR4tW8zB1nC5dF3gH6jK0sA"]
        );
        // long but not random: path fragments and plain words stay
        assert_eq!(
            hits("C:\\ProgramData\\Microsoft\\Windows\\Start Menu"),
            vec![]
        );
        assert_eq!(hits("abcdefghijklmnopqrstuvwxyz"), vec![]);
    }

    #[test]
    fn custom_patterns() {
        let custom = parse_custom(&["SRV-".into(), "re:INC\\d{6}".into(), "re:(".into()]);
        let text = "Connected to srv-dc01, ticket INC004211";
        let found: Vec<String> = find_spans(text, &custom)
            .into_iter()
            .map(|s| text[s.start..s.end].to_string())
            .collect();
        assert_eq!(found, vec!["srv-dc01", "INC004211"]);
    }

    #[test]
    fn rect_from_word_boxes_covers_only_the_hit() {
        let line = OcrLine {
            text: "IP 10.0.0.1 up".into(),
            bbox: Rect::new(0, 0, 140, 20),
            words: vec![
                OcrWord {
                    text: "IP".into(),
                    bbox: Rect::new(0, 0, 20, 20),
                },
                OcrWord {
                    text: "10.0.0.1".into(),
                    bbox: Rect::new(30, 0, 80, 20),
                },
                OcrWord {
                    text: "up".into(),
                    bbox: Rect::new(120, 0, 20, 20),
                },
            ],
        };
        let m = find_sensitive(&[line], &[]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].rect, Rect::new(27, -3, 86, 26));
    }

    #[test]
    fn rect_falls_back_to_proportional_slice() {
        // 10 chars over 100 px, hit is chars 5..9 ("pw=x" value "abcd")
        let line = OcrLine {
            text: "pwd: abcd".into(),
            bbox: Rect::new(0, 0, 90, 10),
            words: vec![],
        };
        let m = find_sensitive(&[line], &[]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].text, "abcd");
        assert_eq!(
            m[0].rect,
            Rect::new(50 - PAD, -PAD, 40 + 2 * PAD as u32, 16)
        );
    }

    #[test]
    fn pixelate_averages_blocks_and_clips() {
        let mut img = image::RgbaImage::from_fn(8, 4, |x, _| {
            image::Rgba(if x % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [200, 100, 50, 255]
            })
        });
        pixelate(&mut img, Rect::new(-2, 0, 6, 4), 2);
        // inside: 2x2 blocks averaged
        assert_eq!(img.get_pixel(0, 0).0, [100, 50, 25, 255]);
        assert_eq!(img.get_pixel(3, 3).0, [100, 50, 25, 255]);
        // outside the rect: untouched
        assert_eq!(img.get_pixel(4, 0).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(5, 0).0, [200, 100, 50, 255]);
    }

    #[test]
    fn overlapping_hits_merge() {
        // an email inside a key/value: one redaction, named by the first detector
        let found = hits("token=a.b@c.com");
        assert_eq!(found.len(), 1);
    }
}
