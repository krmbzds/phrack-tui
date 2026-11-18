//! Tiny line-based persistence: read-tracking, marks, last position, theme.
//! Lives in `$XDG_STATE_HOME/phrack-tui/state` (or `~/.local/state/...`).

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Default, Clone, Debug)]
pub struct PState {
    pub path: Option<PathBuf>,
    pub read: HashSet<(u32, u32)>,
    pub last: Option<(u32, u32, usize)>,
    pub global_marks: HashMap<char, (u32, u32, usize)>,
    pub local_marks: HashMap<(u32, u32, char), usize>,
    pub pos: HashMap<(u32, u32), usize>,
    pub theme: Option<String>,
}

pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PHRACK_TUI_STATE") {
        return Some(p.into());
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("phrack-tui").join("state"))
}

impl PState {
    pub fn load(path: Option<PathBuf>) -> PState {
        let mut st = PState { path: path.clone(), ..Default::default() };
        let Some(p) = path else { return st };
        let Ok(text) = fs::read_to_string(p) else { return st };
        for l in text.lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            let num = |i: usize| f.get(i).and_then(|s| s.parse::<u32>().ok());
            let ch = |i: usize| f.get(i).and_then(|s| s.chars().next());
            match f.first().copied() {
                Some("read") => {
                    if let (Some(a), Some(b)) = (num(1), num(2)) {
                        st.read.insert((a, b));
                    }
                }
                Some("last") => {
                    if let (Some(a), Some(b), Some(c)) = (num(1), num(2), num(3)) {
                        st.last = Some((a, b, c as usize));
                    }
                }
                Some("gmark") => {
                    if let (Some(c), Some(a), Some(b), Some(r)) = (ch(1), num(2), num(3), num(4)) {
                        st.global_marks.insert(c, (a, b, r as usize));
                    }
                }
                Some("lmark") => {
                    if let (Some(a), Some(b), Some(c), Some(r)) = (num(1), num(2), ch(3), num(4)) {
                        st.local_marks.insert((a, b, c), r as usize);
                    }
                }
                Some("pos") => {
                    if let (Some(a), Some(b), Some(r)) = (num(1), num(2), num(3)) {
                        st.pos.insert((a, b), r as usize);
                    }
                }
                Some("theme") => st.theme = f.get(1).map(|s| s.to_string()),
                _ => {}
            }
        }
        st
    }

    pub fn save(&self) {
        let Some(p) = &self.path else { return };
        let mut s = String::new();
        if let Some(t) = &self.theme {
            s.push_str(&format!("theme {t}\n"));
        }
        if let Some((a, b, c)) = self.last {
            s.push_str(&format!("last {a} {b} {c}\n"));
        }
        let mut read: Vec<_> = self.read.iter().collect();
        read.sort();
        for (a, b) in read {
            s.push_str(&format!("read {a} {b}\n"));
        }
        let mut gm: Vec<_> = self.global_marks.iter().collect();
        gm.sort();
        for (c, (a, b, r)) in gm {
            s.push_str(&format!("gmark {c} {a} {b} {r}\n"));
        }
        let mut lm: Vec<_> = self.local_marks.iter().collect();
        lm.sort();
        for ((a, b, c), r) in lm {
            s.push_str(&format!("lmark {a} {b} {c} {r}\n"));
        }
        let mut pos: Vec<_> = self.pos.iter().collect();
        pos.sort();
        for ((a, b), r) in pos {
            s.push_str(&format!("pos {a} {b} {r}\n"));
        }
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(p, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("phrack-tui-test-{}", std::process::id()));
        let p = dir.join("state");
        let mut s = PState::load(Some(p.clone()));
        s.read.insert((25, 3));
        s.last = Some((25, 3, 99));
        s.global_marks.insert('A', (1, 2, 3));
        s.local_marks.insert((25, 3, 'a'), 7);
        s.pos.insert((25, 3), 99);
        s.theme = Some("amber".into());
        s.save();
        let t = PState::load(Some(p));
        assert!(t.read.contains(&(25, 3)));
        assert_eq!(t.last, Some((25, 3, 99)));
        assert_eq!(t.global_marks[&'A'], (1, 2, 3));
        assert_eq!(t.local_marks[&(25, 3, 'a')], 7);
        assert_eq!(t.theme.as_deref(), Some("amber"));
        let _ = fs::remove_dir_all(dir);
    }
}
