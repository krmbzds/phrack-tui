//! Application state and all key handling.

use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{MouseEvent, MouseEventKind};

use crate::{
    boot::Boot,
    collection::{Collection, Snippet},
    collview::{CollView, Tab},
    crt::Fx,
    data::{Library, Special},
    doc::{self, Doc, Ref},
    input::K,
    picker::{PickAction, PickItem, Picker, Target},
    reader::{build_regex, Reader, Search, VMode},
    state::PState,
    theme::{self, Theme},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Issues,
    Articles,
}

pub struct Browser {
    pub issue: usize,
    pub art: usize,
    pub pane: Pane,
}

pub enum Overlay {
    None,
    Help(usize),
    Picker(Box<Picker>),
    Coll(Box<CollView>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdCtx {
    None,
    NewSnippet,
    SnipName(u32),
    SnipNote(u32),
    BmNote(u32),
}

pub struct CmdLine {
    pub prefix: char,
    pub text: String,
    pub ctx: CmdCtx,
}

impl CmdLine {
    pub fn plain(prefix: char, text: String) -> CmdLine {
        CmdLine { prefix, text, ctx: CmdCtx::None }
    }
}

pub struct Msg {
    pub text: String,
    pub err: bool,
    pub at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Loc {
    pub issue: u32,
    pub art: u32,
    pub row: usize,
}

#[derive(Default)]
pub struct Jumps {
    pub list: Vec<Loc>,
    pub pos: usize,
}

pub struct App {
    pub lib: Library,
    pub theme: Theme,
    pub fx: Fx,
    pub st: PState,
    pub coll: Collection,
    pub hover: Option<(usize, usize)>,
    pub browser: Browser,
    pub reader: Option<Reader>,
    pub overlay: Overlay,
    pub cmd: Option<CmdLine>,
    pub pending: String,
    pub count: Option<usize>,
    pub msg: Option<Msg>,
    pub jumps: Jumps,
    pub prev_loc: Option<Loc>,
    pub size: (u16, u16),
    pub quit: bool,
    pub blink: bool,
    pub boot: Option<Boot>,
    pub nu: bool,
    pub rnu: bool,
    pub redraw_all: bool,
    pub bright: f32,
    last_blink: Instant,
    next_flicker: Instant,
    flicker_until: Option<Instant>,
    rng: u64,
}

macro_rules! rd {
    ($s:expr) => {
        $s.reader.as_mut().expect("reader open")
    };
}

impl App {
    pub fn new(lib: Library, st: PState, coll: Collection, size: (u16, u16)) -> App {
        let theme = st
            .theme
            .as_deref()
            .and_then(theme::by_name)
            .unwrap_or_else(|| theme::all().remove(0));
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15)
            | 1;
        let mut app = App {
            browser: Browser { issue: lib.issues.len().saturating_sub(1), art: 0, pane: Pane::Issues },
            lib,
            theme,
            fx: Fx::default(),
            st,
            coll,
            hover: None,
            reader: None,
            overlay: Overlay::None,
            cmd: None,
            pending: String::new(),
            count: None,
            msg: None,
            jumps: Jumps::default(),
            prev_loc: None,
            size,
            quit: false,
            blink: true,
            boot: None,
            nu: false,
            rnu: false,
            redraw_all: false,
            bright: 1.0,
            last_blink: Instant::now(),
            next_flicker: Instant::now() + Duration::from_secs(6),
            flicker_until: None,
            rng: seed,
        };
        if let Some((i, a, _)) = app.st.last {
            if app.lib.article(i, a).is_some() {
                app.browser_select(i, a, Pane::Articles);
            }
        }
        app
    }

    // ---- small helpers --------------------------------------------------

    pub fn say(&mut self, text: impl Into<String>) {
        self.msg = Some(Msg { text: text.into(), err: false, at: Instant::now() });
    }

    pub fn err(&mut self, text: impl Into<String>) {
        self.msg = Some(Msg { text: text.into(), err: true, at: Instant::now() });
    }

    fn rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    pub fn view_h(&self) -> usize {
        (self.size.1 as usize).saturating_sub(4).max(1)
    }

    pub fn view_w(&self) -> usize {
        (self.size.0 as usize).saturating_sub(2).max(1)
    }

    /// (gutter width, text area width) for the reader.
    pub fn geometry(&self) -> (usize, usize) {
        // two columns of sign gutter (bookmark, snippet) + optional line numbers
        let gutter = 2 + match &self.reader {
            Some(r) if self.nu || self.rnu => r.len().to_string().len() + 1,
            _ => 0,
        };
        (gutter, self.view_w().saturating_sub(gutter + 1).max(1))
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.size = (w, h);
        self.redraw_all = true;
        self.reader_visible();
    }

    fn reader_visible(&mut self) {
        let (vh, (_, tw)) = (self.view_h(), self.geometry());
        if let Some(r) = self.reader.as_mut() {
            r.clamp();
            r.ensure_visible(vh, tw);
        }
    }

    // ---- ticking --------------------------------------------------------

    /// Advance timers. Returns true when something visible changed.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut dirty = false;
        if let Some(b) = &self.boot {
            if b.done() {
                self.boot = None;
            }
            return true;
        }
        if now.duration_since(self.last_blink) > Duration::from_millis(530) {
            self.last_blink = now;
            self.blink = !self.blink;
            dirty = self.reader.is_some();
        }
        if let Some(m) = &self.msg {
            if now.duration_since(m.at) > Duration::from_secs(6) {
                self.msg = None;
                dirty = true;
            }
        }
        if let Overlay::Picker(p) = &mut self.overlay {
            dirty |= p.poll();
        }
        if self.fx.flicker {
            match self.flicker_until {
                Some(t) if now >= t => {
                    self.flicker_until = None;
                    self.bright = 1.0;
                    let wait = 4 + self.rand() % 9;
                    self.next_flicker = now + Duration::from_secs(wait);
                    dirty = true;
                }
                None if now >= self.next_flicker => {
                    self.bright = if self.rand() % 3 == 0 { 0.90 } else { 0.95 };
                    self.flicker_until = Some(now + Duration::from_millis(70));
                    dirty = true;
                }
                _ => {}
            }
        } else if self.bright != 1.0 {
            self.bright = 1.0;
            dirty = true;
        }
        dirty
    }

    // ---- input dispatch ---------------------------------------------------

    pub fn on_key(&mut self, k: K) {
        self.hover = None;
        self.blink = true;
        self.last_blink = Instant::now();
        if self.boot.take().is_some() {
            return;
        }
        if k == K::Ctrl('c') && self.cmd.is_none() && matches!(self.overlay, Overlay::None) {
            self.pending.clear();
            self.count = None;
            if let Some(r) = self.reader.as_mut() {
                r.visual = None;
                r.hints = None;
            }
            return;
        }
        if self.cmd.is_some() {
            return self.cmdline_key(k);
        }
        match std::mem::replace(&mut self.overlay, Overlay::None) {
            Overlay::None => {}
            Overlay::Help(scroll) => return self.help_key(k, scroll),
            Overlay::Coll(v) => return self.coll_key(*v, k),
            Overlay::Picker(mut p) => {
                p.page = self.view_h().saturating_sub(6).max(4);
                match p.key(k) {
                    PickAction::None => self.overlay = Overlay::Picker(p),
                    PickAction::Close => {}
                    PickAction::Select(t) => {
                        let pat = p.pattern.clone();
                        self.pick_select(t, pat);
                    }
                }
                return;
            }
        }
        if self.reader.is_some() {
            self.reader_key(k);
        } else {
            self.browser_key(k);
        }
    }

    /// Map a screen cell to (row, char col, inside-the-text) in the open phile.
    pub fn screen_pos(&self, x: u16, y: u16) -> Option<(usize, usize, bool)> {
        let r = self.reader.as_ref()?;
        if y < 1 || y as usize > self.view_h() {
            return None;
        }
        let row = r.top + (y as usize - 1);
        if row >= r.len() {
            return None;
        }
        let (gutter, avail) = self.geometry();
        let page_x = if r.width < avail { (avail - r.width) / 2 } else { 0 };
        let tx = 1 + gutter + page_x;
        let dc = (x as usize + r.hx).checked_sub(tx)?;
        let mut acc = 0usize;
        for (i, ch) in r.doc.lines[row].iter().enumerate() {
            let w = unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
            if dc < acc + w {
                return Some((row, i, true));
            }
            acc += w;
        }
        Some((row, r.doc.line_len(row).saturating_sub(1), false))
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        if self.boot.is_some() || self.cmd.is_some() {
            return;
        }
        let dir: isize = match ev.kind {
            MouseEventKind::ScrollDown => 1,
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                let new = if matches!(self.overlay, Overlay::None) {
                    self.screen_pos(ev.column, ev.row).filter(|p| p.2).map(|p| (p.0, p.1))
                } else {
                    None
                };
                if new != self.hover {
                    self.hover = new;
                    self.redraw_all = false;
                    self.blink = true;
                }
                return;
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if matches!(self.overlay, Overlay::None) && self.reader.is_some() {
                    if let Some((row, col, _)) = self.screen_pos(ev.column, ev.row) {
                        let r = rd!(self);
                        r.row = row;
                        r.col = col;
                        r.want = col;
                        self.after_move();
                    }
                }
                return;
            }
            _ => return,
        };
        match &mut self.overlay {
            Overlay::Picker(p) => {
                for _ in 0..3 {
                    p.key(if dir > 0 { K::Down } else { K::Up });
                }
            }
            Overlay::Help(s) => *s = (*s as isize + dir * 3).max(0) as usize,
            Overlay::Coll(_) => {
                if let Overlay::Coll(v) = std::mem::replace(&mut self.overlay, Overlay::None) {
                    self.coll_key(*v, if dir > 0 { K::Down } else { K::Up });
                }
            }
            Overlay::None => {
                self.hover = None;
                if self.reader.is_some() {
                    let vh = self.view_h();
                    let tw = self.geometry().1;
                    let r = rd!(self);
                    r.scroll_lines(dir > 0, vh, 3);
                    r.ensure_visible(vh, tw);
                    self.check_read();
                } else {
                    for _ in 0..3 {
                        self.browser_move(dir);
                    }
                }
            }
        }
    }

    fn help_key(&mut self, k: K, scroll: usize) {
        let page = self.view_h().saturating_sub(4);
        let max = crate::help::lines().len().saturating_sub(page);
        let new = match k {
            K::Esc | K::Char('q') | K::F1 => return,
            K::Char('j') | K::Down | K::Enter => scroll + 1,
            K::Char('k') | K::Up => scroll.saturating_sub(1),
            K::Ctrl('d') | K::PageDown | K::Char(' ') => scroll + page / 2,
            K::Ctrl('u') | K::PageUp => scroll.saturating_sub(page / 2),
            K::Char('g') | K::Home => 0,
            K::Char('G') | K::End => max,
            _ => scroll,
        };
        self.overlay = Overlay::Help(new.min(max));
    }

    // ---- browser ----------------------------------------------------------

    pub fn browser_select(&mut self, issue: u32, art: u32, pane: Pane) {
        if let Some(i) = self.lib.issue_index(issue) {
            self.browser.issue = i;
            self.browser.art = self.lib.issues[i].articles.iter().position(|a| a.num == art).unwrap_or(0);
            self.browser.pane = pane;
        }
    }

    pub fn browser_move(&mut self, d: isize) {
        match self.browser.pane {
            Pane::Issues => {
                let n = self.lib.issues.len();
                self.browser.issue = (self.browser.issue as isize + d).clamp(0, n as isize - 1) as usize;
                self.browser.art = 0;
            }
            Pane::Articles => {
                let n = self.lib.issues[self.browser.issue].articles.len();
                self.browser.art = (self.browser.art as isize + d).clamp(0, n as isize - 1) as usize;
            }
        }
    }

    fn browser_key(&mut self, k: K) {
        if !self.pending.is_empty() {
            let p = std::mem::take(&mut self.pending);
            return self.browser_prefixed(&p, k);
        }
        if let K::Char(c @ '0'..='9') = k {
            if c != '0' || self.count.is_some() {
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(c as usize - '0' as usize).min(99_999));
                return;
            }
        }
        let count = self.count.take();
        let n = count.unwrap_or(1) as isize;
        let half = (self.view_h() / 2).max(1) as isize;
        match k {
            K::Char('j') | K::Down => self.browser_move(n),
            K::Char('k') | K::Up => self.browser_move(-n),
            K::Ctrl('d') | K::PageDown => self.browser_move(half),
            K::Ctrl('u') | K::PageUp => self.browser_move(-half),
            K::Char('G') | K::End => match (count, self.browser.pane) {
                (Some(c), Pane::Issues) => {
                    let idx = self.lib.issues.iter().position(|i| i.num as usize >= c).unwrap_or(self.lib.issues.len() - 1);
                    self.browser.issue = idx;
                    self.browser.art = 0;
                }
                (Some(c), Pane::Articles) => self.browser.art = c.saturating_sub(1).min(self.lib.issues[self.browser.issue].articles.len() - 1),
                (None, Pane::Issues) => {
                    self.browser.issue = self.lib.issues.len() - 1;
                    self.browser.art = 0;
                }
                (None, Pane::Articles) => self.browser.art = self.lib.issues[self.browser.issue].articles.len() - 1,
            },
            K::Home => self.browser_prefixed("g", K::Char('g')),
            K::Char('l') | K::Right | K::Enter | K::Char('o') => match self.browser.pane {
                Pane::Issues => self.browser.pane = Pane::Articles,
                Pane::Articles => self.open_selected(),
            },
            K::Char('h') | K::Left | K::Backspace => self.browser.pane = Pane::Issues,
            K::Tab => {
                self.browser.pane = if self.browser.pane == Pane::Issues { Pane::Articles } else { Pane::Issues };
            }
            K::Char('x') => self.toggle_read_selected(),
            K::Char('/') | K::Ctrl('p') => self.open_finder(""),
            K::Char(':') => self.cmd = Some(CmdLine::plain(':', String::new())),
            K::Ctrl('o') => self.jump_back(),
            K::F1 => self.overlay = Overlay::Help(0),
            K::Ctrl('l') => self.redraw_all = true,
            K::Char('q') | K::Esc => {
                if self.browser.pane == Pane::Articles && k == K::Char('q') {
                    self.browser.pane = Pane::Issues;
                } else if k == K::Char('q') {
                    self.quit_app();
                }
            }
            K::Char(c @ ('g' | ']' | '[' | 'Z')) => {
                self.pending = c.to_string();
                self.count = count;
            }
            _ => {}
        }
    }

    fn browser_prefixed(&mut self, p: &str, k: K) {
        let count = self.count.take();
        match (p, k) {
            ("g", K::Char('g')) => match (count, self.browser.pane) {
                (Some(c), Pane::Issues) => {
                    self.browser.issue = self.lib.issues.iter().position(|i| i.num as usize >= c).unwrap_or(self.lib.issues.len() - 1);
                    self.browser.art = 0;
                }
                (Some(c), Pane::Articles) => self.browser.art = c.saturating_sub(1).min(self.lib.issues[self.browser.issue].articles.len() - 1),
                (None, Pane::Issues) => {
                    self.browser.issue = 0;
                    self.browser.art = 0;
                }
                (None, Pane::Articles) => self.browser.art = 0,
            },
            ("]", K::Char('i')) => self.browser_move_issue(count.unwrap_or(1) as isize),
            ("[", K::Char('i')) => self.browser_move_issue(-(count.unwrap_or(1) as isize)),
            ("]", K::Char('a')) => {
                self.browser.pane = Pane::Articles;
                self.browser_move(count.unwrap_or(1) as isize);
            }
            ("[", K::Char('a')) => {
                self.browser.pane = Pane::Articles;
                self.browser_move(-(count.unwrap_or(1) as isize));
            }
            ("Z", K::Char('Z' | 'Q')) => self.quit = true,
            ("g", K::Char(c)) => self.global_g(c),
            _ => {}
        }
    }

    fn browser_move_issue(&mut self, d: isize) {
        let n = self.lib.issues.len() as isize;
        self.browser.issue = (self.browser.issue as isize + d).clamp(0, n - 1) as usize;
        self.browser.art = 0;
    }

    fn open_selected(&mut self) {
        let i = &self.lib.issues[self.browser.issue];
        let (issue, art) = (i.num, i.articles[self.browser.art].num);
        self.open_article(issue, art, None);
    }

    fn toggle_read_selected(&mut self) {
        let i = &self.lib.issues[self.browser.issue];
        let key = (i.num, i.articles[self.browser.art].num);
        if !self.st.read.remove(&key) {
            self.st.read.insert(key);
        }
        self.st.save();
    }

    fn quit_app(&mut self) {
        self.save_pos();
        self.quit = true;
    }

    // ---- opening things -----------------------------------------------------

    pub fn save_pos(&mut self) {
        if let Some(r) = &self.reader {
            self.st.pos.insert((r.issue, r.art), r.row);
            self.st.last = Some((r.issue, r.art, r.row));
        }
        self.st.theme = Some(self.theme.name.to_string());
        self.st.save();
    }

    pub fn open_article(&mut self, issue: u32, num: u32, line: Option<usize>) -> bool {
        self.save_pos();
        let Some(a) = self.lib.article(issue, num) else {
            self.err(format!("no such phile: {issue}/{num}"));
            return false;
        };
        let lines = doc::load_lines(&a.path);
        let doc = Doc::build(lines, &self.lib, issue);
        let mut r = Reader::new(issue, num, doc);
        (r.snips, r.bms) = self.coll.sync(issue, num, &r.doc);
        let row = line.or_else(|| self.st.pos.get(&(issue, num)).copied()).unwrap_or(0);
        r.row = row.min(r.len() - 1);
        r.first_nonblank_if_empty();
        let vh = self.view_h();
        if r.row >= vh.saturating_sub(4) {
            r.center(vh);
        }
        self.reader = Some(r);
        self.browser_select(issue, num, Pane::Articles);
        self.st.last = Some((issue, num, row));
        self.reader_visible();
        self.check_read();
        self.pending.clear();
        self.count = None;
        true
    }

    pub fn open_issue_toc(&mut self, issue: u32) {
        if self.lib.issue(issue).is_none() {
            self.err(format!("no such issue: {issue}"));
            return;
        }
        if self.reader.is_some() {
            self.push_jump();
        }
        self.save_pos();
        self.reader = None;
        self.browser_select(issue, 0, Pane::Articles);
    }

    pub fn close_reader(&mut self) {
        if self.reader.is_none() {
            return;
        }
        self.push_jump();
        self.save_pos();
        let r = self.reader.take().unwrap();
        self.browser_select(r.issue, r.art, Pane::Articles);
    }

    pub fn check_read(&mut self) {
        let Some(r) = &self.reader else { return };
        let n = r.len();
        let done = n <= self.view_h() || r.row * 100 >= (n - 1) * 92;
        if done && self.st.read.insert((r.issue, r.art)) {
            self.st.save();
        }
    }

    fn cur_loc(&self) -> Option<Loc> {
        self.reader.as_ref().map(|r| Loc { issue: r.issue, art: r.art, row: r.row })
    }

    pub fn push_jump(&mut self) {
        let Some(c) = self.cur_loc() else { return };
        self.prev_loc = Some(c);
        let j = &mut self.jumps;
        j.list.truncate(j.pos);
        if j.list.last() != Some(&c) {
            j.list.push(c);
        }
        if j.list.len() > 100 {
            j.list.remove(0);
        }
        j.pos = j.list.len();
    }

    fn goto_loc(&mut self, l: Loc) {
        match &self.reader {
            Some(r) if r.issue == l.issue && r.art == l.art => {
                let vh = self.view_h();
                let r = rd!(self);
                r.goto_row(l.row);
                if l.row < r.top || l.row >= r.top + vh {
                    r.center(vh);
                }
                self.after_move();
            }
            _ => {
                self.open_article(l.issue, l.art, Some(l.row));
            }
        }
    }

    fn jump_back(&mut self) {
        let cur = self.cur_loc();
        let len = self.jumps.list.len();
        if self.jumps.pos >= len {
            if let Some(c) = cur {
                if self.jumps.list.last() != Some(&c) {
                    self.jumps.list.push(c);
                }
                self.jumps.pos = self.jumps.list.len() - 1;
            } else {
                self.jumps.pos = len;
            }
        }
        if self.jumps.pos == 0 {
            self.say("already at the oldest jump");
            return;
        }
        self.jumps.pos -= 1;
        let l = self.jumps.list[self.jumps.pos];
        self.goto_loc(l);
    }

    fn jump_forward(&mut self) {
        if self.jumps.pos + 1 >= self.jumps.list.len() {
            self.say("already at the newest jump");
            return;
        }
        self.jumps.pos += 1;
        let l = self.jumps.list[self.jumps.pos];
        self.goto_loc(l);
    }

    pub fn after_move(&mut self) {
        let (vh, tw) = (self.view_h(), self.geometry().1);
        if let Some(r) = self.reader.as_mut() {
            r.clamp();
            r.ensure_visible(vh, tw);
        }
        self.check_read();
    }

    /// Flat position of an article in reading order.
    fn order(&self) -> Vec<(u32, u32)> {
        self.lib.all_articles().map(|a| (a.issue, a.num)).collect()
    }

    fn current_key(&self) -> Option<(u32, u32)> {
        if let Some(r) = &self.reader {
            return Some((r.issue, r.art));
        }
        let i = &self.lib.issues[self.browser.issue];
        Some((i.num, i.articles.get(self.browser.art)?.num))
    }

    fn step_article(&mut self, d: isize) {
        let order = self.order();
        let Some(cur) = self.current_key() else { return };
        let Some(i) = order.iter().position(|&k| k == cur) else { return };
        let j = i as isize + d;
        if j < 0 || j as usize >= order.len() {
            self.say(if d > 0 { "last phile in the archive" } else { "first phile in the archive" });
            return;
        }
        let (iss, art) = order[j as usize];
        if self.reader.is_some() {
            self.push_jump();
        }
        let crossed = iss != cur.0;
        if self.open_article(iss, art, Some(0)) && crossed {
            self.say(format!("-- Phrack {iss} --"));
        }
    }

    fn step_issue(&mut self, d: isize) {
        let Some((cur, _)) = self.current_key() else { return };
        let Some(i) = self.lib.issue_index(cur) else { return };
        let j = i as isize + d;
        if j < 0 || j as usize >= self.lib.issues.len() {
            self.say(if d > 0 { "newest issue" } else { "first issue" });
            return;
        }
        let iss = &self.lib.issues[j as usize];
        let (n, a) = (iss.num, iss.articles[0].num);
        if self.reader.is_some() {
            self.push_jump();
        }
        self.open_article(n, a, Some(0));
        self.say(format!("-- Phrack {n} --"));
    }

    fn goto_special(&mut self, kind: Special) {
        let Some((issue, _)) = self.current_key() else { return };
        match self.lib.special(issue, kind).map(|a| (a.issue, a.num)) {
            Some((i, a)) => {
                if self.reader.is_some() {
                    self.push_jump();
                }
                self.open_article(i, a, Some(0));
            }
            None => self.err(format!("Phrack {issue} has no {}", kind.name())),
        }
    }

    fn goto_random(&mut self) {
        let order = self.order();
        let (i, a) = order[(self.rand() % order.len() as u64) as usize];
        if self.reader.is_some() {
            self.push_jump();
        }
        if self.open_article(i, a, Some(0)) {
            let t = self.lib.article(i, a).map(|x| x.title.clone()).unwrap_or_default();
            self.say(format!("cat /dev/random > {i}/{a}  \"{t}\""));
        }
    }

    fn goto_unread(&mut self) {
        let order = self.order();
        let cur = self.current_key();
        let start = cur.and_then(|c| order.iter().position(|&k| k == c)).map_or(0, |i| i + 1);
        let found = (0..order.len())
            .map(|o| order[(start + o) % order.len()])
            .find(|k| !self.st.read.contains(k) && Some(*k) != cur);
        match found {
            Some((i, a)) => {
                if self.reader.is_some() {
                    self.push_jump();
                }
                self.open_article(i, a, Some(0));
            }
            None => self.say("every phile read. you are elite."),
        }
    }

    fn follow(&mut self, r: &Ref) {
        if self.reader.is_some() {
            self.push_jump();
        }
        match r.art {
            Some(a) => {
                self.open_article(r.issue, a, Some(0));
            }
            None => {
                // Straight to the issue's contents; Ctrl-o brings you back.
                self.save_pos();
                self.reader = None;
                self.browser_select(r.issue, 0, Pane::Articles);
                self.say(format!("-> Phrack {}", r.issue));
            }
        }
    }

    // ---- reader keys ------------------------------------------------------

    fn reader_key(&mut self, k: K) {
        if rd!(self).hints.is_some() {
            return self.hint_key(k);
        }
        if !self.pending.is_empty() {
            let p = std::mem::take(&mut self.pending);
            return self.reader_prefixed(&p, k);
        }
        if let K::Char(c @ '0'..='9') = k {
            if c != '0' || self.count.is_some() {
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(c as usize - '0' as usize).min(999_999));
                return;
            }
        }
        let count = self.count.take();
        let n = count.unwrap_or(1);
        let vh = self.view_h();
        match k {
            K::Char('j') | K::Down | K::Ctrl('j') | K::Ctrl('n') => rd!(self).down(n),
            K::Char('k') | K::Up | K::Ctrl('k') => rd!(self).up(n),
            K::Char('h') | K::Left | K::Backspace => rd!(self).left(n),
            K::Char('l') | K::Right => rd!(self).right(n),
            K::Char('0') | K::Home => rd!(self).line_start(),
            K::Char('^') => rd!(self).first_nonblank(),
            K::Char('$') | K::End => rd!(self).line_end(),
            K::Char('w') => rd!(self).word_fwd(false, n),
            K::Char('W') => rd!(self).word_fwd(true, n),
            K::Char('b') => rd!(self).word_back(false, n),
            K::Char('B') => rd!(self).word_back(true, n),
            K::Char('e') => rd!(self).word_end(false, n),
            K::Char('E') => rd!(self).word_end(true, n),
            K::Char('G') => {
                self.push_jump();
                let r = rd!(self);
                let row = count.map_or(r.len() - 1, |c| c.saturating_sub(1));
                r.goto_line_first_nonblank(row);
            }
            K::Char(c @ ('H' | 'M' | 'L')) => rd!(self).screen_row(c, vh, n),
            K::Ctrl('d') => rd!(self).half_page(true, vh, n),
            K::Ctrl('u') => rd!(self).half_page(false, vh, n),
            K::Ctrl('f') | K::PageDown | K::Char(' ') => rd!(self).page(true, vh, n),
            K::Ctrl('b') | K::PageUp => rd!(self).page(false, vh, n),
            K::Ctrl('e') => rd!(self).scroll_lines(true, vh, n),
            K::Ctrl('y') => rd!(self).scroll_lines(false, vh, n),
            K::Char('}') => rd!(self).para(true, n),
            K::Char('{') => rd!(self).para(false, n),
            K::Char('n') => self.search_next(true, n),
            K::Char('N') => self.search_next(false, n),
            K::Char('*') => self.search_word(true),
            K::Char('#') => self.search_word(false),
            K::Char('/') => self.cmd = Some(CmdLine::plain('/', String::new())),
            K::Char('?') => self.cmd = Some(CmdLine::plain('?', String::new())),
            K::Char(':') => {
                let text = if rd!(self).visual.is_some() { "'<,'>".to_string() } else { String::new() };
                rd!(self).visual = None;
                self.cmd = Some(CmdLine::plain(':', text));
            }
            K::Char('v') => self.toggle_visual(VMode::Char),
            K::Char('V') => self.toggle_visual(VMode::Line),
            K::Char('y') if rd!(self).visual.is_some() => self.yank_visual(),
            K::Char('s') if rd!(self).visual.is_some() => self.begin_snippet(),
            K::Char('Y') => self.yank_lines(n),
            K::Esc => {
                let r = rd!(self);
                if r.visual.take().is_none() {
                    r.hl = false;
                }
            }
            K::Enter => {
                let r = rd!(self);
                let row = r.row;
                let col = r.col;
                let refs: Vec<Ref> = r.doc.refs_on(row).cloned().collect();
                let hit = refs.iter().find(|x| col >= x.col && col < x.col + x.len).or_else(|| refs.first()).cloned();
                match hit {
                    Some(x) => self.follow(&x),
                    None => rd!(self).down(n),
                }
            }
            K::Char('f') => self.start_hints(),
            K::Char('q') => self.close_reader(),
            K::Ctrl('o') => self.jump_back(),
            K::Tab => self.jump_forward(),
            K::Ctrl('p') => self.open_finder(""),
            K::Ctrl('l') => self.redraw_all = true,
            K::Ctrl('g') => self.file_info(),
            K::F1 => self.overlay = Overlay::Help(0),
            K::Char(c @ ('g' | ']' | '[' | 'z' | 'm' | '\'' | '`' | 'Z' | 'y')) => {
                self.pending = c.to_string();
                self.count = count;
                return;
            }
            _ => {}
        }
        if self.reader.is_some() {
            self.after_move();
        }
    }

    fn reader_prefixed(&mut self, p: &str, k: K) {
        let count = self.count.take();
        let n = count.unwrap_or(1);
        let vh = self.view_h();
        let K::Char(c) = k else { return };
        match (p, c) {
            ("g", 'g') => {
                self.push_jump();
                let r = rd!(self);
                r.goto_line_first_nonblank(count.map_or(0, |c| c.saturating_sub(1)));
            }
            ("g", 'f') => {
                let r = rd!(self);
                let (row, col) = (r.row, r.col);
                let refs: Vec<Ref> = r.doc.refs_on(row).cloned().collect();
                match refs.iter().find(|x| col >= x.col && col < x.col + x.len).or_else(|| refs.first()).cloned() {
                    Some(x) => return self.follow(&x),
                    None => self.err("no Phrack reference on this line (try f for hints)"),
                }
            }
            ("g", 'i' | 't') => {
                let i = rd!(self).issue;
                return self.open_issue_toc(i);
            }
            ("g", 'I') => return self.goto_special(Special::Intro),
            ("g", 'l') => return self.goto_special(Special::Loopback),
            ("g", 'n') => return self.goto_special(Special::Linenoise),
            ("g", 'p') => return self.goto_special(Special::Prophile),
            ("g", 'w') => return self.goto_special(Special::WorldNews),
            ("g", 'x') => return self.goto_special(Special::Index),
            ("g", 'A') => return self.open_authors(),
            ("g", 'a') => return self.open_author_of_current(),
            ("g", 's') => return self.open_outline(),
            ("g", 'm') => return self.open_marks(),
            ("g", 'b') => return self.toggle_bookmark_here(""),
            ("g", 'B') => return self.open_coll(Tab::Bookmarks),
            ("g", 'S') => return self.open_coll(Tab::Snippets),
            ("g", 'X') => return self.delete_snippet_here(),
            ("]", 'b') => return self.step_bookmark(true, n),
            ("[", 'b') => return self.step_bookmark(false, n),
            ("]", 's') => return self.step_snippet(true, n),
            ("[", 's') => return self.step_snippet(false, n),
            ("g", 'u') => return self.goto_unread(),
            ("g", 'r') => return self.goto_random(),
            ("g", '/') => {
                self.cmd = Some(CmdLine::plain(':', "grep ".into()));
                return;
            }
            ("g", '?') => {
                self.overlay = Overlay::Help(0);
                return;
            }
            ("]", ']') => self.section(true, n),
            ("[", '[') => self.section(false, n),
            ("]", 'a') => return self.step_article(n as isize),
            ("[", 'a') => return self.step_article(-(n as isize)),
            ("]", 'i') => return self.step_issue(n as isize),
            ("[", 'i') => return self.step_issue(-(n as isize)),
            ("]", 'r') | ("]", 'f') => {
                // next reference below the cursor
                let r = rd!(self);
                let pos = (r.row, r.col);
                if let Some(x) = r.doc.refs.iter().find(|x| (x.row, x.col) > pos).cloned() {
                    r.row = x.row;
                    r.col = x.col;
                    r.want = x.col;
                } else {
                    self.err("no more references");
                }
            }
            ("[", 'r') | ("[", 'f') => {
                let r = rd!(self);
                let pos = (r.row, r.col);
                if let Some(x) = r.doc.refs.iter().rev().find(|x| (x.row, x.col) < pos).cloned() {
                    r.row = x.row;
                    r.col = x.col;
                    r.want = x.col;
                } else {
                    self.err("no earlier references");
                }
            }
            ("z", c) => match c {
                'h' => rd!(self).left(n * 8),
                'l' => rd!(self).right(n * 8),
                'H' => {
                    let half = self.view_w() / 2;
                    rd!(self).left(half)
                }
                'L' => {
                    let half = self.view_w() / 2;
                    rd!(self).right(half)
                }
                other => {
                    rd!(self).z(other, vh);
                    if matches!(other, 'z' | 't' | 'b' | '.' | '-') {
                        // z-commands set `top` explicitly; don't let scrolloff undo them
                        return;
                    }
                }
            },
            ("m", c) if c.is_ascii_alphabetic() => self.set_mark(c),
            ("'" | "`", c) => self.jump_mark(c),
            ("Z", 'Z' | 'Q') => return self.quit_app(),
            ("y", 'y') => return self.yank_lines(n),
            _ => {}
        }
        if self.reader.is_some() {
            self.after_move();
        }
    }

    fn global_g(&mut self, c: char) {
        match c {
            'r' => self.goto_random(),
            'u' => self.goto_unread(),
            'A' => self.open_authors(),
            'm' => self.open_marks(),
            'B' => self.open_coll(Tab::Bookmarks),
            'S' => self.open_coll(Tab::Snippets),
            '?' => self.overlay = Overlay::Help(0),
            '/' => self.cmd = Some(CmdLine::plain(':', "grep ".into())),
            'I' => self.goto_special(Special::Intro),
            'l' => self.goto_special(Special::Loopback),
            'n' => self.goto_special(Special::Linenoise),
            'p' => self.goto_special(Special::Prophile),
            'w' => self.goto_special(Special::WorldNews),
            'x' => self.goto_special(Special::Index),
            _ => {}
        }
    }

    fn section(&mut self, fwd: bool, n: usize) {
        self.push_jump();
        let vh = self.view_h();
        let r = rd!(self);
        if r.heading(fwd, n) {
            // park the heading near the top, like `zt` with a little context
            r.top = r.row.saturating_sub(2);
            let _ = vh;
        } else {
            self.say(if fwd { "no more sections" } else { "no earlier sections" });
        }
    }

    fn file_info(&mut self) {
        let vh = self.view_h();
        let r = rd!(self);
        let a = self.lib.article(r.issue, r.art);
        let t = a.map(|a| format!("{} — {}", a.title, a.author)).unwrap_or_default();
        let m = format!("Phrack {}/{} \"{}\" {} lines --{}--", r.issue, r.art, t, r.len(), r.percent(vh));
        self.say(m);
    }

    // ---- search -----------------------------------------------------------

    fn search_next(&mut self, same_dir: bool, n: usize) {
        let Some(fwd) = rd!(self).search.as_ref().map(|s| s.fwd) else {
            self.err("no previous search pattern");
            return;
        };
        self.push_jump();
        let r = rd!(self);
        r.hl = true;
        let pat = r.search.as_ref().map(|s| s.pat.clone()).unwrap_or_default();
        match r.search_step(if same_dir { fwd } else { !fwd }, n) {
            Some(true) => self.say(format!("search hit {}, continuing at {}  /{pat}", if fwd == same_dir { "BOTTOM" } else { "TOP" }, if fwd == same_dir { "TOP" } else { "BOTTOM" })),
            Some(false) => self.say(format!("/{pat}")),
            None => self.err(format!("Pattern not found: {pat}")),
        }
    }

    fn search_word(&mut self, fwd: bool) {
        let Some(w) = rd!(self).word_under_cursor() else {
            self.err("no word under cursor");
            return;
        };
        let pat = format!(r"\b{}\b", regex::escape(&w));
        rd!(self).search = Some(Search::new(&pat, fwd));
        // step off the current match
        self.search_next(true, 1);
    }

    fn submit_search(&mut self, fwd: bool, text: &str) {
        let r = rd!(self);
        if text.is_empty() {
            match r.search.as_mut() {
                Some(s) => s.fwd = fwd,
                None => {
                    self.err("no previous search pattern");
                    return;
                }
            }
        } else {
            r.search = Some(Search::new(text, fwd));
        }
        self.search_next(true, 1);
    }

    // ---- visual / yank ------------------------------------------------------

    fn toggle_visual(&mut self, mode: VMode) {
        let r = rd!(self);
        r.visual = match r.visual {
            Some((m, _)) if m == mode => None,
            Some((_, a)) => Some((mode, a)),
            None => Some((mode, (r.row, r.col))),
        };
    }

    fn yank_visual(&mut self) {
        let r = rd!(self);
        let text = r.selection_text().unwrap_or_default();
        let lines = text.lines().count().max(1);
        r.visual = None;
        self.clip(&text, lines);
    }

    fn yank_lines(&mut self, n: usize) {
        let r = rd!(self);
        let end = (r.row + n).min(r.len());
        let text = (r.row..end).map(|i| r.doc.line_string(i)).collect::<Vec<_>>().join("\n");
        self.clip(&text, end - self.reader.as_ref().unwrap().row);
    }

    fn clip(&mut self, text: &str, lines: usize) {
        if copy_to_clipboard(text) {
            self.say(format!("{lines} line{} yanked", if lines == 1 { "" } else { "s" }));
        } else {
            self.err("no clipboard tool found; sent OSC 52 to the terminal instead");
        }
    }

    // ---- hints (vimium-style reference following) ---------------------------

    fn start_hints(&mut self) {
        let vh = self.view_h();
        let r = rd!(self);
        let lo = r.top;
        let hi = (r.top + vh).min(r.len());
        let letters = "asdfghjklqwertyuiopzxcvbnm";
        let refs: Vec<Ref> = r.doc.refs.iter().filter(|x| x.row >= lo && x.row < hi).cloned().collect();
        if refs.is_empty() {
            self.err("no Phrack references on screen");
            return;
        }
        let hints: Vec<(Ref, char)> = refs.into_iter().zip(letters.chars()).collect();
        r.hints = Some(hints);
    }

    fn hint_key(&mut self, k: K) {
        let hints = rd!(self).hints.take().unwrap_or_default();
        if let K::Char(c) = k {
            if let Some((r, _)) = hints.iter().find(|(_, l)| *l == c) {
                let r = r.clone();
                self.follow(&r);
            }
        }
    }

    // ---- marks ----------------------------------------------------------------

    fn set_mark(&mut self, c: char) {
        let r = rd!(self);
        let (issue, art, row) = (r.issue, r.art, r.row);
        if c.is_ascii_uppercase() {
            self.st.global_marks.insert(c, (issue, art, row));
        } else {
            self.st.local_marks.insert((issue, art, c), row);
        }
        self.st.save();
        self.say(format!("mark '{c} set at {issue}/{art}:{}", row + 1));
    }

    fn jump_mark(&mut self, c: char) {
        let (issue, art) = {
            let r = rd!(self);
            (r.issue, r.art)
        };
        if c == '\'' || c == '`' {
            match self.prev_loc.take() {
                Some(l) => {
                    self.push_jump();
                    self.goto_loc(l);
                }
                None => self.err("no previous position"),
            }
            return;
        }
        if c.is_ascii_uppercase() {
            match self.st.global_marks.get(&c).copied() {
                Some((i, a, row)) => {
                    self.push_jump();
                    self.goto_loc(Loc { issue: i, art: a, row });
                }
                None => self.err(format!("mark '{c} not set")),
            }
        } else {
            match self.st.local_marks.get(&(issue, art, c)).copied() {
                Some(row) => {
                    self.push_jump();
                    let vh = self.view_h();
                    let r = rd!(self);
                    r.goto_line_first_nonblank(row);
                    if row < r.top || row >= r.top + vh {
                        r.center(vh);
                    }
                }
                None => self.err(format!("mark '{c} not set in this phile")),
            }
        }
    }

    // ---- pickers --------------------------------------------------------------

    pub fn open_finder(&mut self, prefill: &str) {
        let items: Vec<PickItem> = self
            .lib
            .all_articles()
            .map(|a| PickItem {
                label: format!("{:>2}.{:<2}", a.issue, a.num),
                detail: if a.author.is_empty() { a.title.clone() } else { format!("{}  — {}", a.title, a.author) },
                target: Target::Article { issue: a.issue, art: a.num, line: None },
            })
            .collect();
        let n = items.len();
        self.overlay = Overlay::Picker(Box::new(Picker::new(format!("FIND PHILE  ({n})"), items, true).with_filter(prefill)));
    }

    fn open_authors(&mut self) {
        let items: Vec<PickItem> = self
            .lib
            .authors()
            .into_iter()
            .map(|(name, arts)| PickItem {
                label: name.clone(),
                detail: format!("{} phile{}", arts.len(), if arts.len() == 1 { "" } else { "s" }),
                target: Target::Author(name),
            })
            .collect();
        let n = items.len();
        self.overlay = Overlay::Picker(Box::new(Picker::new(format!("AUTHORS  ({n})"), items, true)));
    }

    fn open_author_of_current(&mut self) {
        let Some((i, a)) = self.current_key() else { return };
        let Some(art) = self.lib.article(i, a) else { return };
        let names = crate::data::split_authors(&art.author);
        match names.as_slice() {
            [] => self.err("no author recorded"),
            [one] => self.open_author_articles(one),
            many => {
                let items = many
                    .iter()
                    .map(|n| PickItem { label: n.clone(), detail: String::new(), target: Target::Author(n.clone()) })
                    .collect();
                self.overlay = Overlay::Picker(Box::new(Picker::new("AUTHORS OF THIS PHILE", items, false)));
            }
        }
    }

    fn open_author_articles(&mut self, name: &str) {
        let items: Vec<PickItem> = self
            .lib
            .articles_by(name)
            .into_iter()
            .map(|a| PickItem {
                label: format!("{:>2}.{:<2}", a.issue, a.num),
                detail: a.title.clone(),
                target: Target::Article { issue: a.issue, art: a.num, line: None },
            })
            .collect();
        if items.is_empty() {
            self.err(format!("nothing by {name}"));
            return;
        }
        let n = items.len();
        self.overlay = Overlay::Picker(Box::new(Picker::new(format!("{}  ({n})", name.to_uppercase()), items, false)));
    }

    fn open_outline(&mut self) {
        let r = rd!(self);
        if r.doc.headings.is_empty() {
            self.err("no section headings detected in this phile");
            return;
        }
        let items: Vec<PickItem> = r
            .doc
            .headings
            .iter()
            .map(|h| PickItem { label: format!("{:>5}", h.row + 1), detail: h.title.clone(), target: Target::Line(h.row) })
            .collect();
        let cur = r.current_heading().map(|h| h.row);
        let mut p = Picker::new("SECTIONS", items, false);
        if let Some(c) = cur {
            p.sel = p.items.iter().position(|i| i.target == Target::Line(c)).unwrap_or(0);
        }
        self.overlay = Overlay::Picker(Box::new(p));
    }

    fn open_marks(&mut self) {
        let mut items = Vec::new();
        let mut g: Vec<_> = self.st.global_marks.iter().collect();
        g.sort();
        for (c, (i, a, row)) in g {
            let t = self.lib.article(*i, *a).map(|x| x.title.clone()).unwrap_or_default();
            items.push(PickItem {
                label: format!("'{c}"),
                detail: format!("{i}.{a}:{}  {t}", row + 1),
                target: Target::Article { issue: *i, art: *a, line: Some(*row) },
            });
        }
        if let Some(r) = &self.reader {
            let mut l: Vec<_> = self.st.local_marks.iter().filter(|((i, a, _), _)| *i == r.issue && *a == r.art).collect();
            l.sort();
            for ((_, _, c), row) in l {
                items.push(PickItem { label: format!("'{c}"), detail: format!("line {}", row + 1), target: Target::Line(*row) });
            }
        }
        if items.is_empty() {
            self.err("no marks set (m{a-z} local, m{A-Z} global)");
            return;
        }
        self.overlay = Overlay::Picker(Box::new(Picker::new("MARKS", items, false)));
    }

    fn pick_select(&mut self, t: Target, pattern: Option<String>) {
        match t {
            Target::Author(name) => self.open_author_articles(&name),
            Target::Line(row) => {
                if self.reader.is_some() {
                    self.push_jump();
                    let vh = self.view_h();
                    let r = rd!(self);
                    r.goto_line_first_nonblank(row);
                    r.top = row.saturating_sub(vh / 3);
                    self.after_move();
                }
            }
            Target::Article { issue, art, line } => {
                if self.reader.is_some() {
                    self.push_jump();
                }
                if self.open_article(issue, art, line) {
                    if let Some(p) = pattern {
                        let r = rd!(self);
                        r.search = Some(Search::new(&p, true));
                        r.hl = true;
                        if let Some(l) = line {
                            r.row = l.min(r.len() - 1);
                            r.col = r.line_matches(r.row).first().map_or(0, |m| m.0);
                            r.want = r.col;
                        }
                        self.after_move();
                    }
                }
            }
        }
    }

    pub fn start_grep(&mut self, pat: &str) {
        if pat.trim().is_empty() {
            self.err("usage: :grep PATTERN");
            return;
        }
        let re = build_regex(pat);
        let files: Vec<(u32, u32, PathBuf)> = self.lib.all_articles().map(|a| (a.issue, a.num, a.path.clone())).collect();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut total = 0usize;
            for (i, n, path) in files {
                let lines = doc::load_lines(&path);
                let mut chunk = Vec::new();
                for (row, l) in lines.iter().enumerate() {
                    if re.is_match(l) {
                        chunk.push(PickItem {
                            label: format!("{i}.{n}:{}", row + 1),
                            detail: l.trim().to_string(),
                            target: Target::Article { issue: i, art: n, line: Some(row) },
                        });
                    }
                }
                total += chunk.len();
                if !chunk.is_empty() && tx.send(chunk).is_err() {
                    return;
                }
                if total >= 5000 {
                    return;
                }
            }
        });
        let mut p = Picker::new(format!("GREP /{pat}/"), Vec::new(), false);
        p.rx = Some(rx);
        p.searching = true;
        p.pattern = Some(pat.to_string());
        self.overlay = Overlay::Picker(Box::new(p));
    }

    // ---- command line -----------------------------------------------------------

    fn cmdline_key(&mut self, k: K) {
        let Some(c) = self.cmd.as_mut() else { return };
        match k {
            K::Esc | K::Ctrl('c') => self.cmd = None,
            K::Backspace => {
                if c.text.pop().is_none() {
                    self.cmd = None;
                }
            }
            K::Ctrl('u') => c.text.clear(),
            K::Ctrl('w') => {
                let t = c.text.trim_end().rfind(' ').map_or(0, |i| i + 1);
                c.text.truncate(t);
            }
            K::Char(ch) => c.text.push(ch),
            K::Enter => {
                let CmdLine { prefix, text, ctx } = self.cmd.take().unwrap();
                match ctx {
                    CmdCtx::NewSnippet => self.save_snippet(text.trim()),
                    CmdCtx::SnipName(id) => self.edit_snippet(id, Some(text.trim()), None),
                    CmdCtx::SnipNote(id) => self.edit_snippet(id, None, Some(text.trim())),
                    CmdCtx::BmNote(id) => self.edit_bookmark_note(id, text.trim()),
                    CmdCtx::None => {}
                }
                match prefix {
                    _ if ctx != CmdCtx::None => {}
                    ':' => self.exec(text.trim()),
                    '/' if self.reader.is_some() => self.submit_search(true, &text),
                    '?' if self.reader.is_some() => self.submit_search(false, &text),
                    _ => {}
                }
                if self.reader.is_some() {
                    self.after_move();
                }
            }
            _ => {}
        }
    }

    pub fn exec(&mut self, line: &str) {
        let line = line.trim_start_matches(|c: char| c == ':' || c == ' ');
        let line = line.strip_prefix("'<,'>").unwrap_or(line).trim();
        if line.is_empty() {
            return;
        }
        let (cmd, arg) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
        // :N  -> line (reader) or issue (browser);  :I/A or :I.A -> article
        if let Ok(n) = cmd.parse::<usize>() {
            if self.reader.is_some() {
                self.push_jump();
                let vh = self.view_h();
                let r = rd!(self);
                r.goto_line_first_nonblank(n.saturating_sub(1));
                r.top = r.row.saturating_sub(vh / 3);
            } else if let Some(i) = self.lib.issues.iter().position(|i| i.num as usize >= n) {
                self.browser.issue = i;
                self.browser.art = 0;
            }
            return;
        }
        if let Some((a, b)) = cmd.split_once(['/', '.']) {
            if let (Ok(i), Ok(n)) = (a.parse::<u32>(), b.parse::<u32>()) {
                if self.reader.is_some() {
                    self.push_jump();
                }
                self.open_article(i, n, None);
                return;
            }
        }
        match cmd {
            "q" | "quit" | "close" | "clo" => {
                if self.reader.is_some() {
                    self.close_reader();
                } else {
                    self.quit_app();
                }
            }
            "qa" | "qa!" | "q!" | "wq" | "wqa" | "x" | "xa" | "exit" => self.quit_app(),
            "w" | "write" => {
                self.save_pos();
                self.say("state saved");
            }
            "help" | "h" | "?" => self.overlay = Overlay::Help(0),
            "issue" | "i" | "toc" | "t" => {
                let cur = self.current_key().map(|k| k.0).unwrap_or(1);
                match arg.parse::<u32>() {
                    Ok(n) => self.open_issue_toc(n),
                    Err(_) if arg.is_empty() => self.open_issue_toc(cur),
                    Err(_) => self.err("usage: :issue N"),
                }
            }
            "open" | "o" | "e" | "edit" => {
                let nums: Vec<u32> = arg.split(|c: char| !c.is_ascii_digit()).filter_map(|s| s.parse().ok()).collect();
                match nums.as_slice() {
                    [i] => self.open_issue_toc(*i),
                    [i, a, ..] => {
                        if self.reader.is_some() {
                            self.push_jump();
                        }
                        self.open_article(*i, *a, None);
                    }
                    _ => self.err("usage: :open ISSUE/PHILE"),
                }
            }
            "find" | "f" | "files" | "ls" | "fzf" => self.open_finder(arg),
            "grep" | "g" | "vimgrep" | "vim" | "rg" => self.start_grep(arg),
            "author" | "au" | "authors" => {
                if arg.is_empty() {
                    self.open_authors();
                } else {
                    self.open_author_articles(arg);
                }
            }
            "sections" | "outline" | "sec" if self.reader.is_some() => self.open_outline(),
            "marks" => self.open_marks(),
            "bookmark" | "bm" | "bmark" => self.toggle_bookmark_here(arg),
            "bookmarks" | "bookmarklist" | "bms" => self.open_coll(Tab::Bookmarks),
            "snippets" | "snips" | "snippet" | "snip" => self.open_coll(Tab::Snippets),
            "delsnip" | "delsnippet" => match arg.parse::<u32>() {
                Ok(id) => {
                    if self.coll.remove_snippet(id) {
                        self.refresh_marks();
                        self.say(format!("snippet #{id} deleted"));
                    } else {
                        self.err(format!("no snippet #{id}"));
                    }
                }
                Err(_) => self.delete_snippet_here(),
            },
            "delbm" | "delbookmark" => match arg.parse::<u32>() {
                Ok(id) => {
                    if self.coll.remove_bookmark(id) {
                        self.refresh_marks();
                        self.say(format!("bookmark #{id} deleted"));
                    } else {
                        self.err(format!("no bookmark #{id}"));
                    }
                }
                Err(_) => self.err("usage: :delbm ID  (or gb on the line to toggle)"),
            },
            "delmarks" | "delm" => {
                if arg == "!" || arg.is_empty() {
                    self.st.global_marks.clear();
                    self.st.local_marks.clear();
                } else {
                    for c in arg.chars() {
                        self.st.global_marks.remove(&c);
                        self.st.local_marks.retain(|(_, _, m), _| *m != c);
                    }
                }
                self.st.save();
                self.say("marks deleted");
            }
            "random" | "rand" => self.goto_random(),
            "unread" | "next" | "n" => self.goto_unread(),
            "intro" => self.goto_special(Special::Intro),
            "loopback" => self.goto_special(Special::Loopback),
            "linenoise" => self.goto_special(Special::Linenoise),
            "prophile" => self.goto_special(Special::Prophile),
            "pwn" | "worldnews" => self.goto_special(Special::WorldNews),
            "markread" => {
                if let Some(k) = self.current_key() {
                    self.st.read.insert(k);
                    self.st.save();
                }
            }
            "markunread" => {
                if let Some(k) = self.current_key() {
                    self.st.read.remove(&k);
                    self.st.save();
                }
            }
            "noh" | "nohlsearch" | "noh!" => {
                if let Some(r) = self.reader.as_mut() {
                    r.hl = false;
                }
            }
            "theme" | "colorscheme" | "colo" => self.set_theme(arg),
            "set" | "se" => {
                for opt in arg.split_whitespace() {
                    self.set_option(opt);
                }
            }
            "nu" | "number" => self.set_option("nu"),
            "nonu" => self.set_option("nonu"),
            "reg" | "registers" => {}
            other => self.err(format!("Not an editor command: {other}")),
        }
    }

    // ---- bookmarks & snippets -------------------------------------------------

    pub fn refresh_marks(&mut self) {
        if let Some(r) = self.reader.as_mut() {
            (r.snips, r.bms) = self.coll.sync(r.issue, r.art, &r.doc);
        }
    }

    /// `gb` / `:bookmark [note]`: toggle a bookmark on the cursor line. With a
    /// note on an existing bookmark, the note is updated instead.
    fn toggle_bookmark_here(&mut self, note: &str) {
        let Some(r) = &self.reader else {
            return self.err("open a phile to bookmark a line");
        };
        let (issue, art, row) = (r.issue, r.art, r.row);
        let text = r.doc.line_string(row);
        if !note.is_empty() {
            if let Some(id) = self.coll.bookmark_at(issue, art, row).map(|b| b.id) {
                return self.edit_bookmark_note(id, note);
            }
        }
        if self.coll.toggle_bookmark(issue, art, row, &text, note) {
            let id = self.coll.bookmark_at(issue, art, row).map_or(0, |b| b.id);
            self.say(format!("bookmark #{id} added at {issue}/{art}:{}   (gB to browse)", row + 1));
        } else {
            self.say(format!("bookmark removed from {issue}/{art}:{}", row + 1));
        }
        self.refresh_marks();
    }

    fn edit_bookmark_note(&mut self, id: u32, note: &str) {
        if let Some(b) = self.coll.bookmarks.iter_mut().find(|b| b.id == id) {
            b.note = note.to_string();
            self.coll.save();
            self.say(format!("bookmark #{id} note updated"));
        }
    }

    fn edit_snippet(&mut self, id: u32, name: Option<&str>, note: Option<&str>) {
        if let Some(sn) = self.coll.snippets.iter_mut().find(|s| s.id == id) {
            if let Some(n) = name.filter(|n| !n.is_empty()) {
                sn.name = n.to_string();
            }
            if let Some(n) = note {
                sn.note = n.to_string();
            }
            self.coll.save();
            self.say(format!("snippet #{id} updated"));
        }
    }

    /// `s` in visual mode: ask for a name, keeping the selection on screen.
    fn begin_snippet(&mut self) {
        let r = rd!(self);
        let Some(text) = r.selection_text() else { return };
        let auto: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect();
        self.cmd = Some(CmdLine { prefix: 'S', text: auto, ctx: CmdCtx::NewSnippet });
    }

    fn save_snippet(&mut self, name: &str) {
        let r = rd!(self);
        let Some((mode, s, e)) = r.selection_range() else { return };
        let Some(text) = r.selection_text() else { return };
        let line = mode == VMode::Line;
        let (issue, art) = (r.issue, r.art);
        r.visual = None;
        let name = if name.is_empty() {
            text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect()
        } else {
            name.to_string()
        };
        let (s, e) = if line { ((s.0, 0), (e.0, r.doc.line_len(e.0))) } else { (s, e) };
        let id = self.coll.add_snippet(Snippet { id: 0, issue, art, line, s, e, name: name.clone(), note: String::new(), text });
        self.refresh_marks();
        self.say(format!("snippet #{id} \"{name}\" saved   (gS to browse)"));
    }

    fn delete_snippet_here(&mut self) {
        let Some(r) = &self.reader else { return };
        match r.snip_at(r.row, r.col).map(|s| s.id) {
            Some(id) => {
                self.coll.remove_snippet(id);
                self.refresh_marks();
                self.say(format!("snippet #{id} deleted"));
            }
            None => self.err("no snippet under the cursor"),
        }
    }

    fn step_bookmark(&mut self, fwd: bool, n: usize) {
        let r = rd!(self);
        let mut rows: Vec<usize> = r.bms.iter().map(|b| b.0).collect();
        rows.sort();
        let mut cur = r.row;
        for _ in 0..n.max(1) {
            let next = if fwd { rows.iter().copied().find(|&x| x > cur) } else { rows.iter().rev().copied().find(|&x| x < cur) };
            match next {
                Some(x) => cur = x,
                None => break,
            }
        }
        if cur == r.row {
            self.say(if fwd { "no more bookmarks in this phile" } else { "no earlier bookmarks in this phile" });
        } else {
            self.push_jump();
            let vh = self.view_h();
            let r = rd!(self);
            r.goto_line_first_nonblank(cur);
            if cur < r.top || cur >= r.top + vh {
                r.center(vh);
            }
        }
    }

    fn step_snippet(&mut self, fwd: bool, n: usize) {
        let r = rd!(self);
        let mut starts: Vec<(usize, usize)> = r.snips.iter().map(|s| s.s).collect();
        starts.sort();
        let mut cur = (r.row, r.col);
        let from = cur;
        for _ in 0..n.max(1) {
            let next = if fwd { starts.iter().copied().find(|&x| x > cur) } else { starts.iter().rev().copied().find(|&x| x < cur) };
            match next {
                Some(x) => cur = x,
                None => break,
            }
        }
        if cur == from {
            self.say(if fwd { "no more snippets in this phile" } else { "no earlier snippets in this phile" });
        } else {
            self.push_jump();
            let vh = self.view_h();
            let r = rd!(self);
            r.row = cur.0;
            r.col = cur.1;
            r.want = cur.1;
            if cur.0 < r.top || cur.0 >= r.top + vh {
                r.center(vh);
            }
        }
    }

    fn set_theme(&mut self, name: &str) {
        let names: Vec<&str> = theme::all().iter().map(|t| t.name).collect();
        if name.is_empty() {
            self.say(format!("theme: {} (available: {})", self.theme.name, names.join(", ")));
            return;
        }
        match theme::by_name(name) {
            Some(t) => {
                self.theme = t;
                self.st.theme = Some(self.theme.name.to_string());
                self.st.save();
                self.redraw_all = true;
            }
            None => self.err(format!("unknown theme '{name}' (available: {})", names.join(", "))),
        }
    }

    fn set_option(&mut self, opt: &str) {
        if let Some(t) = opt.strip_prefix("theme=").or_else(|| opt.strip_prefix("colorscheme=")) {
            return self.set_theme(t);
        }
        let (name, on) = match opt.strip_prefix("no") {
            Some(rest) if !rest.is_empty() && rest != "h" => (rest, false),
            _ => (opt, true),
        };
        match name {
            "nu" | "number" => self.nu = on,
            "rnu" | "relativenumber" => self.rnu = on,
            "crt" => {
                self.fx = if on { Fx::default() } else { Fx::none() };
            }
            "scan" | "scanlines" => self.fx.scan = on,
            "glow" | "bloom" => self.fx.glow = on,
            "vignette" | "bulge" => self.fx.vignette = on,
            "flicker" => self.fx.flicker = on,
            "hls" | "hlsearch" => {
                if let Some(r) = self.reader.as_mut() {
                    r.hl = on;
                }
            }
            other => self.err(format!("Unknown option: {other}")),
        }
        self.reader_visible();
    }
}

impl crate::reader::Reader {
    /// If the restored row doesn't exist any more, fall back to the top.
    pub fn first_nonblank_if_empty(&mut self) {
        if self.row >= self.len() {
            self.row = 0;
        }
    }
}

/// Best-effort system clipboard; falls back to an OSC 52 escape.
pub fn copy_to_clipboard(text: &str) -> bool {
    for (bin, args) in [
        ("pbcopy", &[][..]),
        ("wl-copy", &[][..]),
        ("xclip", &["-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--input"][..]),
    ] {
        if let Ok(mut child) = Command::new(bin).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(text.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
    false
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for ch in data.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}
