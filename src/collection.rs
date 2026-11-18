//! Bookmarks and snippets: the user's own collection, persisted next to the
//! state file. Tab-separated, one record per line, text fields escaped.

use std::{fs, path::{Path, PathBuf}};

use crate::{doc::Doc, reader::Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub id: u32,
    pub issue: u32,
    pub art: u32,
    pub row: usize,
    /// Snapshot of the bookmarked line, used to re-find it if the file shifts.
    pub text: String,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub id: u32,
    pub issue: u32,
    pub art: u32,
    pub line: bool,
    pub s: (usize, usize),
    pub e: (usize, usize),
    pub name: String,
    pub note: String,
    pub text: String,
}

#[derive(Default, Clone, Debug)]
pub struct Collection {
    pub path: Option<PathBuf>,
    pub bookmarks: Vec<Bookmark>,
    pub snippets: Vec<Snippet>,
    next_id: u32,
}

pub fn path_for(state: Option<&Path>) -> Option<PathBuf> {
    let p = state?;
    let name = p.file_name()?.to_string_lossy().to_string();
    Some(p.with_file_name(format!("{name}.collection")))
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o
}

fn unesc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('t') => o.push('\t'),
                Some(x) => o.push(x),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

impl Collection {
    pub fn load(path: Option<PathBuf>) -> Collection {
        let mut c = Collection { path: path.clone(), ..Default::default() };
        let Some(p) = path else { return c };
        let Ok(text) = fs::read_to_string(p) else { return c };
        for l in text.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|s| s.parse::<usize>().ok());
            match f.first().copied() {
                Some("B") if f.len() >= 7 => {
                    if let (Some(id), Some(issue), Some(art), Some(row)) = (n(1), n(2), n(3), n(4)) {
                        c.bookmarks.push(Bookmark {
                            id: id as u32,
                            issue: issue as u32,
                            art: art as u32,
                            row,
                            note: unesc(f[5]),
                            text: unesc(f[6]),
                        });
                        c.next_id = c.next_id.max(id as u32);
                    }
                }
                Some("S") if f.len() >= 13 => {
                    if let (Some(id), Some(issue), Some(art), Some(line), Some(sr), Some(sc), Some(er), Some(ec)) =
                        (n(1), n(2), n(3), n(4), n(5), n(6), n(7), n(8))
                    {
                        c.snippets.push(Snippet {
                            id: id as u32,
                            issue: issue as u32,
                            art: art as u32,
                            line: line != 0,
                            s: (sr, sc),
                            e: (er, ec),
                            name: unesc(f[9]),
                            note: unesc(f[10]),
                            text: unesc(f[11]),
                        });
                        c.next_id = c.next_id.max(id as u32);
                    }
                }
                _ => {}
            }
        }
        c
    }

    pub fn save(&self) {
        let Some(p) = &self.path else { return };
        let mut s = String::new();
        for b in &self.bookmarks {
            s.push_str(&format!("B\t{}\t{}\t{}\t{}\t{}\t{}\t\n", b.id, b.issue, b.art, b.row, esc(&b.note), esc(&b.text)));
        }
        for n in &self.snippets {
            s.push_str(&format!(
                "S\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t\n",
                n.id, n.issue, n.art, n.line as u8, n.s.0, n.s.1, n.e.0, n.e.1, esc(&n.name), esc(&n.note), esc(&n.text)
            ));
        }
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(p, s);
    }

    fn fresh_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn bookmark_at(&self, issue: u32, art: u32, row: usize) -> Option<&Bookmark> {
        self.bookmarks.iter().find(|b| b.issue == issue && b.art == art && b.row == row)
    }

    /// Add a bookmark, or remove the one already on that line. Returns true if added.
    pub fn toggle_bookmark(&mut self, issue: u32, art: u32, row: usize, text: &str, note: &str) -> bool {
        if let Some(i) = self.bookmarks.iter().position(|b| b.issue == issue && b.art == art && b.row == row) {
            self.bookmarks.remove(i);
            self.save();
            return false;
        }
        let id = self.fresh_id();
        self.bookmarks.push(Bookmark { id, issue, art, row, text: text.trim().chars().take(120).collect(), note: note.to_string() });
        self.save();
        true
    }

    pub fn add_snippet(&mut self, mut s: Snippet) -> u32 {
        s.id = self.fresh_id();
        let id = s.id;
        self.snippets.push(s);
        self.save();
        id
    }

    pub fn remove_bookmark(&mut self, id: u32) -> bool {
        let n = self.bookmarks.len();
        self.bookmarks.retain(|b| b.id != id);
        self.save();
        n != self.bookmarks.len()
    }

    pub fn remove_snippet(&mut self, id: u32) -> bool {
        let n = self.snippets.len();
        self.snippets.retain(|b| b.id != id);
        self.save();
        n != self.snippets.len()
    }

    /// Re-anchor this article's bookmarks and snippets against the loaded text
    /// (fixing stored coordinates when the text moved) and return the spans to
    /// draw plus bookmark rows.
    pub fn sync(&mut self, issue: u32, art: u32, doc: &Doc) -> (Vec<Span>, Vec<(usize, u32)>) {
        let mut dirty = false;
        let mut spans = Vec::new();
        let mut rows = Vec::new();
        for b in self.bookmarks.iter_mut().filter(|b| b.issue == issue && b.art == art) {
            let same = |r: usize| doc.line_string(r).trim().chars().take(120).collect::<String>() == b.text;
            if !(b.row < doc.len() && same(b.row)) && !b.text.is_empty() {
                let found = (0..doc.len()).filter(|&r| same(r)).min_by_key(|&r| r.abs_diff(b.row));
                if let Some(r) = found {
                    b.row = r;
                    dirty = true;
                }
            }
            if b.row < doc.len() {
                rows.push((b.row, b.id));
            }
        }
        for sn in self.snippets.iter_mut().filter(|s| s.issue == issue && s.art == art) {
            if extract(doc, sn.line, sn.s, sn.e).as_deref() != Some(sn.text.as_str()) {
                match relocate(doc, &sn.text, sn.line) {
                    Some((s, e)) => {
                        sn.s = s;
                        sn.e = e;
                        dirty = true;
                    }
                    None => continue, // orphaned: keep the record, draw nothing
                }
            }
            spans.push(Span { id: sn.id, line: sn.line, s: sn.s, e: sn.e });
        }
        if dirty {
            self.save();
        }
        rows.sort();
        (spans, rows)
    }
}

/// Text covered by a range, same semantics as a visual yank.
pub fn extract(doc: &Doc, line: bool, s: (usize, usize), e: (usize, usize)) -> Option<String> {
    if s > e || e.0 >= doc.len() {
        return None;
    }
    let mut out = Vec::new();
    for r in s.0..=e.0 {
        let l = &doc.lines[r];
        let seg: String = if line {
            l.iter().collect()
        } else {
            let from = if r == s.0 { s.1 } else { 0 };
            let to = if r == e.0 { (e.1 + 1).min(l.len()) } else { l.len() };
            l.get(from..to.max(from))?.iter().collect()
        };
        out.push(seg);
    }
    Some(out.join("\n"))
}

fn relocate(doc: &Doc, text: &str, line: bool) -> Option<((usize, usize), (usize, usize))> {
    if text.is_empty() {
        return None;
    }
    let mut starts = Vec::with_capacity(doc.len());
    let mut joined = String::new();
    for (i, l) in doc.lines.iter().enumerate() {
        if i > 0 {
            joined.push('\n');
        }
        starts.push(joined.len());
        joined.extend(l.iter());
    }
    let at = joined.find(text)?;
    let locate = |byte: usize| -> (usize, usize) {
        let row = starts.partition_point(|&s| s <= byte) - 1;
        (row, joined[starts[row]..byte].chars().count())
    };
    let s = locate(at);
    let last = joined[..at + text.len()].char_indices().last().map_or(at, |(i, _)| i);
    let mut e = locate(last);
    let mut s = s;
    if line {
        s.1 = 0;
        e.1 = doc.line_len(e.0);
    }
    Some((s, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Library;

    fn doc(t: &str) -> Doc {
        Doc::build(crate::doc::body_lines(t), &Library { issues: vec![] }, 1)
    }

    #[test]
    fn roundtrip_and_escape() {
        let dir = std::env::temp_dir().join(format!("phrack-coll-{}", std::process::id()));
        let p = dir.join("state.collection");
        let mut c = Collection::load(Some(p.clone()));
        assert!(c.toggle_bookmark(3, 2, 10, "  hello\tworld ", "a\nnote"));
        c.add_snippet(Snippet {
            id: 0,
            issue: 3,
            art: 2,
            line: false,
            s: (1, 2),
            e: (2, 3),
            name: "n\\x".into(),
            note: String::new(),
            text: "ab\ncd".into(),
        });
        let d = Collection::load(Some(p));
        assert_eq!(d.bookmarks, c.bookmarks);
        assert_eq!(d.snippets, c.snippets);
        assert_eq!(d.snippets[0].id, 2);
        assert!(!c.toggle_bookmark(3, 2, 10, "", ""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sync_relocates() {
        let d1 = doc("one\ntwo three\nfour five\nsix");
        let mut c = Collection::default();
        let text = extract(&d1, false, (1, 4), (2, 3)).unwrap();
        assert_eq!(text, "three\nfour");
        c.add_snippet(Snippet { id: 0, issue: 1, art: 1, line: false, s: (1, 4), e: (2, 3), name: "x".into(), note: String::new(), text });
        c.toggle_bookmark(1, 1, 3, "six", "");
        let (sp, rows) = c.sync(1, 1, &d1);
        assert_eq!(sp.len(), 1);
        assert_eq!(rows, vec![(3, 2)]);
        // two lines get inserted at the top
        let d2 = doc("new\nnew2\none\ntwo three\nfour five\nsix");
        let (sp, rows) = c.sync(1, 1, &d2);
        assert_eq!((sp[0].s, sp[0].e), ((3, 4), (4, 3)));
        assert_eq!(rows, vec![(5, 2)]);
        // text gone entirely: orphaned, nothing drawn
        let d3 = doc("nothing here");
        assert!(c.sync(1, 1, &d3).0.is_empty());
    }
}
