//! Archive downloader (a port of `phrack-issue-manager`, minus its exporters).
//!
//! The archive is a plain HTML site: `/issues/` links to `/issues/N/`, which
//! links to `/issues/N/M.txt`. One lister thread walks the issues in order and
//! queues the philes it finds; a small pool of workers fetches them. Progress
//! comes back as [`Event`]s over a channel, so the UI (or the headless CLI)
//! can show philes appearing as they land. Files are written atomically and
//! already-present philes are skipped unless `refresh` is set.

use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use regex::Regex;

#[derive(Clone, Debug)]
pub struct Options {
    pub archive_url: String,
    pub root: PathBuf,
    pub concurrency: usize,
    /// Re-download philes that already exist.
    pub refresh: bool,
    /// Only this issue.
    pub only: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// The archive lists this many issues.
    Indexed(usize),
    /// An issue's page was read and holds this many philes.
    Listed { issue: u32, files: usize },
    /// A phile is on disk (`fresh` = downloaded now, otherwise it was already there).
    Saved { issue: u32, art: u32, path: PathBuf, bytes: u64, fresh: bool },
    Failed { msg: String },
    /// Nothing can proceed (no index, nothing to do).
    Fatal(String),
}

pub struct Handle {
    pub rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct Job {
    issue: u32,
    art: u32,
    url: String,
    dest: PathBuf,
}

// ---- parsing ------------------------------------------------------------------

fn hrefs(html: &str) -> Vec<String> {
    let re = Regex::new(r#"(?i)href\s*=\s*["']([^"']+)["']"#).unwrap();
    re.captures_iter(html).map(|c| c[1].to_string()).collect()
}

/// Issue numbers linked from the `/issues/` index, ascending and de-duplicated.
pub fn parse_issue_index(html: &str) -> Vec<u32> {
    let re = Regex::new(r"^(?:https?://[^/]+)?/issues/(\d+)/?$").unwrap();
    let set: BTreeSet<u32> = hrefs(html).iter().filter_map(|h| re.captures(h)).filter_map(|c| c[1].parse().ok()).collect();
    set.into_iter().collect()
}

/// Phile numbers (`N/M.txt`) linked from an issue page, ascending.
pub fn parse_issue_files(html: &str, issue: u32) -> Vec<u32> {
    let re = Regex::new(&format!(r"^(?:https?://[^/]+)?/issues/{issue}/(\d+)\.txt$")).unwrap();
    let set: BTreeSet<u32> = hrefs(html).iter().filter_map(|h| re.captures(h)).filter_map(|c| c[1].parse().ok()).collect();
    set.into_iter().collect()
}

// ---- network ------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(45)))
        .user_agent(concat!("phrack-tui/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn get_once(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| e.to_string())
}

/// GET with a few retries and a little backoff.
fn get(agent: &ureq::Agent, url: &str, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 1..=3u64 {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        match get_once(agent, url) {
            Ok(b) => return Ok(b),
            Err(e) => last = e,
        }
        if attempt < 3 {
            thread::sleep(Duration::from_millis(350 * attempt));
        }
    }
    Err(last)
}

fn save(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("txt.part");
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all().ok();
    fs::rename(tmp, path)
}

/// Start downloading in the background.
pub fn start(opts: Options) -> Handle {
    let (tx, rx) = mpsc::channel::<Event>();
    let cancel = Arc::new(AtomicBool::new(false));
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let agent = agent();

    for _ in 0..opts.concurrency.clamp(1, 16) {
        let (tx, job_rx, agent, cancel) = (tx.clone(), job_rx.clone(), agent.clone(), cancel.clone());
        thread::spawn(move || loop {
            let job = match job_rx.lock().map(|g| g.recv()) {
                Ok(Ok(j)) => j,
                _ => break,
            };
            if cancel.load(Ordering::Relaxed) {
                continue; // drain the queue quickly
            }
            let ev = match get(&agent, &job.url, &cancel).and_then(|b| save(&job.dest, &b).map(|_| b.len() as u64).map_err(|e| e.to_string())) {
                Ok(bytes) => Event::Saved { issue: job.issue, art: job.art, path: job.dest, bytes, fresh: true },
                Err(e) => Event::Failed { msg: format!("{}/{}: {e}", job.issue, job.art) },
            };
            if tx.send(ev).is_err() {
                break;
            }
        });
    }

    let lister_cancel = cancel.clone();
    let lister_tx = tx.clone();
    thread::spawn(move || {
        let tx = lister_tx;
        let base = opts.archive_url.trim_end_matches('/').to_string();
        let issues = match opts.only {
            Some(n) => vec![n],
            None => match get(&agent, &format!("{base}/issues/"), &lister_cancel) {
                Ok(b) => parse_issue_index(&String::from_utf8_lossy(&b)),
                Err(e) => {
                    let _ = tx.send(Event::Fatal(format!("cannot reach {base}: {e}")));
                    return;
                }
            },
        };
        if issues.is_empty() {
            let _ = tx.send(Event::Fatal(format!("no issues found at {base}/issues/")));
            return;
        }
        let _ = tx.send(Event::Indexed(issues.len()));
        for issue in issues {
            if lister_cancel.load(Ordering::Relaxed) {
                break;
            }
            let page = match get(&agent, &format!("{base}/issues/{issue}/"), &lister_cancel) {
                Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                Err(e) => {
                    let _ = tx.send(Event::Failed { msg: format!("issue {issue}: {e}") });
                    continue;
                }
            };
            let files = parse_issue_files(&page, issue);
            let _ = tx.send(Event::Listed { issue, files: files.len() });
            for art in files {
                let dest = opts.root.join(issue.to_string()).join(format!("{art}.txt"));
                let have = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                if have > 0 && !opts.refresh {
                    let _ = tx.send(Event::Saved { issue, art, path: dest, bytes: have, fresh: false });
                } else {
                    let url = format!("{base}/issues/{issue}/{art}.txt");
                    if job_tx.send(Job { issue, art, url, dest }).is_err() {
                        return;
                    }
                }
            }
        }
        // dropping job_tx and tx lets the workers drain and exit
    });
    drop(tx);
    Handle { rx, cancel }
}

/// Headless run for `phrack-tui --download [ISSUE]`; returns the exit code.
pub fn run_cli(opts: Options) -> i32 {
    let h = start(opts.clone());
    eprintln!("downloading from {} into {}", opts.archive_url, opts.root.display());
    let (mut total, mut fresh, mut failed, mut issues, mut bytes) = (0usize, 0usize, 0usize, 0usize, 0u64);
    let mut fatal = false;
    while let Ok(ev) = h.rx.recv() {
        match ev {
            Event::Indexed(n) => eprintln!("archive lists {n} issue(s)"),
            Event::Listed { issue, files } => {
                issues += 1;
                eprintln!("issue {issue}: {files} philes");
            }
            Event::Saved { fresh: f, bytes: b, .. } => {
                total += 1;
                if f {
                    fresh += 1;
                    bytes += b;
                }
            }
            Event::Failed { msg } => {
                failed += 1;
                eprintln!("  failed: {msg}");
            }
            Event::Fatal(m) => {
                fatal = true;
                eprintln!("error: {m}");
            }
        }
    }
    eprintln!("{issues} issue(s), {total} philes on disk ({fresh} new, {:.1} MB), {failed} failed", bytes as f64 / 1_048_576.0);
    if fatal || failed > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
    };

    #[test]
    fn parses_links() {
        let idx = r#"<a href="/">home</a><a href="/issues/">x</a><a href="/issues/10/">10</a>
            <a href='/issues/2/'>2</a><a href="https://archives.phrack.org/issues/2/">dup</a><a href="/issues/2/phrack-2.pdf">pdf</a>"#;
        assert_eq!(parse_issue_index(idx), vec![2, 10]);
        let page = r##"<a href="/issues/25/1.txt">1</a><a href="/issues/25/10.txt">10</a><a href="/issues/25/2.txt">2</a>
            <a href="/issues/25/phrack-25.pdf">pdf</a><a href="/issues/26/1.txt">other</a><a href="#file">x</a>"##;
        assert_eq!(parse_issue_files(page, 25), vec![1, 2, 10]);
    }

    fn serve(routes: HashMap<String, Vec<u8>>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || {
            for s in l.incoming().flatten() {
                let routes = routes.clone();
                thread::spawn(move || handle(s, &routes));
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn handle(mut s: TcpStream, routes: &HashMap<String, Vec<u8>>) {
        let mut buf = [0u8; 2048];
        let mut req = Vec::new();
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => req.extend_from_slice(&buf[..n]),
            }
        }
        let line = String::from_utf8_lossy(&req);
        let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
        let (status, body) = match routes.get(&path) {
            Some(b) => ("200 OK", b.clone()),
            None => ("404 Not Found", b"nope".to_vec()),
        };
        let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let _ = s.write_all(&body);
    }

    fn drain(h: &Handle) -> Vec<Event> {
        let mut v = Vec::new();
        while let Ok(e) = h.rx.recv_timeout(Duration::from_secs(20)) {
            v.push(e);
        }
        v
    }

    #[test]
    fn downloads_resumes_and_refreshes() {
        let mut r: HashMap<String, Vec<u8>> = HashMap::new();
        r.insert("/issues/".into(), br#"<a href="/issues/1/">1</a><a href="/issues/2/">2</a>"#.to_vec());
        r.insert("/issues/1/".into(), br#"<a href="/issues/1/1.txt">a</a><a href="/issues/1/2.txt">b</a>"#.to_vec());
        r.insert("/issues/2/".into(), br#"<a href="/issues/2/1.txt">a</a><a href="/issues/2/9.txt">missing</a>"#.to_vec());
        r.insert("/issues/1/1.txt".into(), b"---\ntitle: One\nauthor: A\n---\nhello".to_vec());
        r.insert("/issues/1/2.txt".into(), vec![b'x', 0xC9, 0xCD, 0xBB]); // raw non-UTF-8 survives
        r.insert("/issues/2/1.txt".into(), b"two".to_vec());
        let base = serve(r);
        let root = std::env::temp_dir().join(format!("phrack-dl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let opts = Options { archive_url: base.clone(), root: root.clone(), concurrency: 3, refresh: false, only: None };

        let ev = drain(&start(opts.clone()));
        assert!(matches!(ev[0], Event::Indexed(2)));
        let saved: Vec<_> = ev.iter().filter(|e| matches!(e, Event::Saved { fresh: true, .. })).collect();
        assert_eq!(saved.len(), 3);
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Failed { .. })).count(), 1); // 2/9 is a 404
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Listed { .. })).count(), 2);
        assert_eq!(fs::read(root.join("1/2.txt")).unwrap(), vec![b'x', 0xC9, 0xCD, 0xBB]);
        assert!(!root.join("1/1.txt.part").exists());
        assert_eq!(fs::read(root.join("2/1.txt")).unwrap(), b"two");

        // second run: everything present is skipped, only the 404 is retried
        let ev = drain(&start(opts.clone()));
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Saved { fresh: false, .. })).count(), 3);
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Saved { fresh: true, .. })).count(), 0);

        // refresh re-downloads; `only` limits to one issue
        let ev = drain(&start(Options { refresh: true, only: Some(1), ..opts.clone() }));
        assert!(matches!(ev[0], Event::Indexed(1)));
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Saved { fresh: true, .. })).count(), 2);

        // an unreachable archive is one clean fatal event
        let ev = drain(&start(Options { archive_url: "http://127.0.0.1:1".into(), ..opts }));
        assert!(matches!(ev.as_slice(), [Event::Fatal(_)]), "{ev:?}");
        let _ = fs::remove_dir_all(root);
    }
}
