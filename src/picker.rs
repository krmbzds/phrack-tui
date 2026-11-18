//! A generic filterable list overlay (finder, authors, outline, marks, grep).

use std::sync::mpsc::{Receiver, TryRecvError};

use crate::input::K;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Article { issue: u32, art: u32, line: Option<usize> },
    Author(String),
    /// Jump inside the article being read.
    Line(usize),
}

#[derive(Clone, Debug)]
pub struct PickItem {
    pub label: String,
    pub detail: String,
    pub target: Target,
}

pub enum PickAction {
    None,
    Close,
    Select(Target),
}

pub struct Picker {
    pub title: String,
    pub items: Vec<PickItem>,
    pub view: Vec<usize>,
    pub filter: String,
    pub filtering: bool,
    pub sel: usize,
    pub rx: Option<Receiver<Vec<PickItem>>>,
    pub searching: bool,
    /// Pattern to seed the reader's search with when a hit is opened.
    pub pattern: Option<String>,
    pub page: usize,
    pub pending_g: bool,
}

impl Picker {
    pub fn new(title: impl Into<String>, items: Vec<PickItem>, filtering: bool) -> Picker {
        let mut p = Picker {
            title: title.into(),
            view: Vec::new(),
            items,
            filter: String::new(),
            filtering,
            sel: 0,
            rx: None,
            searching: false,
            pattern: None,
            page: 10,
            pending_g: false,
        };
        p.refilter();
        p
    }

    pub fn with_filter(mut self, f: &str) -> Picker {
        self.filter = f.to_string();
        self.refilter();
        self
    }

    pub fn refilter(&mut self) {
        let terms: Vec<String> = self.filter.split_whitespace().map(|t| t.to_lowercase()).collect();
        self.view = (0..self.items.len())
            .filter(|&i| {
                if terms.is_empty() {
                    return true;
                }
                let it = &self.items[i];
                let hay = format!("{} {}", it.label, it.detail).to_lowercase();
                terms.iter().all(|t| hay.contains(t.as_str()))
            })
            .collect();
        self.sel = self.sel.min(self.view.len().saturating_sub(1));
    }

    pub fn selected(&self) -> Option<&PickItem> {
        self.view.get(self.sel).map(|&i| &self.items[i])
    }

    /// Pull streamed results (grep) into the list. Returns true if changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(mut chunk) => {
                    self.items.append(&mut chunk);
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.searching = false;
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            let keep = self.sel;
            self.refilter();
            self.sel = keep.min(self.view.len().saturating_sub(1));
        }
        changed
    }

    fn mv(&mut self, delta: isize) {
        let n = self.view.len() as isize;
        if n == 0 {
            return;
        }
        self.sel = (self.sel as isize + delta).clamp(0, n - 1) as usize;
    }

    pub fn key(&mut self, k: K) -> PickAction {
        if self.filtering {
            return match k {
                K::Esc => {
                    self.filtering = false;
                    PickAction::None
                }
                K::Enter => match self.selected() {
                    Some(it) => PickAction::Select(it.target.clone()),
                    None => PickAction::None,
                },
                K::Backspace => {
                    if self.filter.pop().is_none() {
                        self.filtering = false;
                    }
                    self.refilter();
                    PickAction::None
                }
                K::Ctrl('u') => {
                    self.filter.clear();
                    self.refilter();
                    PickAction::None
                }
                K::Ctrl('w') => {
                    let t = self.filter.trim_end().rfind(' ').map_or(0, |i| i + 1);
                    self.filter.truncate(t);
                    self.refilter();
                    PickAction::None
                }
                K::Down | K::Ctrl('n') | K::Ctrl('j') => {
                    self.mv(1);
                    PickAction::None
                }
                K::Up | K::Ctrl('p') | K::Ctrl('k') => {
                    self.mv(-1);
                    PickAction::None
                }
                K::PageDown => {
                    self.mv(self.page as isize);
                    PickAction::None
                }
                K::PageUp => {
                    self.mv(-(self.page as isize));
                    PickAction::None
                }
                K::Char(c) => {
                    self.filter.push(c);
                    self.refilter();
                    self.sel = 0;
                    PickAction::None
                }
                _ => PickAction::None,
            };
        }
        if std::mem::take(&mut self.pending_g) {
            if k == K::Char('g') {
                self.sel = 0;
            }
            return PickAction::None;
        }
        match k {
            K::Esc | K::Char('q') => PickAction::Close,
            K::Char('j') | K::Down | K::Ctrl('n') => {
                self.mv(1);
                PickAction::None
            }
            K::Char('k') | K::Up | K::Ctrl('p') => {
                self.mv(-1);
                PickAction::None
            }
            K::Ctrl('d') | K::PageDown => {
                self.mv((self.page / 2).max(1) as isize);
                PickAction::None
            }
            K::Ctrl('u') | K::PageUp => {
                self.mv(-((self.page / 2).max(1) as isize));
                PickAction::None
            }
            K::Char('g') => {
                self.pending_g = true;
                PickAction::None
            }
            K::Char('G') | K::End => {
                self.sel = self.view.len().saturating_sub(1);
                PickAction::None
            }
            K::Home => {
                self.sel = 0;
                PickAction::None
            }
            K::Char('/') | K::Char('i') | K::Char('a') => {
                self.filtering = true;
                PickAction::None
            }
            K::Enter | K::Char('l') | K::Right => match self.selected() {
                Some(it) => PickAction::Select(it.target.clone()),
                None => PickAction::None,
            },
            _ => PickAction::None,
        }
    }
}
