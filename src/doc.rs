//! Loading and analysing a single phile: decoding legacy encodings, locating
//! section headings and cross-references to other Phrack issues.

use std::{fs, path::Path, sync::OnceLock};

use regex::Regex;

use crate::data::Library;

const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

pub fn decode(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let lossy = String::from_utf8_lossy(bytes);
    let bad = lossy.chars().filter(|&c| c == '\u{fffd}').count();
    if bad * 200 < lossy.chars().count().max(1) {
        // Mostly UTF-8 with a few stray bytes: keep it, mark the strays.
        return lossy.replace('\u{fffd}', "?");
    }
    let table: Vec<char> = CP437_HIGH.chars().collect();
    bytes
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { table[(b - 0x80) as usize] })
        .collect()
}

fn ansi_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap())
}

/// Strip front-matter, ANSI escapes, control characters; expand tabs.
pub fn body_lines(text: &str) -> Vec<String> {
    let text = ansi_re().replace_all(text, "");
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.first().map(|l| l.trim_end_matches('\r').trim()) == Some("---") {
        if let Some(end) = lines.iter().take(12).skip(1).position(|l| l.trim_end_matches('\r').trim() == "---") {
            lines.drain(..end + 2);
        }
    }
    let mut out: Vec<String> = lines.iter().map(|l| clean_line(l)).collect();
    while out.len() > 1 && out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn clean_line(l: &str) -> String {
    let mut s = String::with_capacity(l.len());
    let mut col = 0usize;
    for c in l.chars() {
        match c {
            '\t' => {
                let n = 8 - col % 8;
                s.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\r' | '\u{feff}' => {}
            c if (c as u32) < 0x20 || c as u32 == 0x7f || ('\u{80}'..='\u{9f}').contains(&c) => {}
            c => {
                s.push(c);
                col += 1;
            }
        }
    }
    s.trim_end().to_string()
}

pub fn load_lines(path: &Path) -> Vec<String> {
    match fs::read(path) {
        Ok(b) => body_lines(&decode(&b)),
        Err(e) => vec![format!("<< cannot read {}: {e} >>", path.display())],
    }
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub row: usize,
    pub title: String,
}

/// A reference to another issue/article found in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ref {
    pub row: usize,
    pub col: usize,
    pub len: usize,
    pub issue: u32,
    pub art: Option<u32>,
}

pub struct Doc {
    pub lines: Vec<Vec<char>>,
    pub headings: Vec<Heading>,
    pub refs: Vec<Ref>,
}

impl Doc {
    pub fn build(lines: Vec<String>, lib: &Library, cur_issue: u32) -> Doc {
        let headings = find_headings(&lines);
        let refs = find_refs(&lines, lib, cur_issue);
        Doc { lines: lines.iter().map(|l| l.chars().collect()).collect(), headings, refs }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map_or(0, Vec::len)
    }

    pub fn line_string(&self, row: usize) -> String {
        self.lines.get(row).map(|l| l.iter().collect()).unwrap_or_default()
    }

    pub fn refs_on(&self, row: usize) -> impl Iterator<Item = &Ref> {
        let start = self.refs.partition_point(|r| r.row < row);
        self.refs[start..].iter().take_while(move |r| r.row == row)
    }
}

fn is_rule(l: &str) -> bool {
    let t = l.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '=') || t.chars().all(|c| c == '~'))
}

pub fn find_headings(lines: &[String]) -> Vec<Heading> {
    static BRACKET: OnceLock<Regex> = OnceLock::new();
    static NUMBERED: OnceLock<Regex> = OnceLock::new();
    let bracket = BRACKET.get_or_init(|| {
        Regex::new(r"^\s*[|=\-]+[=\-]*\[\s*(\S.*?)\s*\][=\-|\s]*$").unwrap()
    });
    let numbered = NUMBERED.get_or_init(|| {
        Regex::new(r"^\s{0,12}(?:\d{1,2}|[IVX]{1,5}|0x[0-9a-fA-F]{1,3})\s*(?:[.)]|\s-|:)\s+([A-Z0-9][^\n]{3,64})$").unwrap()
    });
    let blank = |i: isize| i < 0 || lines.get(i as usize).is_none_or(|l| l.trim().is_empty());
    let mut out: Vec<Heading> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let ii = i as isize;
        let t = l.trim();
        if t.is_empty() || l.len() > 120 {
            continue;
        }
        let title = if let Some(c) = bracket.captures(l) {
            Some(c[1].trim_matches(|c: char| c == '=' || c == '-' || c == '|' || c == ' ').to_string())
        } else if blank(ii - 1) && numbered.is_match(l) && l.chars().filter(|c| c.is_alphabetic()).count() >= 4 {
            Some(t.to_string())
        } else if lines.get(i + 1).is_some_and(|n| is_rule(n))
            && !is_rule(l)
            && t.len() >= 3
            && t.len() <= 70
            && t.len().abs_diff(lines[i + 1].trim().len()) <= 6
        {
            Some(t.to_string())
        } else if blank(ii - 1) && blank(ii + 1) && is_shouting(t) {
            Some(t.to_string())
        } else {
            None
        };
        if let Some(title) = title {
            if title.len() >= 2 && out.last().is_none_or(|h| h.row + 1 < i || h.title != title) {
                out.push(Heading { row: i, title });
            }
        }
    }
    out
}

fn is_shouting(t: &str) -> bool {
    if t.len() < 6 || t.len() > 60 {
        return false;
    }
    let letters = t.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 5
        && t.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
        && letters * 10 >= t.chars().count() * 7
        && t.split_whitespace().count() >= 2
        && !t.contains("PHRACK INC")
}

fn ref_regexes() -> &'static (Regex, Regex, Regex) {
    static R: OnceLock<(Regex, Regex, Regex)> = OnceLock::new();
    R.get_or_init(|| {
        (
            // "Phrack 49, article 3" / "Phrack #49 phile 0x03"
            Regex::new(r"(?i)phrack(?:\s+(?:magazine|inc\.?|issue))*[\s#]*(\d{1,2})\s*[,:;-]\s*(?:article|phile|file|text)\s*#?\s*(0x[0-9a-f]{1,2}|\d{1,2})\b").unwrap(),
            // "Phrack 49", "Phrack #50", "Phrack Magazine issue 7"
            Regex::new(r"(?i)phrack(?:\s+(?:magazine|inc\.?|issue))*[\s#]*(\d{1,2})\b").unwrap(),
            // "p49-0x0e", "P64-12"
            Regex::new(r"\bp(\d{2})[-_](0x[0-9a-fA-F]{2}|\d{1,2})\b").unwrap(),
        )
    })
}

fn parse_num(s: &str) -> Option<u32> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

pub fn find_refs(lines: &[String], lib: &Library, cur_issue: u32) -> Vec<Ref> {
    let (full, plain, short) = ref_regexes();
    let mut out = Vec::new();
    for (row, l) in lines.iter().enumerate() {
        let lower_has = l.len() >= 4 && (l.contains("hrack") || l.contains("HRACK") || l.contains('p'));
        if !lower_has {
            continue;
        }
        let mut taken: Vec<(usize, usize)> = Vec::new();
        let mut found: Vec<(usize, usize, u32, Option<u32>)> = Vec::new();
        for (re, has_art) in [(full, true), (short, true), (plain, false)] {
            for c in re.captures_iter(l) {
                let m = c.get(0).unwrap();
                if taken.iter().any(|&(s, e)| m.start() < e && s < m.end()) {
                    continue;
                }
                let Some(issue) = parse_num(&c[1]) else { continue };
                let art = if has_art { c.get(2).and_then(|a| parse_num(a.as_str())) } else { None };
                let valid = match art {
                    Some(a) => lib.article(issue, a).is_some(),
                    None => lib.issue(issue).is_some(),
                };
                // Pointing at the page you're already on is noise.
                if !valid || (issue == cur_issue && art.is_none()) {
                    continue;
                }
                taken.push((m.start(), m.end()));
                found.push((m.start(), m.end(), issue, art));
            }
        }
        found.sort();
        for (s, e, issue, art) in found {
            let col = l[..s].chars().count();
            let len = l[s..e].chars().count();
            out.push(Ref { row, col, len, issue, art });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Article, Issue};

    fn lib() -> Library {
        let mk = |i: u32, n: u32| Article {
            issue: i,
            num: n,
            title: "t".into(),
            author: "a".into(),
            path: "x".into(),
            size: 0,
        };
        Library {
            issues: vec![
                Issue { num: 7, articles: vec![mk(7, 1)] },
                Issue { num: 49, articles: vec![mk(49, 1), mk(49, 3), mk(49, 14)] },
                Issue { num: 50, articles: vec![mk(50, 1)] },
            ],
        }
    }

    #[test]
    fn frontmatter_stripped() {
        let l = body_lines("---\ntitle: x\n---\n\nhello\t!\n\n\n");
        assert_eq!(l, vec!["", "hello   !"]);
    }

    #[test]
    fn cp437_fallback() {
        let s = decode(&[b'a', 0xC9, 0xCD, 0xBB, 0x80, 0xB0, 0xB0, 0xB0, 0xB0, 0xB0, 0xB0, 0xB0, 0xB0, 0xB0, 0xB0, b'\n']);
        assert!(s.starts_with("a╔═╗Ç"));
    }

    #[test]
    fn headings() {
        let t: Vec<String> = [
            "intro text",
            "",
            "--[ 1 - Introduction ]--",
            "",
            "body",
            "|=-----=[ Hello World ]=-----=|",
            "body",
            "",
            "2. The Second Part",
            "",
            "Underlined",
            "----------",
            "",
            "ALL CAPS TITLE HERE",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let h = find_headings(&t);
        let rows: Vec<usize> = h.iter().map(|h| h.row).collect();
        assert_eq!(rows, vec![2, 5, 8, 10, 13]);
        assert_eq!(h[0].title, "1 - Introduction");
        assert_eq!(h[1].title, "Hello World");
    }

    #[test]
    fn refs() {
        let t: Vec<String> = [
            "as seen in Phrack 49, article 3 and Phrack #50 or p49-0x0e",
            "see Phrack 99 (bogus) and Phrack 7",
            "Phrack Magazine Volume 7, Issue 51 September 01, 1997",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let r = find_refs(&t, &lib(), 1);
        let got: Vec<(usize, u32, Option<u32>)> = r.iter().map(|r| (r.row, r.issue, r.art)).collect();
        assert_eq!(got, vec![(0, 49, Some(3)), (0, 50, None), (0, 49, Some(14)), (1, 7, None)]);
        assert_eq!(r[0].col, 11);
    }
}
