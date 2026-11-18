//! The bookmark / snippet collection browser (state + key handling).

use crate::{
    app::{App, CmdCtx, CmdLine, Overlay},
    doc,
    input::K,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Bookmarks,
    Snippets,
}

#[derive(Default)]
pub struct Preview {
    pub lines: Vec<String>,
    /// Index into `lines` of the bookmarked line.
    pub hi: Option<usize>,
}

pub struct CollView {
    pub tab: Tab,
    pub sel: usize,
    pub filter: String,
    pub filtering: bool,
    pub pending: Option<char>,
    pub preview: Preview,
    cache: Option<((u32, u32), Vec<String>)>,
}

impl CollView {
    pub fn new(tab: Tab) -> CollView {
        CollView { tab, sel: 0, filter: String::new(), filtering: false, pending: None, preview: Preview::default(), cache: None }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: u32,
    pub issue: u32,
    pub art: u32,
    pub row: usize,
    pub col: usize,
    /// Snippet name, or bookmark note / line text.
    pub label: String,
    pub place: String,
    pub art_title: String,
    pub author: String,
    pub note: String,
    pub text: String,
}

impl App {
    pub fn open_coll(&mut self, tab: Tab) {
        let mut v = CollView::new(tab);
        // start on the entry nearest the phile being read
        if let Some(r) = &self.reader {
            let items = self.coll_items(tab, "");
            if let Some(i) = items.iter().position(|it| (it.issue, it.art) >= (r.issue, r.art)) {
                v.sel = i;
            }
        }
        self.coll_refresh_preview(&mut v);
        self.overlay = Overlay::Coll(Box::new(v));
    }

    pub fn coll_items(&self, tab: Tab, filter: &str) -> Vec<Item> {
        let meta = |issue: u32, art: u32| {
            self.lib.article(issue, art).map(|a| (a.title.clone(), a.author.clone())).unwrap_or_default()
        };
        let mut items: Vec<Item> = match tab {
            Tab::Bookmarks => self
                .coll
                .bookmarks
                .iter()
                .map(|b| {
                    let (t, a) = meta(b.issue, b.art);
                    Item {
                        id: b.id,
                        issue: b.issue,
                        art: b.art,
                        row: b.row,
                        col: 0,
                        label: if b.note.is_empty() { b.text.clone() } else { b.note.clone() },
                        place: format!("{}.{}:{}", b.issue, b.art, b.row + 1),
                        art_title: t,
                        author: a,
                        note: b.note.clone(),
                        text: b.text.clone(),
                    }
                })
                .collect(),
            Tab::Snippets => self
                .coll
                .snippets
                .iter()
                .map(|s| {
                    let (t, a) = meta(s.issue, s.art);
                    Item {
                        id: s.id,
                        issue: s.issue,
                        art: s.art,
                        row: s.s.0,
                        col: s.s.1,
                        label: s.name.clone(),
                        place: format!("{}.{}:{}", s.issue, s.art, s.s.0 + 1),
                        art_title: t,
                        author: a,
                        note: s.note.clone(),
                        text: s.text.clone(),
                    }
                })
                .collect(),
        };
        items.sort_by_key(|i| (i.issue, i.art, i.row, i.col, i.id));
        let terms: Vec<String> = filter.split_whitespace().map(|t| t.to_lowercase()).collect();
        if !terms.is_empty() {
            items.retain(|i| {
                let hay = format!("{} {} {} {} {} {}", i.label, i.place, i.art_title, i.author, i.note, i.text).to_lowercase();
                terms.iter().all(|t| hay.contains(t.as_str()))
            });
        }
        items
    }

    fn coll_refresh_preview(&self, v: &mut CollView) {
        let items = self.coll_items(v.tab, &v.filter);
        v.sel = v.sel.min(items.len().saturating_sub(1));
        v.preview = Preview::default();
        let Some(it) = items.get(v.sel) else { return };
        match v.tab {
            Tab::Snippets => v.preview.lines = it.text.lines().take(400).map(String::from).collect(),
            Tab::Bookmarks => {
                let key = (it.issue, it.art);
                if v.cache.as_ref().map(|c| c.0) != Some(key) {
                    let lines = self.lib.article(it.issue, it.art).map(|a| doc::load_lines(&a.path)).unwrap_or_default();
                    v.cache = Some((key, lines));
                }
                let lines = &v.cache.as_ref().unwrap().1;
                let start = it.row.saturating_sub(8);
                let end = (it.row + 14).min(lines.len());
                v.preview.lines = lines.get(start..end).unwrap_or(&[]).to_vec();
                v.preview.hi = Some(it.row - start);
            }
        }
    }

    pub fn coll_key(&mut self, mut v: CollView, k: K) {
        let n = self.coll_items(v.tab, &v.filter).len() as isize;
        let page = (self.view_h() / 2).max(1) as isize;
        let mv = |v: &mut CollView, d: isize| v.sel = (v.sel as isize + d).clamp(0, (n - 1).max(0)) as usize;
        let mut close = false;
        let mut jump: Option<Item> = None;
        if v.filtering {
            match k {
                K::Esc | K::Enter => v.filtering = false,
                K::Backspace => {
                    if v.filter.pop().is_none() {
                        v.filtering = false;
                    }
                    v.sel = 0;
                }
                K::Ctrl('u') => {
                    v.filter.clear();
                    v.sel = 0;
                }
                K::Down | K::Ctrl('n') => mv(&mut v, 1),
                K::Up | K::Ctrl('p') => mv(&mut v, -1),
                K::Char(c) => {
                    v.filter.push(c);
                    v.sel = 0;
                }
                _ => {}
            }
        } else if let Some(p) = v.pending.take() {
            let items = self.coll_items(v.tab, &v.filter);
            match (p, k) {
                ('g', K::Char('g')) => v.sel = 0,
                ('d', K::Char('d')) => {
                    if let Some(it) = items.get(v.sel) {
                        let (id, tab) = (it.id, v.tab);
                        match tab {
                            Tab::Bookmarks => self.coll.remove_bookmark(id),
                            Tab::Snippets => self.coll.remove_snippet(id),
                        };
                        self.refresh_marks();
                        self.say(format!("{} #{id} deleted", if tab == Tab::Bookmarks { "bookmark" } else { "snippet" }));
                    }
                }
                _ => {}
            }
        } else {
            let items = self.coll_items(v.tab, &v.filter);
            match k {
                K::Esc | K::Char('q') => close = true,
                K::Char('j') | K::Down | K::Ctrl('n') => mv(&mut v, 1),
                K::Char('k') | K::Up | K::Ctrl('p') => mv(&mut v, -1),
                K::Ctrl('d') | K::PageDown => mv(&mut v, page),
                K::Ctrl('u') | K::PageUp => mv(&mut v, -page),
                K::Char('G') | K::End => v.sel = (n - 1).max(0) as usize,
                K::Home => v.sel = 0,
                K::Char(c @ ('g' | 'd')) => v.pending = Some(c),
                K::Tab | K::BackTab | K::Char('1') | K::Char('2') | K::Char('h') | K::Char('l') | K::Left | K::Right => {
                    let to = match k {
                        K::Char('1') | K::Char('h') | K::Left => Tab::Bookmarks,
                        K::Char('2') | K::Char('l') | K::Right => Tab::Snippets,
                        _ => {
                            if v.tab == Tab::Bookmarks {
                                Tab::Snippets
                            } else {
                                Tab::Bookmarks
                            }
                        }
                    };
                    if to != v.tab {
                        v.tab = to;
                        v.sel = 0;
                        v.filter.clear();
                    }
                }
                K::Char('/') => v.filtering = true,
                K::Enter | K::Char('o') => {
                    if let Some(it) = items.get(v.sel) {
                        jump = Some(it.clone());
                        close = true;
                    }
                }
                K::Char('r') | K::Char('e') => {
                    if let Some(it) = items.get(v.sel) {
                        let (prefix, text, ctx) = match (v.tab, k) {
                            (Tab::Snippets, K::Char('r')) => ('R', it.label.clone(), CmdCtx::SnipName(it.id)),
                            (Tab::Snippets, _) => ('E', it.note.clone(), CmdCtx::SnipNote(it.id)),
                            (Tab::Bookmarks, _) => ('E', it.note.clone(), CmdCtx::BmNote(it.id)),
                        };
                        self.cmd = Some(CmdLine::with_ctx(prefix, text, ctx));
                    }
                }
                K::Char('y') => {
                    if let Some(it) = items.get(v.sel) {
                        let lines = it.text.lines().count().max(1);
                        let text = it.text.clone();
                        if crate::app::copy_to_clipboard(&text) {
                            self.say(format!("{lines} line{} yanked", if lines == 1 { "" } else { "s" }));
                        }
                    }
                }
                _ => {}
            }
        }
        self.coll_refresh_preview(&mut v);
        if !close {
            self.overlay = Overlay::Coll(Box::new(v));
        }
        if let Some(it) = jump {
            if self.reader.is_some() {
                self.push_jump();
            }
            if self.open_article(it.issue, it.art, Some(it.row)) {
                let vh = self.view_h();
                let r = self.reader.as_mut().unwrap();
                r.row = it.row.min(r.len() - 1);
                r.col = it.col.min(r.doc.line_len(r.row).saturating_sub(1));
                r.want = r.col;
                r.top = r.row.saturating_sub(vh / 3);
                self.after_move();
            }
        }
    }
}
