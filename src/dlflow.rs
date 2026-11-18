//! The "Download issues? Y/n" prompt, the download overlay's state, and how
//! download events feed the live library. (Drawing lives in `ui.rs`.)

use std::{
    collections::{BTreeMap, VecDeque},
    sync::mpsc::TryRecvError,
    time::Instant,
};

use crate::{
    app::{App, Overlay, Pane},
    config::PromptMode,
    download::{self, Event},
    input::K,
};

/// How long the finished overlay lingers before handing over to the main screen.
const LINGER_MS: u64 = 1100;

pub struct LogLine {
    pub label: String,
    pub bytes: u64,
    pub fresh: bool,
}

pub struct DlState {
    pub handle: Option<download::Handle>,
    started: Instant,
    /// Fixed animation clock (ms) for offline rendering; `None` = wall clock.
    pub clock: Option<u64>,
    pub total_issues: Option<usize>,
    /// issue -> (philes listed, philes done)
    pub per_issue: BTreeMap<u32, (usize, usize)>,
    pub files: usize,
    pub fresh: usize,
    pub bytes: u64,
    pub failed: Vec<String>,
    pub log: VecDeque<LogLine>,
    pub finished_at: Option<u64>,
    pub cancelled: bool,
    pub fatal: Option<String>,
    /// Demo recordings compress time; this scales the displayed transfer rate back
    /// to what the same bytes would take on a real connection. 1.0 normally.
    pub rate_scale: f64,
}

impl DlState {
    pub fn new(handle: Option<download::Handle>) -> DlState {
        DlState {
            handle,
            started: Instant::now(),
            clock: None,
            total_issues: None,
            per_issue: BTreeMap::new(),
            files: 0,
            fresh: 0,
            bytes: 0,
            failed: Vec::new(),
            log: VecDeque::new(),
            finished_at: None,
            cancelled: false,
            fatal: None,
            rate_scale: 1.0,
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.unwrap_or_else(|| self.started.elapsed().as_millis() as u64)
    }

    /// 0.0..=1.0 across all issues; an issue counts as it fills up.
    pub fn progress(&self) -> f64 {
        if self.finished_at.is_some() && self.fatal.is_none() {
            return 1.0;
        }
        let total = self.total_issues.unwrap_or(0).max(self.per_issue.len());
        if total == 0 {
            return 0.0;
        }
        let sum: f64 = self.per_issue.values().map(|&(l, d)| if l == 0 { 1.0 } else { (d as f64 / l as f64).min(1.0) }).sum();
        (sum / total as f64).min(1.0)
    }

    pub fn kb_per_s(&self) -> f64 {
        let secs = (self.now_ms() as f64 / 1000.0 * self.rate_scale).max(0.5);
        self.bytes as f64 / 1024.0 / secs
    }
}

impl App {
    /// Called once the UI is ready (immediately, or after the boot sequence).
    pub fn start(&mut self) {
        if self.boot.is_none() {
            self.maybe_prompt();
        }
    }

    pub fn finish_boot(&mut self) {
        self.boot = None;
        self.maybe_prompt();
    }

    /// Offer the download if the config says so (by default: when nothing is on disk).
    pub fn maybe_prompt(&mut self) {
        if self.dl.is_some() || !matches!(self.overlay, Overlay::None) {
            return;
        }
        let ask = match self.cfg.download_prompt {
            PromptMode::Always => true,
            PromptMode::Missing => self.lib.issues.is_empty(),
            PromptMode::Never => false,
        };
        if ask {
            self.overlay = Overlay::DlPrompt;
        }
    }

    pub fn dlprompt_key(&mut self, k: K) {
        match k {
            K::Enter | K::Char('y') | K::Char('Y') => self.start_download(false, None),
            K::Char('n') | K::Char('N') | K::Esc | K::Char('q') => {
                self.overlay = Overlay::None;
                if self.lib.issues.is_empty() {
                    self.say("no issues on disk: press Enter or type :download to fetch them");
                }
            }
            _ => {}
        }
    }

    pub fn download_key(&mut self, k: K) {
        let Some(dl) = self.dl.as_mut() else {
            self.overlay = Overlay::None;
            return;
        };
        if dl.finished_at.is_some() {
            return self.end_download();
        }
        if k == K::Esc && !dl.cancelled {
            dl.cancelled = true;
            if let Some(h) = &dl.handle {
                h.cancel();
            }
        }
    }

    pub fn start_download(&mut self, refresh: bool, only: Option<u32>) {
        if self.dl.is_some() {
            return self.err("a download is already running");
        }
        if self.reader.is_some() {
            self.close_reader();
        }
        self.cmd = None;
        if self.dl_offline {
            return self.begin_download_ui();
        }
        let handle = download::start(download::Options {
            archive_url: self.cfg.archive_url.clone(),
            root: self.root.clone(),
            concurrency: self.cfg.concurrency,
            refresh,
            only,
        });
        self.dl = Some(DlState::new(Some(handle)));
        self.overlay = Overlay::Download;
    }

    /// Open the download overlay without a network (the demo recorder drives it).
    pub fn begin_download_ui(&mut self) {
        self.dl = Some(DlState::new(None));
        self.overlay = Overlay::Download;
    }

    pub fn apply_dl_event(&mut self, ev: Event) {
        match ev {
            Event::Indexed(n) => {
                if let Some(dl) = self.dl.as_mut() {
                    dl.total_issues = Some(n);
                }
            }
            Event::Listed { issue, files } => {
                if let Some(dl) = self.dl.as_mut() {
                    dl.per_issue.entry(issue).or_insert((0, 0)).0 = files;
                }
            }
            Event::Saved { issue, art, path, bytes, fresh } => {
                self.lib.add_article(issue, art, path);
                let label = self.lib.article(issue, art).map(|a| format!("{issue}.{art}  {}", a.title)).unwrap_or_else(|| format!("{issue}.{art}"));
                if let Some(dl) = self.dl.as_mut() {
                    dl.per_issue.entry(issue).or_insert((0, 0)).1 += 1;
                    dl.files += 1;
                    if fresh {
                        dl.fresh += 1;
                        dl.bytes += bytes;
                    }
                    dl.log.push_back(LogLine { label, bytes, fresh });
                    while dl.log.len() > 32 {
                        dl.log.pop_front();
                    }
                }
                // let the screen behind the overlay follow the newest phile
                self.browser_select(issue, art, Pane::Articles);
            }
            Event::Failed { msg } => {
                if let Some(dl) = self.dl.as_mut() {
                    dl.failed.push(msg);
                }
            }
            Event::Fatal(msg) => {
                if let Some(dl) = self.dl.as_mut() {
                    dl.fatal = Some(msg);
                }
            }
        }
    }

    pub fn finish_download(&mut self) {
        if let Some(dl) = self.dl.as_mut() {
            if dl.finished_at.is_none() {
                dl.finished_at = Some(dl.now_ms());
            }
        }
    }

    pub fn end_download(&mut self) {
        let Some(dl) = self.dl.take() else { return };
        self.overlay = Overlay::None;
        self.redraw_all = true;
        if let Some(last) = self.lib.issues.last().map(|i| i.num) {
            self.browser_select(last, 0, Pane::Issues);
        }
        let secs = dl.now_ms() as f64 / 1000.0;
        if let Some(f) = &dl.fatal {
            self.err(format!("download failed: {f}"));
        } else if dl.cancelled {
            self.say(format!("download cancelled: {} philes on disk so far", self.lib.total_articles()));
        } else if !dl.failed.is_empty() {
            self.err(format!("{} philes failed ({}); :download retries them", dl.failed.len(), dl.failed[0]));
        } else {
            self.say(format!("{} new philes, {} in the archive ({secs:.0}s)", dl.fresh, self.lib.total_articles()));
        }
    }

    /// Pump the downloader's events; returns true when the screen should redraw.
    pub fn dl_tick(&mut self) -> bool {
        let Some(dl) = self.dl.as_mut() else { return false };
        let mut evs = Vec::new();
        let mut gone = false;
        if let Some(h) = &dl.handle {
            for _ in 0..600 {
                match h.rx.try_recv() {
                    Ok(e) => evs.push(e),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        gone = true;
                        break;
                    }
                }
            }
        }
        for e in evs {
            self.apply_dl_event(e);
        }
        if gone {
            self.finish_download();
        }
        if let Some(dl) = &self.dl {
            // errors wait for a key press; a clean run hands over by itself
            if let (Some(at), None) = (dl.finished_at, &dl.fatal) {
                if dl.now_ms().saturating_sub(at) >= LINGER_MS {
                    self.end_download();
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        collection::Collection,
        config::{Config, PromptMode},
        data::Library,
        state::PState,
    };

    fn app(cfg: Config) -> App {
        let root = std::env::temp_dir().join(format!("phrack-flow-{}", std::process::id()));
        let mut a = App::new(Library { issues: vec![] }, PState::default(), Collection::default(), cfg, root, (100, 30));
        a.dl_offline = true; // never touch the network from a test
        a
    }

    fn type_keys(a: &mut App, s: &str) {
        for c in s.chars() {
            a.on_key(if c == '\n' { K::Enter } else { K::Char(c) });
        }
    }

    #[test]
    fn prompt_follows_config() {
        for (mode, empty_asks, full_asks) in [(PromptMode::Missing, true, false), (PromptMode::Always, true, true), (PromptMode::Never, false, false)] {
            let cfg = Config { download_prompt: mode, ..Config::default() };
            let mut a = app(cfg.clone());
            a.start();
            assert_eq!(matches!(a.overlay, Overlay::DlPrompt), empty_asks, "{mode:?} empty");
            // a library that already has a phile
            let mut b = app(cfg);
            let f = std::env::temp_dir().join(format!("phrack-flow-file-{}.txt", std::process::id()));
            std::fs::write(&f, "---\ntitle: T\nauthor: A\n---\nbody").unwrap();
            b.lib.add_article(1, 1, f.clone());
            b.start();
            assert_eq!(matches!(b.overlay, Overlay::DlPrompt), full_asks, "{mode:?} full");
            let _ = std::fs::remove_file(f);
        }
    }

    #[test]
    fn prompt_keys() {
        let mut a = app(Config::default());
        a.start();
        a.on_key(K::Char('n'));
        assert!(matches!(a.overlay, Overlay::None) && a.dl.is_none());
        for k in [K::Enter, K::Char('y'), K::Char('Y')] {
            let mut a = app(Config::default());
            a.start();
            a.on_key(k);
            assert!(matches!(a.overlay, Overlay::Download) && a.dl.is_some(), "{k:?}");
        }
        // anything else leaves the prompt up
        let mut a = app(Config::default());
        a.start();
        a.on_key(K::Char('x'));
        assert!(matches!(a.overlay, Overlay::DlPrompt));
    }

    #[test]
    fn events_populate_the_library_live() {
        let dir = std::env::temp_dir().join(format!("phrack-flow-ev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = app(Config::default());
        a.start();
        a.on_key(K::Enter);
        a.apply_dl_event(Event::Indexed(2));
        for (issue, art, title) in [(1u32, 1u32, "Intro"), (1, 2, "Second"), (2, 1, "Other")] {
            let p = dir.join(format!("{issue}-{art}.txt"));
            std::fs::write(&p, format!("---\ntitle: {title}\nauthor: Me\n---\nhello")).unwrap();
            a.apply_dl_event(Event::Listed { issue, files: if issue == 1 { 2 } else { 1 } });
            a.apply_dl_event(Event::Saved { issue, art, path: p, bytes: 40, fresh: true });
        }
        assert_eq!(a.lib.issues.len(), 2);
        assert_eq!(a.lib.article(1, 2).unwrap().title, "Second");
        let dl = a.dl.as_ref().unwrap();
        assert_eq!((dl.files, dl.fresh, dl.bytes), (3, 3, 120));
        assert!((dl.progress() - 1.0).abs() < 1e-9);
        assert_eq!(dl.log.back().unwrap().label, "2.1  Other");
        // the screen behind the overlay follows the newest phile
        assert_eq!((a.lib.issues[a.browser.issue].num, a.browser.art), (2, 0));
        // finishing lingers, then hands over to the main screen
        a.finish_download();
        a.dl.as_mut().unwrap().clock = Some(5_000);
        a.dl.as_mut().unwrap().finished_at = Some(1_000);
        a.dl_tick();
        assert!(a.dl.is_none() && matches!(a.overlay, Overlay::None));
        assert_eq!(a.lib.issues[a.browser.issue].num, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fatal_error_waits_for_a_key() {
        let mut a = app(Config::default());
        a.start();
        a.on_key(K::Enter);
        a.apply_dl_event(Event::Fatal("cannot reach".into()));
        a.finish_download();
        a.dl.as_mut().unwrap().clock = Some(60_000);
        a.dl_tick();
        assert!(a.dl.is_some(), "an error must not vanish on its own");
        a.on_key(K::Char('x'));
        assert!(a.dl.is_none() && a.msg.as_ref().is_some_and(|m| m.err && m.text.contains("cannot reach")));
    }

    #[test]
    fn empty_library_survives_every_key() {
        let mut a = app(Config { download_prompt: PromptMode::Never, ..Config::default() });
        a.start();
        // movement, counts, g-prefixes, finders, commands: none may panic on an empty archive
        for s in ["jkhlGgg", "5j", "gr", "gu", "gA", "\u{1b}", "gm", "]a", "[i", "x", "/", "\u{1b}", "oh", "]]", "ZZZ"] {
            let mut a2 = app(Config { download_prompt: PromptMode::Never, ..Config::default() });
            for c in s.chars() {
                a2.on_key(if c == '\u{1b}' { K::Esc } else { K::Char(c) });
            }
        }
        for cmd in ["random", "unread", "issue 3", "open 1/2", "find x", "grep x", "author x", "marks", "theme crt", "set nu", "25", "1/1", "bogus"] {
            a.exec(cmd);
        }
        a.on_key(K::Enter); // asks again
        assert!(matches!(a.overlay, Overlay::DlPrompt));
    }
}
