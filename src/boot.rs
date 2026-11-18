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
}

pub struct Boot {
    start: Instant,
    /// (start ms, text, ms per character)
    lines: Vec<(u64, String, u64)>,
    end: u64,
}

/// `LABEL ........ VALUE`, dotted out to a fixed column.
fn dotted(label: &str, value: &str) -> String {
    format!("{:.<27} {}", format!("{label} "), value)
}

pub fn script(i: &BootInfo) -> Vec<(u64, String, u64)> {
    let fx: Vec<&str> = [(i.scan, "SCAN"), (i.glow, "BLOOM"), (i.vignette, "TUBE"), (i.flicker, "FLICKER")]
        .iter()
        .filter(|(b, _)| *b)
        .map(|(_, n)| *n)
        .collect();
    let fx = if fx.is_empty() { "OFF (FLAT PANEL)".to_string() } else { fx.join(" ") };
    let mut lines: Vec<(u64, String, u64)> = vec![
        (0, "PHRACK-TUI BIOS v1.985  (C) 1985 METAL SHOP SYSTEMS".into(), 7),
        (330, dotted("MEMORY TEST", "640K OK"), 9),
        (600, dotted("PHOSPHOR WARM-UP", i.phosphor), 9),
        (870, dotted("DISPLAY", if i.truecolor { "24-BIT RGB" } else { "256 COLOUR" }), 9),
        (1140, dotted("CRT", &fx), 9),
        (1400, dotted("PALETTE", &i.theme.to_uppercase()), 9),
        (1660, String::new(), 1),
        (1690, "ATZ".into(), 50),
        (1860, "OK".into(), 50),
        (1970, "ATDT 314-432-0756".into(), 35),
        (2600, "CONNECT 1200".into(), 30),
        (2900, "CARRIER DETECTED. MOUNTING ARCHIVE ...".into(), 10),
        (3300, format!("{} ISSUES / {} PHILES ON LINE", i.issues, i.philes), 10),
    ];
    let mut t = 3550;
    if i.read == 0 {
        lines.push((t, "NEW CALLER. NO PHILES READ YET.".into(), 10));
    } else {
        let mut s = format!("{}/{} PHILES READ", i.read, i.philes);
        if i.bookmarks + i.snippets > 0 {
            s.push_str(&format!(", {} BOOKMARKS, {} SNIPPETS", i.bookmarks, i.snippets));
        }
        lines.push((t, s, 10));
    }
    t += 260;
    if let Some((issue, art)) = i.last {
        lines.push((t, format!("LAST CALL: PHRACK {issue}, PHILE {art}"), 10));
        t += 260;
    }
    lines.push((t, String::new(), 1));
    lines.push((t + 30, "WELCOME, LAMER.  [F1] HELP   [CTRL-P] FIND   [ANY KEY] SKIP".into(), 8));
    // keep it brisk: the whole thing runs in about 3-4 seconds
    for l in &mut lines {
        l.0 = l.0 * 3 / 4;
        l.2 = (l.2 * 3 / 4).max(1);
    }
    lines
}

impl Boot {
    pub fn new(info: &BootInfo) -> Boot {
        let lines = script(info);
        let end = lines.iter().map(|(t, s, sp)| t + s.chars().count() as u64 * sp).max().unwrap_or(0) + 450;
        Boot { start: Instant::now(), lines, end }
    }

    fn elapsed(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
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
    }
}
