//! The on-disk archive: `issues/<N>/<M>.txt`, each file starting with a tiny
//! `---` front-matter block carrying `title:` and `author:`.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use regex::Regex;

#[derive(Clone, Debug)]
pub struct Article {
    pub issue: u32,
    pub num: u32,
    pub title: String,
    pub author: String,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub num: u32,
    pub articles: Vec<Article>,
}

#[derive(Clone, Debug)]
pub struct Library {
    pub issues: Vec<Issue>,
}

impl Library {
    pub fn scan(root: &Path) -> io::Result<Library> {
        let mut issues = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let Some(num) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            if !entry.path().is_dir() {
                continue;
            }
            let mut articles = Vec::new();
            for f in fs::read_dir(entry.path())? {
                let f = f?;
                let name = f.file_name();
                let name = name.to_string_lossy();
                let Some(stem) = name.strip_suffix(".txt") else { continue };
                let Ok(n) = stem.parse::<u32>() else { continue };
                let (title, author) = read_meta(&f.path());
                let size = f.metadata().map(|m| m.len()).unwrap_or(0);
                articles.push(Article {
                    issue: num,
                    num: n,
                    title: if title.is_empty() { format!("Phile {n}") } else { title },
                    author,
                    path: f.path(),
                    size,
                });
            }
            articles.sort_by_key(|a| a.num);
            if articles.is_empty() {
                continue;
            }
            issues.push(Issue { num, articles });
        }
        issues.sort_by_key(|i| i.num);
        Ok(Library { issues })
    }

    pub fn issue_index(&self, num: u32) -> Option<usize> {
        self.issues.binary_search_by_key(&num, |i| i.num).ok()
    }

    pub fn issue(&self, num: u32) -> Option<&Issue> {
        self.issue_index(num).map(|i| &self.issues[i])
    }

    pub fn article(&self, issue: u32, num: u32) -> Option<&Article> {
        self.issue(issue)?.articles.iter().find(|a| a.num == num)
    }

    pub fn total_articles(&self) -> usize {
        self.issues.iter().map(|i| i.articles.len()).sum()
    }

    pub fn all_articles(&self) -> impl Iterator<Item = &Article> {
        self.issues.iter().flat_map(|i| i.articles.iter())
    }

    /// Author -> list of (issue, article) pairs, sorted case-insensitively.
    pub fn authors(&self) -> Vec<(String, Vec<(u32, u32)>)> {
        let mut map: BTreeMap<String, (String, Vec<(u32, u32)>)> = BTreeMap::new();
        for a in self.all_articles() {
            for name in split_authors(&a.author) {
                let e = map.entry(name.to_lowercase()).or_insert_with(|| (name.clone(), Vec::new()));
                e.1.push((a.issue, a.num));
            }
        }
        map.into_values().collect()
    }

    pub fn articles_by(&self, author: &str) -> Vec<&Article> {
        let key = author.to_lowercase();
        self.all_articles()
            .filter(|a| split_authors(&a.author).iter().any(|n| n.to_lowercase() == key))
            .collect()
    }

    /// Find a "special column" philes (Loopback, Linenoise ...) in an issue.
    pub fn special(&self, issue: u32, kind: Special) -> Option<&Article> {
        let iss = self.issue(issue)?;
        if kind == Special::Intro {
            if let Some(a) = iss.articles.iter().find(|a| a.title.to_lowercase().contains("intro")) {
                return Some(a);
            }
            return iss.articles.first();
        }
        iss.articles.iter().find(|a| kind.matches(&a.title))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    Intro,
    Loopback,
    Linenoise,
    Prophile,
    WorldNews,
    Index,
}

impl Special {
    pub fn name(self) -> &'static str {
        match self {
            Special::Intro => "Introduction",
            Special::Loopback => "Loopback",
            Special::Linenoise => "Linenoise",
            Special::Prophile => "Prophile",
            Special::WorldNews => "World News",
            Special::Index => "Index",
        }
    }

    fn matches(self, title: &str) -> bool {
        let t = title.to_lowercase();
        let squashed: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
        match self {
            Special::Intro => t.contains("intro"),
            Special::Loopback => t.contains("loopback"),
            Special::Linenoise => squashed.contains("linenoise"),
            Special::Prophile => t.contains("prophile") || t.contains("pro-phile") || t.contains("profile of"),
            Special::WorldNews => t.contains("world news") || t.starts_with("pwn"),
            Special::Index => t.contains("index"),
        }
    }
}

pub fn split_authors(s: &str) -> Vec<String> {
    static SEP: OnceLock<Regex> = OnceLock::new();
    let re = SEP.get_or_init(|| Regex::new(r"(?i)\s*(?:,|&|\+|\band\b|;)\s*").unwrap());
    re.split(s)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn read_head(path: &Path, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let got = f.read(&mut buf).unwrap_or(0);
    buf.truncate(got);
    buf
}

fn read_meta(path: &Path) -> (String, String) {
    let head = read_head(path, 1024);
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (String::new(), String::new());
    }
    let (mut title, mut author) = (String::new(), String::new());
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        if let Some(v) = l.strip_prefix("title:") {
            title = unquote(v.trim());
        } else if let Some(v) = l.strip_prefix("author:") {
            author = unquote(v.trim());
        }
    }
    (title, author)
}

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Resolve the archive directory from CLI arg / env / cwd / compile-time path.
pub fn find_root(arg: Option<&str>) -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(a) = arg {
        cands.push(a.into());
    }
    if let Ok(e) = std::env::var("PHRACK_ISSUES") {
        cands.push(e.into());
    }
    cands.push("issues".into());
    cands.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("issues"));
    cands.push(".".into());
    cands.into_iter().find(|p| {
        p.is_dir()
            && fs::read_dir(p)
                .map(|d| d.flatten().any(|e| e.path().is_dir() && e.file_name().to_string_lossy().parse::<u32>().is_ok()))
                .unwrap_or(false)
    })
}
