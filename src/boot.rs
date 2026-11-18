//! The fake 1985 dial-up boot sequence.

use std::time::Instant;

pub struct Boot {
    start: Instant,
    /// (start ms, text, ms per character)
    lines: Vec<(u64, String, u64)>,
    end: u64,
}

impl Boot {
    pub fn new(issues: usize, philes: usize) -> Boot {
        let script: Vec<(u64, String, u64)> = vec![
            (0, "PHRACK-TUI BIOS v1.985  (C) 1985 METAL SHOP SYSTEMS".into(), 7),
            (330, "MEMORY TEST ............ 640K OK".into(), 9),
            (640, "PHOSPHOR WARM-UP ........ P1 GREEN".into(), 9),
            (950, String::new(), 1),
            (980, "ATZ".into(), 50),
            (1150, "OK".into(), 50),
            (1260, "ATDT 314-432-0756".into(), 35),
            (1900, "CONNECT 1200".into(), 30),
            (2200, "CARRIER DETECTED. MOUNTING ARCHIVE ...".into(), 10),
            (2600, format!("{issues} ISSUES / {philes} PHILES ON LINE"), 10),
            (2850, String::new(), 1),
            (2880, "WELCOME, LAMER.  [F1] HELP   [CTRL-P] FIND   [ANY KEY] SKIP".into(), 8),
        ];
        let end = script.iter().map(|(t, s, sp)| t + s.chars().count() as u64 * sp).max().unwrap_or(0) + 450;
        Boot { start: Instant::now(), lines: script, end }
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
