//! Cursor, motions, search and visual selection for an open phile.

use regex::Regex;
use unicode_width::UnicodeWidthChar;

use crate::doc::{Doc, Ref};

pub struct Search {
    pub pat: String,
    pub re: Regex,
    pub fwd: bool,
}

impl Search {
    pub fn new(pat: &str, fwd: bool) -> Search {
        Search { pat: pat.to_string(), re: build_regex(pat), fwd }
    }
}

/// Smart-case regex; falls back to a literal match when the pattern is invalid.
pub fn build_regex(pat: &str) -> Regex {
    let ci = !pat.chars().any(|c| c.is_uppercase());
    let mk = |p: &str| regex::RegexBuilder::new(p).case_insensitive(ci).build();
    mk(pat).or_else(|_| mk(&regex::escape(pat))).unwrap()
}

/// A saved snippet's anchored range, ready to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub id: u32,
    pub line: bool,
    pub s: (usize, usize),
    pub e: (usize, usize),
}

impl Span {
    pub fn touches_row(&self, row: usize) -> bool {
        row >= self.s.0 && row <= self.e.0
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        if !self.touches_row(row) {
            return false;
        }
        self.line || ((row, col) >= self.s && (row, col) <= self.e)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VMode {
    Char,
    Line,
}

pub struct Reader {
    pub issue: u32,
    pub art: u32,
    pub doc: Doc,
    pub row: usize,
    pub col: usize,
    pub want: usize,
    pub top: usize,
    pub hx: usize,
    pub width: usize,
    pub search: Option<Search>,
    pub hl: bool,
    pub visual: Option<(VMode, (usize, usize))>,
    pub hints: Option<Vec<(Ref, char)>>,
    pub snips: Vec<Span>,
    /// (row, bookmark id)
    pub bms: Vec<(usize, u32)>,
}

impl Reader {
    pub fn new(issue: u32, art: u32, doc: Doc) -> Reader {
        let width = doc
            .lines
            .iter()
            .map(|l| l.iter().map(|c| c.width().unwrap_or(0)).sum::<usize>())
            .max()
            .unwrap_or(0);
        Reader {
            issue,
            art,
            doc,
            row: 0,
            col: 0,
            want: 0,
            top: 0,
            hx: 0,
            width,
            search: None,
            hl: true,
            visual: None,
            hints: None,
            snips: Vec::new(),
            bms: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.doc.len()
    }

    fn max_col(&self, row: usize) -> usize {
        self.doc.line_len(row).saturating_sub(1)
    }

    pub fn clamp(&mut self) {
        self.row = self.row.min(self.len().saturating_sub(1));
        let mc = if self.visual.is_some() { self.doc.line_len(self.row) } else { self.max_col(self.row) };
        self.col = self.col.min(mc.max(0));
        if self.visual.is_none() {
            self.col = self.col.min(self.max_col(self.row));
        }
    }

    pub fn disp_col(&self, row: usize, col: usize) -> usize {
        self.doc.lines.get(row).map_or(0, |l| l.iter().take(col).map(|c| c.width().unwrap_or(0)).sum())
    }

    /// Keep cursor on screen (with scrolloff) and horizontally visible.
    pub fn ensure_visible(&mut self, vh: usize, tw: usize) {
        let vh = vh.max(1);
        let so = 4.min(vh.saturating_sub(1) / 2);
        let n = self.len();
        if self.row < self.top + so {
            self.top = self.row.saturating_sub(so);
        }
        if self.row + so + 1 > self.top + vh {
            self.top = (self.row + so + 1).saturating_sub(vh);
        }
        self.top = self.top.min(n.saturating_sub(1));
        let dc = self.disp_col(self.row, self.col);
        if tw >= self.width {
            self.hx = 0;
        } else {
            if dc < self.hx {
                self.hx = dc;
            }
            if dc >= self.hx + tw {
                self.hx = dc + 1 - tw;
            }
        }
    }

    pub fn center(&mut self, vh: usize) {
        self.top = self.row.saturating_sub(vh / 2);
    }

    // ---- vertical -------------------------------------------------------

    pub fn goto_row(&mut self, row: usize) {
        self.row = row.min(self.len().saturating_sub(1));
        self.col = self.want.min(self.max_col(self.row));
        self.clamp();
    }

    pub fn down(&mut self, n: usize) {
        self.goto_row(self.row + n);
    }

    pub fn up(&mut self, n: usize) {
        self.goto_row(self.row.saturating_sub(n));
    }

    pub fn left(&mut self, n: usize) {
        self.col = self.col.saturating_sub(n);
        self.want = self.col;
    }

    pub fn right(&mut self, n: usize) {
        let lim = if self.visual.is_some() { self.doc.line_len(self.row).saturating_sub(1) } else { self.max_col(self.row) };
        self.col = (self.col + n).min(lim);
        self.want = self.col;
    }

    pub fn line_start(&mut self) {
        self.col = 0;
        self.want = 0;
    }

    pub fn first_nonblank(&mut self) {
        let l = &self.doc.lines[self.row];
        self.col = l.iter().position(|c| !c.is_whitespace()).unwrap_or(0);
        self.want = self.col;
    }

    pub fn line_end(&mut self) {
        self.col = self.max_col(self.row);
        self.want = usize::MAX / 2;
    }

    pub fn goto_line_first_nonblank(&mut self, row: usize) {
        self.row = row.min(self.len().saturating_sub(1));
        self.first_nonblank();
    }

    pub fn half_page(&mut self, down: bool, vh: usize, n: usize) {
        let step = (vh / 2).max(1) * n.max(1);
        if down {
            self.top = (self.top + step).min(self.len().saturating_sub(1));
            self.goto_row(self.row + step);
        } else {
            self.top = self.top.saturating_sub(step);
            self.goto_row(self.row.saturating_sub(step));
        }
    }

    pub fn page(&mut self, down: bool, vh: usize, n: usize) {
        let step = vh.saturating_sub(2).max(1) * n.max(1);
        if down {
            self.top = (self.top + step).min(self.len().saturating_sub(1));
            self.goto_row((self.row + step).max(self.top));
        } else {
            self.top = self.top.saturating_sub(step);
            self.goto_row(self.row.saturating_sub(step));
        }
    }

    pub fn scroll_lines(&mut self, down: bool, vh: usize, n: usize) {
        let max_top = self.len().saturating_sub(1);
        if down {
            self.top = (self.top + n).min(max_top);
            if self.row < self.top + 4.min(vh / 2) {
                self.goto_row(self.top + 4.min(vh / 2));
            }
        } else {
            self.top = self.top.saturating_sub(n);
            let bottom = self.top + vh.saturating_sub(1);
            if self.row + 4.min(vh / 2) > bottom {
                self.goto_row(bottom.saturating_sub(4.min(vh / 2)));
            }
        }
    }

    pub fn screen_row(&mut self, which: char, vh: usize, n: usize) {
        let last = (self.top + vh).min(self.len()).saturating_sub(1);
        let so = 4.min(vh.saturating_sub(1) / 2);
        let r = match which {
            'H' => (self.top + n.saturating_sub(1)).max(if self.top > 0 { self.top + so } else { 0 }),
            'L' => last.saturating_sub(n.saturating_sub(1)).min(if last + 1 < self.len() { last.saturating_sub(so) } else { last }),
            _ => (self.top + last) / 2,
        };
        self.goto_line_first_nonblank(r.min(last));
    }

    pub fn z(&mut self, which: char, vh: usize) {
        match which {
            'z' | '.' => self.top = self.row.saturating_sub(vh / 2),
            't' | '\r' => self.top = self.row,
            'b' | '-' => self.top = (self.row + 1).saturating_sub(vh),
            _ => {}
        }
        if which == '.' || which == '\r' || which == '-' {
            self.first_nonblank();
        }
    }

    pub fn para(&mut self, fwd: bool, n: usize) {
        let blank = |r: usize| self.doc.lines[r].iter().all(|c| c.is_whitespace());
        let mut r = self.row;
        for _ in 0..n.max(1) {
            if fwd {
                while r + 1 < self.len() && blank(r) {
                    r += 1;
                }
                while r + 1 < self.len() && !blank(r) {
                    r += 1;
                }
            } else {
                while r > 0 && blank(r) {
                    r -= 1;
                }
                while r > 0 && !blank(r) {
                    r -= 1;
                }
            }
        }
        self.row = r;
        self.col = 0;
        self.want = 0;
    }

    /// `]]` / `[[`: jump to the n-th next/previous heading. Returns false at the edge.
    pub fn heading(&mut self, fwd: bool, n: usize) -> bool {
        let hs = &self.doc.headings;
        let mut r = self.row;
        let mut moved = false;
        for _ in 0..n.max(1) {
            let t = if fwd {
                hs.iter().find(|h| h.row > r)
            } else {
                hs.iter().rev().find(|h| h.row < r)
            };
            match t {
                Some(h) => {
                    r = h.row;
                    moved = true;
                }
                None => break,
            }
        }
        if moved {
            self.goto_line_first_nonblank(r);
            self.top = r.saturating_sub(1).min(self.top.max(r.saturating_sub(1)));
        }
        moved
    }

    pub fn current_heading(&self) -> Option<&crate::doc::Heading> {
        self.doc.headings.iter().rev().find(|h| h.row <= self.row)
    }

    // ---- words ------------------------------------------------------------

    fn cls(&self, (r, c): (usize, usize), big: bool) -> u8 {
        let l = &self.doc.lines[r];
        match l.get(c) {
            None => 0,
            Some(ch) if ch.is_whitespace() => 0,
            Some(_) if big => 1,
            Some(ch) if ch.is_alphanumeric() || *ch == '_' => 1,
            Some(_) => 2,
        }
    }

    fn next_pos(&self, (r, c): (usize, usize)) -> Option<(usize, usize)> {
        if c < self.doc.line_len(r) {
            Some((r, c + 1))
        } else if r + 1 < self.len() {
            Some((r + 1, 0))
        } else {
            None
        }
    }

    fn prev_pos(&self, (r, c): (usize, usize)) -> Option<(usize, usize)> {
        if c > 0 {
            Some((r, c - 1))
        } else if r > 0 {
            Some((r - 1, self.doc.line_len(r - 1)))
        } else {
            None
        }
    }

    fn settle(&mut self, p: (usize, usize)) {
        self.row = p.0;
        let ll = self.doc.line_len(p.0);
        self.col = if ll == 0 {
            0
        } else if self.visual.is_some() {
            p.1.min(ll - 1)
        } else {
            p.1.min(ll - 1)
        };
        self.want = self.col;
    }

    pub fn word_fwd(&mut self, big: bool, n: usize) {
        for _ in 0..n.max(1) {
            let start = (self.row, self.col);
            let mut p = start;
            let c0 = self.cls(p, big);
            if c0 != 0 {
                loop {
                    match self.next_pos(p) {
                        Some(nx) => {
                            p = nx;
                            if self.cls(p, big) != c0 {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
            loop {
                if self.cls(p, big) != 0 {
                    break;
                }
                if self.doc.line_len(p.0) == 0 && p != start {
                    break;
                }
                match self.next_pos(p) {
                    Some(nx) => p = nx,
                    None => break,
                }
            }
            self.settle(p);
        }
    }

    pub fn word_end(&mut self, big: bool, n: usize) {
        for _ in 0..n.max(1) {
            let mut p = (self.row, self.col);
            let Some(nx) = self.next_pos(p) else { return };
            p = nx;
            while self.cls(p, big) == 0 {
                match self.next_pos(p) {
                    Some(nx) => p = nx,
                    None => break,
                }
            }
            let c0 = self.cls(p, big);
            while let Some(nx) = self.next_pos(p) {
                if nx.0 == p.0 && self.cls(nx, big) == c0 {
                    p = nx;
                } else {
                    break;
                }
            }
            self.settle(p);
        }
    }

    pub fn word_back(&mut self, big: bool, n: usize) {
        for _ in 0..n.max(1) {
            let mut p = (self.row, self.col);
            let Some(pv) = self.prev_pos(p) else { return };
            p = pv;
            while self.cls(p, big) == 0 {
                if self.doc.line_len(p.0) == 0 {
                    break;
                }
                match self.prev_pos(p) {
                    Some(pv) => p = pv,
                    None => break,
                }
            }
            let c0 = self.cls(p, big);
            while let Some(pv) = self.prev_pos(p) {
                if pv.0 == p.0 && c0 != 0 && self.cls(pv, big) == c0 {
                    p = pv;
                } else {
                    break;
                }
            }
            self.settle(p);
        }
    }

    pub fn word_under_cursor(&self) -> Option<String> {
        let l = &self.doc.lines[self.row];
        let isw = |c: &char| c.is_alphanumeric() || *c == '_';
        let start = (self.col..l.len()).find(|&i| isw(&l[i]))?;
        let b = (0..=start).rev().take_while(|&i| isw(&l[i])).last().unwrap_or(start);
        let e = (start..l.len()).take_while(|&i| isw(&l[i])).last().unwrap_or(start);
        Some(l[b..=e].iter().collect())
    }

    // ---- search -----------------------------------------------------------

    /// (char col, char len) of each match on a row.
    pub fn line_matches(&self, row: usize) -> Vec<(usize, usize)> {
        let Some(s) = &self.search else { return Vec::new() };
        let line = self.doc.line_string(row);
        if line.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for m in s.re.find_iter(&line) {
            if m.is_empty() {
                continue;
            }
            let col = line[..m.start()].chars().count();
            let len = line[m.start()..m.end()].chars().count();
            out.push((col, len));
        }
        out
    }

    /// Move to the next/previous match. Returns Some(wrapped) when found.
    pub fn search_step(&mut self, fwd: bool, n: usize) -> Option<bool> {
        self.search.as_ref()?;
        let total = self.len();
        let mut pos = (self.row, self.col);
        let mut wrapped = false;
        for _ in 0..n.max(1) {
            let mut found = None;
            for i in 0..=total {
                let (row, wrap) = if fwd {
                    let r = pos.0 + i;
                    (r % total, r >= total)
                } else if i <= pos.0 {
                    (pos.0 - i, false)
                } else {
                    (pos.0 + total - i, true)
                };
                let ms = self.line_matches(row);
                let cand = if i == 0 {
                    if fwd { ms.iter().find(|m| m.0 > pos.1) } else { ms.iter().rev().find(|m| m.0 < pos.1) }
                } else if i == total {
                    if fwd { ms.iter().find(|m| m.0 <= pos.1) } else { ms.iter().rev().find(|m| m.0 >= pos.1) }
                } else if fwd {
                    ms.first()
                } else {
                    ms.last()
                };
                if let Some(m) = cand {
                    found = Some((row, m.0));
                    wrapped |= wrap;
                    break;
                }
            }
            pos = found?;
        }
        self.row = pos.0;
        self.col = pos.1;
        self.want = pos.1;
        Some(wrapped)
    }

    // ---- visual / yank ----------------------------------------------------

    pub fn in_selection(&self, row: usize, col: usize) -> bool {
        let Some((mode, a)) = self.visual else { return false };
        let (s, e) = order(a, (self.row, self.col));
        match mode {
            VMode::Line => row >= s.0 && row <= e.0,
            VMode::Char => (row, col) >= s && (row, col) <= e,
        }
    }

    pub fn selection_range(&self) -> Option<(VMode, (usize, usize), (usize, usize))> {
        let (mode, a) = self.visual?;
        let (s, e) = order(a, (self.row, self.col));
        Some((mode, s, e))
    }

    pub fn selection_text(&self) -> Option<String> {
        let (mode, a) = self.visual?;
        let (s, e) = order(a, (self.row, self.col));
        let mut out = Vec::new();
        for r in s.0..=e.0 {
            let l = &self.doc.lines[r];
            let seg: String = match mode {
                VMode::Line => l.iter().collect(),
                VMode::Char => {
                    let from = if r == s.0 { s.1 } else { 0 };
                    let to = if r == e.0 { (e.1 + 1).min(l.len()) } else { l.len() };
                    l.get(from..to.max(from)).map(|x| x.iter().collect()).unwrap_or_default()
                }
            };
            out.push(seg);
        }
        Some(out.join("\n"))
    }

    /// The innermost snippet covering a position.
    pub fn snip_at(&self, row: usize, col: usize) -> Option<&Span> {
        self.snips.iter().filter(|s| s.contains(row, col)).min_by_key(|s| (s.e.0 - s.s.0, s.id))
    }

    pub fn bookmark_on(&self, row: usize) -> Option<u32> {
        self.bms.iter().find(|b| b.0 == row).map(|b| b.1)
    }

    pub fn percent(&self, vh: usize) -> String {
        let n = self.len();
        if n <= vh {
            "All".into()
        } else if self.top == 0 {
            "Top".into()
        } else if self.top + vh >= n {
            "Bot".into()
        } else {
            format!("{}%", (self.row + 1) * 100 / n)
        }
    }
}

fn order(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if a <= b { (a, b) } else { (b, a) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Library;

    fn rd(text: &str) -> Reader {
        let lib = Library { issues: vec![] };
        let lines = crate::doc::body_lines(text);
        Reader::new(1, 1, Doc::build(lines, &lib, 1))
    }

    #[test]
    fn words() {
        let mut r = rd("foo bar.baz\n\nqux");
        r.word_fwd(false, 1);
        assert_eq!((r.row, r.col), (0, 4));
        r.word_fwd(false, 1);
        assert_eq!((r.row, r.col), (0, 7));
        r.word_fwd(false, 2);
        assert_eq!((r.row, r.col), (1, 0)); // empty line stops
        r.word_fwd(false, 1);
        assert_eq!((r.row, r.col), (2, 0));
        r.word_back(false, 1);
        assert_eq!((r.row, r.col), (1, 0));
        r.word_back(false, 1);
        assert_eq!((r.row, r.col), (0, 8));
        r.row = 0;
        r.col = 0;
        r.word_end(false, 1);
        assert_eq!((r.row, r.col), (0, 2));
        r.word_end(false, 1);
        assert_eq!((r.row, r.col), (0, 6));
    }

    #[test]
    fn search_wraps() {
        let mut r = rd("alpha\nbeta\nAlpha again");
        r.search = Some(Search::new("alpha", true));
        assert_eq!(r.search_step(true, 1), Some(false));
        assert_eq!((r.row, r.col), (2, 0));
        assert_eq!(r.search_step(true, 1), Some(true));
        assert_eq!((r.row, r.col), (0, 0));
        assert_eq!(r.search_step(false, 1), Some(true));
        assert_eq!((r.row, r.col), (2, 0));
    }

    #[test]
    fn paragraphs_and_yank() {
        let mut r = rd("a\nb\n\nc\nd");
        r.para(true, 1);
        assert_eq!(r.row, 2);
        r.para(true, 1);
        assert_eq!(r.row, 4);
        r.row = 0;
        r.visual = Some((VMode::Line, (0, 0)));
        r.row = 1;
        assert_eq!(r.selection_text().unwrap(), "a\nb");
    }
}
