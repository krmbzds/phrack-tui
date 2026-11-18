//! The fake 1985 dial-up boot sequence. Every line that describes the machine
//! is generated from the live configuration (theme, effects, archive, history).

use std::time::Instant;

/// What the boot screen reports about the current setup.
pub struct BootInfo {
    pub issues: usize,
    pub philes: usize,
    pub phosphor: &'static str,
    pub theme: &'static str,
    pub truecolor: bool,
    pub scan: bool,
    pub glow: bool,
    pub vignette: bool,
    pub flicker: bool,
    pub read: usize,
    pub bookmarks: usize,
    pub snippets: usize,
    pub last: Option<(u32, u32)>,
    /// Archive host from the config, and where it is stored (both shown on the boot screen).
    pub host: String,
    pub storage: String,
}

pub struct Boot {
    start: Instant,
    /// (start ms, text, ms per character)
    lines: Vec<(u64, String, u64)>,
    end: u64,
    /// Fixed clock for offline rendering (the demo recorder); `None` = wall clock.
    clock: Option<u64>,
}

/// `LABEL ........ VALUE`, dotted out to a fixed column.
fn dotted(label: &str, value: &str) -> String {
    format!("{:.<27} {}", format!("{label} "), value)
}

/// Trim a path to `max` columns, keeping the tail.
fn tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        format!("…{}", s.chars().skip(n - max + 1).collect::<String>())
    }
}

pub fn script(i: &BootInfo) -> Vec<(u64, String, u64)> {
    let fx: Vec<&str> = [(i.scan, "SCAN"), (i.glow, "BLOOM"), (i.vignette, "TUBE"), (i.flicker, "FLICKER")]
        .iter()
        .filter(|(b, _)| *b)
        .map(|(_, n)| *n)
        .collect();
    let fx = if fx.is_empty() { "OFF (FLAT PANEL)".to_string() } else { fx.join(" ") };
    let empty = i.philes == 0;

    // (text, ms per character, pause after the line)
    let mut steps: Vec<(String, u64, u64)> = vec![
        ("PHRACK-TUI BIOS v1.985  (C) 1985 METAL SHOP SYSTEMS".into(), 7, 330),
        (dotted("MEMORY TEST", "640K OK"), 9, 270),
        (dotted("PHOSPHOR WARM-UP", i.phosphor), 9, 270),
        (dotted("DISPLAY", if i.truecolor { "24-BIT RGB" } else { "256 COLOUR" }), 9, 270),
        (dotted("CRT", &fx), 9, 260),
        (dotted("PALETTE", &i.theme.to_uppercase()), 9, 260),
        (dotted("ARCHIVE", &i.host), 9, 260),
        (dotted("STORAGE", &tail(&i.storage, 36)), 9, 300),
        (String::new(), 1, 30),
        ("ATZ".into(), 50, 140),
        ("OK".into(), 50, 110),
        ("ATDT 314-432-0756".into(), 35, 630),
        ("CONNECT 1200".into(), 30, 300),
        ("CARRIER DETECTED. MOUNTING ARCHIVE ...".into(), 10, 400),
    ];
    if empty {
        steps.push(("ARCHIVE EMPTY: NO PHILES ON DISK".into(), 10, 300));
        steps.push((format!("FIRST CALL. READY TO DIAL {}", i.host.to_uppercase()), 10, 260));
    } else {
        steps.push((format!("{} ISSUES / {} PHILES ON LINE", i.issues, i.philes), 10, 250));
        if i.read == 0 {
            steps.push(("NEW CALLER. NO PHILES READ YET.".into(), 10, 260));
        } else {
            let mut s = format!("{}/{} PHILES READ", i.read, i.philes);
            if i.bookmarks + i.snippets > 0 {
                s.push_str(&format!(", {} BOOKMARKS, {} SNIPPETS", i.bookmarks, i.snippets));
            }
            steps.push((s, 10, 260));
        }
        if let Some((issue, art)) = i.last {
            steps.push((format!("LAST CALL: PHRACK {issue}, PHILE {art}"), 10, 260));
        }
    }
    steps.push((String::new(), 1, 30));
    steps.push(("WELCOME, LAMER.  [F1] HELP   [CTRL-P] FIND   [ANY KEY] SKIP".into(), 8, 0));

    // keep it brisk: the whole thing runs in about 3-4 seconds
    let mut t = 0u64;
    let mut lines = Vec::new();
    for (text, speed, pause) in steps {
        let speed = (speed * 3 / 4).max(1);
        lines.push((t * 3 / 4, text.clone(), speed));
        t += pause; // gap between line starts; long lines overlap the next one a little
    }
    lines
}

impl Boot {
    pub fn new(info: &BootInfo) -> Boot {
        let lines = script(info);
        let end = lines.iter().map(|(t, s, sp)| t + s.chars().count() as u64 * sp).max().unwrap_or(0) + 450;
        Boot { start: Instant::now(), lines, end, clock: None }
    }

    fn elapsed(&self) -> u64 {
        self.clock.unwrap_or_else(|| self.start.elapsed().as_millis() as u64)
    }

    pub fn set_clock(&mut self, ms: u64) {
        self.clock = Some(ms);
    }

    pub fn end_ms(&self) -> u64 {
        self.end
    }

    pub fn done(&self) -> bool {
        self.elapsed() >= self.end
    }

    /// Lines as currently typed out.
    pub fn visible(&self) -> Vec<String> {
        self.visible_at(self.elapsed())
    }

    pub fn visible_at(&self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        for (start, text, speed) in &self.lines {
            if t < *start {
                break;
            }
            let n = (((t - start) / (*speed).max(1)) as usize).min(text.chars().count());
            out.push(text.chars().take(n).collect());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> BootInfo {
        BootInfo {
            issues: 73,
            philes: 1057,
            phosphor: "P3 AMBER",
            theme: "amber",
            truecolor: true,
            scan: true,
            glow: false,
            vignette: true,
            flicker: false,
            read: 0,
            bookmarks: 0,
            snippets: 0,
            last: None,
            host: "archives.phrack.org".into(),
            storage: "~/.local/share/phrack-tui/issues".into(),
        }
    }

    #[test]
    fn reflects_configuration() {
        let all: Vec<String> = script(&info()).into_iter().map(|l| l.1).collect();
        let text = all.join("\n");
        assert!(text.contains("P3 AMBER") && !text.contains("P1 GREEN"));
        assert!(text.contains("SCAN TUBE") && !text.contains("BLOOM"));
        assert!(text.contains("PALETTE") && text.contains("AMBER"));
        assert!(text.contains("NEW CALLER"));
        assert!(text.contains("ARCHIVE ") && text.contains("archives.phrack.org"));
        assert!(text.contains("STORAGE ") && text.contains("~/.local/share/phrack-tui/issues"));
        let mut i = info();
        i.read = 12;
        i.snippets = 2;
        i.last = Some((25, 3));
        i.scan = false;
        i.vignette = false;
        let text: String = script(&i).into_iter().map(|l| l.1).collect::<Vec<_>>().join("\n");
        assert!(text.contains("12/1057 PHILES READ, 0 BOOKMARKS, 2 SNIPPETS"));
        assert!(text.contains("LAST CALL: PHRACK 25, PHILE 3"));
        assert!(text.contains("OFF (FLAT PANEL)"));
        // an empty archive says so instead of quoting counts
        i.philes = 0;
        i.issues = 0;
        let text: String = script(&i).into_iter().map(|l| l.1).collect::<Vec<_>>().join("\n");
        assert!(text.contains("ARCHIVE EMPTY") && text.contains("READY TO DIAL ARCHIVES.PHRACK.ORG"));
        assert!(!text.contains("PHILES READ") && !text.contains("LAST CALL"));
    }
}
