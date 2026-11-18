//! Records the animated README demo.
//!
//! A scripted key sequence is fed to the real [`App`]; every resulting frame is
//! rendered by the real UI code and serialised into one SMIL-animated SVG.
//! Terminal cells become `<text>`/`<rect>` rows that are shown or hidden on a
//! shared timeline; the CRT look (tube gradient, vignette, scanlines, bloom,
//! flicker) is applied as SVG overlays so the file stays small.
//!
//! The recording starts from an EMPTY library, shows the first-run download prompt
//! and replays the real archive through the same events the downloader emits, so
//! it needs a local copy of the archive but never touches the network or your config.
//!
//!     cargo run --release -- --demo-svg docs/demo.svg

use std::{collections::HashMap, fmt::Write as _, fs, thread, time::Duration};

use unicode_width::UnicodeWidthChar;

use crate::{
    app::{App, Overlay},
    boot::Boot,
    buffer::{Buffer, Style},
    collection::Collection,
    config::Config,
    crt::Fx,
    download::Event,
    data::Library,
    input::K,
    state::PState,
    theme::{self, Rgb, Theme},
    ui,
};

const COLS: u16 = 100;
const ROWS: u16 = 30;
const CW: f64 = 8.4; // advance of a 14px monospace glyph
const CH: f64 = 17.0;
// 8.4 px per cell / 0.6021 em advance (Menlo / Meslo / DejaVu Sans Mono) = 13.95 px
const FONT: f64 = 13.95;
const FONT_STACK: &str = "PhrackMono,'MesloLGS NF',Menlo,'SF Mono',Consolas,'DejaVu Sans Mono','Liberation Mono',monospace";
const PAD: f64 = 20.0; // bezel
const CAP_H: f64 = 50.0; // caption strip under the monitor

/// Optional terminal font to embed in the SVG so it looks the same everywhere.
///
/// Point `PHRACK_DEMO_FONT_DIR` at a directory holding `regular.woff2`, `bold.woff2`
/// and `covered-regular.txt` / `covered-bold.txt` (hex ranges, e.g. `20-7e a0-ff`).
/// Nothing is stored in the repository: the font only ends up inside the generated SVG.
/// Without it the SVG uses whatever monospace font the viewer has.
#[derive(Default)]
struct Fonts {
    regular: Option<Vec<u8>>,
    bold: Option<Vec<u8>>,
    cov_regular: Vec<(u32, u32)>,
    cov_bold: Vec<(u32, u32)>,
}

impl Fonts {
    fn load() -> Fonts {
        let Some(dir) = std::env::var_os("PHRACK_DEMO_FONT_DIR").map(std::path::PathBuf::from) else {
            return Fonts::default();
        };
        let ranges = |name: &str| -> Vec<(u32, u32)> {
            fs::read_to_string(dir.join(name))
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|r| {
                    let (a, b) = r.split_once('-')?;
                    Some((u32::from_str_radix(a, 16).ok()?, u32::from_str_radix(b, 16).ok()?))
                })
                .collect()
        };
        Fonts {
            regular: fs::read(dir.join("regular.woff2")).ok(),
            bold: fs::read(dir.join("bold.woff2")).ok(),
            cov_regular: ranges("covered-regular.txt"),
            cov_bold: ranges("covered-bold.txt"),
        }
    }

    /// Is `ch` drawn by the embedded font in this weight? Without an embedded font we
    /// trust the viewer's monospace font for ASCII and the box-drawing/block range.
    fn covers(&self, ch: char, bold: bool) -> bool {
        let (font, cov) = if bold { (&self.bold, &self.cov_bold) } else { (&self.regular, &self.cov_regular) };
        if font.is_none() {
            return ch.is_ascii() || ('\u{2500}'..='\u{259f}').contains(&ch);
        }
        let c = ch as u32;
        cov.iter().any(|&(a, b)| c >= a && c <= b)
    }
}

struct Frame {
    start: u64,
    dur: u64,
    rows: Vec<u32>,
    theme: usize,
    caption: Option<usize>,
}

struct Caption {
    title: String,
    keys: String,
    at: u64,
}

struct Recorder {
    app: App,
    corpus: Library,
    fonts: Fonts,
    buf: Buffer,
    t: u64,
    frames: Vec<Frame>,
    table: Vec<String>,
    intern: HashMap<(usize, String), u32>,
    themes: Vec<Theme>,
    captions: Vec<Caption>,
    cur_caption: Option<usize>,
    msg: Option<(String, u64)>,
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn f1(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

impl Recorder {
    fn new(lib: Library) -> Recorder {
        let st = PState::default();
        let root = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".local/share/phrack-tui/issues"))
            .unwrap_or_else(|| crate::paths::data_dir().join("issues"));
        let mut app = App::new(Library { issues: Vec::new() }, st, Collection::default(), Config::default(), root, (COLS, ROWS));
        app.dl_offline = true; // replay, never download
        app.fx = Fx::none(); // the SVG overlays do the CRT effects
        app.seed_rng(0x00c0_ffee);
        let th = app.theme.clone();
        Recorder {
            corpus: lib,
            fonts: Fonts::load(),
            buf: Buffer::new(COLS, ROWS, Style::new(th.fg, th.bg)),
            app,
            t: 0,
            frames: Vec::new(),
            table: Vec::new(),
            intern: HashMap::new(),
            themes: theme::all(),
            captions: Vec::new(),
            cur_caption: None,
            msg: None,
        }
    }

    // ---- frame capture ------------------------------------------------------

    fn expire_msg(&mut self) {
        let Some(m) = &self.app.msg else {
            self.msg = None;
            return;
        };
        match &self.msg {
            Some((t, since)) if *t == m.text => {
                if self.t.saturating_sub(*since) > 2600 {
                    self.app.msg = None;
                    self.msg = None;
                }
            }
            _ => self.msg = Some((m.text.clone(), self.t)),
        }
    }

    /// Scripted pauses and key delays are shortened a little so the loop stays brisk.
    fn snap(&mut self, dur: u64) {
        self.snap_raw((dur * 72 / 100).max(25));
    }

    fn snap_raw(&mut self, dur: u64) {
        self.expire_msg();
        ui::render(&self.app, &mut self.buf);
        let th = self.app.theme.clone();
        let theme = self.themes.iter().position(|t| t.name == th.name).unwrap_or(0);
        let rows: Vec<u32> = (0..ROWS as usize).map(|y| self.row_id(y, &th)).collect();
        match self.frames.last_mut() {
            Some(l) if l.rows == rows && l.theme == theme && l.caption == self.cur_caption => l.dur += dur,
            _ => self.frames.push(Frame { start: self.t, dur, rows, theme, caption: self.cur_caption }),
        }
        self.t += dur;
    }

    fn wait(&mut self, ms: u64) {
        // pacing is scaled, but the cursor keeps blinking at its real rate
        let (mut left, mut on) = ((ms * 72 / 100).max(25), true);
        while left > 0 {
            let d = left.min(530);
            self.app.blink = on;
            self.snap_raw(d);
            on = !on;
            left -= d;
        }
    }

    fn key(&mut self, k: K, delay: u64) {
        self.app.on_key(k);
        self.app.blink = true;
        self.snap(delay);
    }

    /// Type a key string; `<CR> <Esc> <Tab> <S-Tab> <BS> <F1> <C-x>` are special.
    fn keys(&mut self, s: &str, delay: u64) {
        let cs: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let k = if cs[i] == '<' {
                match cs[i..].iter().position(|&c| c == '>') {
                    Some(j) => {
                        let tok: String = cs[i + 1..i + j].iter().collect();
                        i += j + 1;
                        match tok.as_str() {
                            "CR" => K::Enter,
                            "Esc" => K::Esc,
                            "Tab" => K::Tab,
                            "S-Tab" => K::BackTab,
                            "BS" => K::Backspace,
                            "F1" => K::F1,
                            t if t.starts_with("C-") => K::Ctrl(t.chars().nth(2).unwrap_or('x')),
                            _ => continue,
                        }
                    }
                    None => {
                        i += 1;
                        K::Char('<')
                    }
                }
            } else {
                i += 1;
                K::Char(cs[i - 1])
            };
            self.key(k, delay);
        }
    }

    fn caption(&mut self, title: &str, keys: &str) {
        self.captions.push(Caption { title: title.into(), keys: keys.into(), at: self.t });
        self.cur_caption = Some(self.captions.len() - 1);
    }

    fn wait_grep(&mut self) {
        for _ in 0..400 {
            self.app.tick();
            match &self.app.overlay {
                Overlay::Picker(p) if p.searching => thread::sleep(Duration::from_millis(15)),
                _ => break,
            }
        }
    }

    fn boot(&mut self) {
        // the SVG overlays provide the effects, so the boot screen should report them as on
        let real = std::mem::replace(&mut self.app.fx, Fx::default());
        self.app.boot = Some(Boot::new(&self.app.boot_info(true)));
        self.app.fx = real;
        let end = self.app.boot.as_ref().map_or(0, Boot::end_ms);
        let mut t = 0;
        while t < end {
            if let Some(b) = self.app.boot.as_mut() {
                b.set_clock(t);
            }
            self.app.blink = (t / 530) % 2 == 0;
            self.snap_raw(50);
            t += 50;
        }
        self.app.boot = None;
        self.app.start(); // empty archive: the "Download issues? Y/n" prompt appears
    }

    /// Replay the archive through the downloader's own event stream: a short
    /// "dialling" moment, then philes landing in issue order, then the hand-over.
    /// A lived-in profile: some issues already read, so progress bars have something to show.
    fn seed_read_marks(&mut self) {
        let st = &mut self.app.st;
        for i in self.corpus.issues.iter().filter(|i| i.num <= 6 || i.num == 49) {
            for a in &i.articles {
                st.read.insert((i.num, a.num));
            }
        }
        for a in 1..=8 {
            st.read.insert((50, a));
        }
        st.read.insert((25, 1));
        st.read.insert((25, 2));
    }

    fn download_replay(&mut self) {
        let mut evs = vec![Event::Indexed(self.corpus.issues.len())];
        for i in &self.corpus.issues {
            evs.push(Event::Listed { issue: i.num, files: i.articles.len() });
            for a in &i.articles {
                evs.push(Event::Saved { issue: i.num, art: a.num, path: a.path.clone(), bytes: a.size, fresh: true });
            }
        }
        const STEP: u64 = 120;
        const CONNECT: u64 = 6;
        const FRAMES: u64 = 52;
        if let Some(dl) = self.app.dl.as_mut() {
            dl.rate_scale = 8.0; // this replay runs ~8x faster than a real download
        }
        let n = evs.len();
        let mut sent = 0;
        for f in 0..CONNECT + FRAMES {
            if let Some(dl) = self.app.dl.as_mut() {
                dl.clock = Some((f + 1) * STEP);
            }
            if f >= CONNECT {
                let k = (f - CONNECT + 1) as usize;
                let upto = if k as u64 == FRAMES { n } else { n * k / FRAMES as usize };
                for ev in evs[sent..upto].iter().cloned() {
                    self.app.apply_dl_event(ev);
                }
                sent = upto;
            }
            self.app.blink = (f / 4) % 2 == 0;
            self.snap_raw(STEP);
        }
        self.app.finish_download();
        while self.app.dl.is_some() {
            if let Some(dl) = self.app.dl.as_mut() {
                dl.clock = Some(dl.clock.unwrap_or(0) + STEP);
            }
            self.app.dl_tick(); // hands over to the main screen after the linger
            self.snap_raw(STEP);
        }
    }

    // ---- row -> SVG fragment --------------------------------------------------

    fn row_id(&mut self, y: usize, th: &Theme) -> u32 {
        let frag = self.row_fragment(y, th);
        if let Some(&id) = self.intern.get(&(y, frag.clone())) {
            return id;
        }
        let id = self.table.len() as u32;
        self.table.push(frag.clone());
        self.intern.insert((y, frag), id);
        id
    }

    fn row_fragment(&self, y: usize, th: &Theme) -> String {
        let b = &self.buf;
        let top = y as f64 * CH;
        let base = top + 13.0;
        let w = b.w as usize;
        let mut out = String::new();

        // background runs (cells that are not the flat theme background)
        let mut x = 0;
        while x < w {
            let c = b.get(x as u16, y as u16);
            if c.st.bg == th.bg {
                x += 1;
                continue;
            }
            let bg = c.st.bg;
            let s = x;
            while x < w && b.get(x as u16, y as u16).st.bg == bg {
                x += 1;
            }
            let _ = write!(out, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>", f1(s as f64 * CW), f1(top), f1((x - s) as f64 * CW + 0.4), f1(CH), hex(bg));
        }

        // text runs
        struct Run {
            x0: usize,
            text: String,
            xs: Vec<f64>,
            cells: usize,
            fg: Rgb,
            bold: bool,
            /// Every glyph comes from the font that will draw it, so the browser's own
            /// (fixed) advance gives the right spacing. Otherwise pin each glyph to its cell.
            natural: bool,
        }
        let mut runs: Vec<Run> = Vec::new();
        let mut cur: Option<Run> = None;
        let mut spaces = 0usize;
        let mut ul: Vec<(usize, usize, Rgb)> = Vec::new();
        let mut x = 0usize;
        while x < w {
            let c = *b.get(x as u16, y as u16);
            if c.ch == '\0' {
                x += 1;
                continue;
            }
            let cw = c.ch.width().unwrap_or(1).max(1);
            if c.ch == ' ' {
                spaces += 1;
                x += 1;
                continue;
            }
            if c.st.ul {
                match ul.last_mut() {
                    Some(l) if l.1 == x && l.2 == c.st.fg => l.1 = x + cw,
                    _ => ul.push((x, x + cw, c.st.fg)),
                }
            }
            // Bold Meslo lacks most box-drawing; those glyphs are identical lines, so draw them regular.
            let bold = c.st.bold && (self.fonts.covers(c.ch, true) || !self.fonts.covers(c.ch, false));
            let same = cur.as_ref().is_some_and(|r| r.fg == c.st.fg && r.bold == bold && spaces <= 2);
            if !same {
                if let Some(r) = cur.take() {
                    runs.push(r);
                }
                cur = Some(Run { x0: x, text: String::new(), xs: Vec::new(), cells: 0, fg: c.st.fg, bold, natural: true });
                spaces = 0;
            }
            let r = cur.as_mut().unwrap();
            for _ in 0..spaces {
                r.text.push(' ');
                r.xs.push((r.x0 + r.cells) as f64 * CW);
                r.cells += 1;
            }
            spaces = 0;
            r.text.push(c.ch);
            r.xs.push(x as f64 * CW);
            r.cells = x + cw - r.x0;
            if !self.fonts.covers(c.ch, r.bold) {
                r.natural = false;
            }
            x += cw;
        }
        if let Some(r) = cur.take() {
            runs.push(r);
        }
        for (a, e, col) in ul {
            let _ = write!(out, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"1\" fill=\"{}\"/>", f1(a as f64 * CW), f1(top + 14.5), f1((e - a) as f64 * CW), hex(col));
        }
        for r in runs {
            let _ = write!(out, "<text y=\"{}\" fill=\"{}\"", f1(base), hex(r.fg));
            if r.bold {
                out.push_str(" font-weight=\"700\"");
            }
            if r.natural {
                // the font's fixed advance lays the glyphs out: no stretching, no spacing tricks
                let _ = write!(out, " x=\"{}\"", f1(r.x0 as f64 * CW));
            } else {
                let xs: Vec<String> = r.xs.iter().map(|v| f1(*v)).collect();
                let _ = write!(out, " x=\"{}\"", xs.join(" "));
            }
            let _ = write!(out, ">{}</text>", esc(&r.text));
        }
        out
    }
}

// ---- SVG assembly -------------------------------------------------------------

/// A discrete 0/1 `opacity` animation that is "on" during `iv` (ms), looping over `total`.
fn visibility(iv: &[(u64, u64)], total: u64) -> String {
    let mut pts: Vec<(u64, u8)> = Vec::new();
    pts.push((0, (iv[0].0 == 0) as u8));
    for &(s, e) in iv {
        if s > 0 {
            pts.push((s, 1));
        }
        if e < total {
            pts.push((e, 0));
        }
    }
    let last = pts.last().map_or(0, |p| p.1);
    pts.push((total, last));
    let kt: Vec<String> = pts.iter().map(|p| format!("{:.5}", p.0 as f64 / total as f64).trim_end_matches('0').trim_end_matches('.').to_string()).collect();
    let kt: Vec<String> = kt.into_iter().map(|s| if s.is_empty() { "0".into() } else { s }).collect();
    let vals: Vec<String> = pts.iter().map(|p| p.1.to_string()).collect();
    format!(
        "<animate attributeName=\"opacity\" calcMode=\"discrete\" dur=\"{}s\" repeatCount=\"indefinite\" keyTimes=\"{}\" values=\"{}\"/>",
        f1(total as f64 / 1000.0),
        kt.join(";"),
        vals.join(";")
    )
}

fn merge(iv: &mut Vec<(u64, u64)>) {
    iv.sort();
    let mut out: Vec<(u64, u64)> = Vec::new();
    for &(s, e) in iv.iter() {
        match out.last_mut() {
            Some(l) if l.1 >= s => l.1 = l.1.max(e),
            _ => out.push((s, e)),
        }
    }
    *iv = out;
}

/// Short opacity pulses at the given times, linear in between.
fn pulses(times: &[u64], peak: f64, len: u64, total: u64) -> String {
    let mut pts: Vec<(u64, f64)> = vec![(0, 0.0)];
    for &s in times {
        let last = pts.last().unwrap().0;
        let s = s.max(last + 2);
        pts.push((s, 0.0));
        pts.push((s + 30, peak));
        pts.push(((s + len).min(total.saturating_sub(1)), 0.0));
    }
    pts.push((total, 0.0));
    pts.dedup_by_key(|p| p.0);
    let kt: Vec<String> = pts.iter().map(|p| format!("{:.5}", p.0 as f64 / total as f64)).collect();
    let vals: Vec<String> = pts.iter().map(|p| format!("{:.2}", p.1)).collect();
    format!(
        "<animate attributeName=\"opacity\" dur=\"{}s\" repeatCount=\"indefinite\" keyTimes=\"{}\" values=\"{}\"/>",
        f1(total as f64 / 1000.0),
        kt.join(";"),
        vals.join(";")
    )
}

/// How an element with these on-intervals should be emitted: `None` = leave it out,
/// otherwise (extra attributes, animation child). With `still` set, only elements
/// visible at that instant are kept, unanimated (used to preview single moments).
fn vis(iv: &[(u64, u64)], still: Option<u64>, total: u64) -> Option<(&'static str, String)> {
    match still {
        Some(t) => iv.iter().any(|&(s, e)| t >= s && t < e).then(|| ("", String::new())),
        None if iv.len() == 1 && iv[0] == (0, total) => Some(("", String::new())),
        None => Some((" opacity=\"0\"", visibility(iv, total))),
    }
}

impl Recorder {
    fn svg(&self, still: Option<u64>) -> String {
        let total = self.t.max(1);
        let tw = COLS as f64 * CW;
        let th = ROWS as f64 * CH;
        let (ow, oh) = (tw + PAD * 2.0, th + PAD * 2.0);
        let (vw, vh) = (ow, oh + CAP_H);
        let mut s = String::new();
        let _ = write!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"phrack-tui demo: browsing, reading, searching and bookmarking the Phrack archive on an amber, green and violet CRT\">\n",
            f1(vw), f1(vh), f1(vw), f1(vh)
        );

        // which themes appear, and when
        let mut theme_iv: Vec<Vec<(u64, u64)>> = vec![Vec::new(); self.themes.len()];
        for f in &self.frames {
            theme_iv[f.theme].push((f.start, f.start + f.dur));
        }
        theme_iv.iter_mut().for_each(merge);

        s.push_str("<defs>\n");
        if self.fonts.regular.is_some() || self.fonts.bold.is_some() {
            s.push_str("<style>");
            for (bytes, weight) in [(&self.fonts.regular, 400), (&self.fonts.bold, 700)] {
                if let Some(b) = bytes {
                    let _ = write!(s, "@font-face{{font-family:'PhrackMono';font-weight:{weight};src:url(data:font/woff2;base64,{}) format('woff2')}}", crate::app::base64(b));
                }
            }
            s.push_str("</style>\n");
        }
        for (i, t) in self.themes.iter().enumerate() {
            if theme_iv[i].is_empty() {
                continue;
            }
            let _ = writeln!(s, "<radialGradient id=\"bg{i}\" cx=\"50%\" cy=\"50%\" r=\"75%\"><stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/></radialGradient>", hex(t.bg_center), hex(t.bg));
        }
        s.push_str("<radialGradient id=\"vig\" cx=\"50%\" cy=\"50%\" r=\"72%\"><stop offset=\"0.55\" stop-color=\"#000\" stop-opacity=\"0\"/><stop offset=\"1\" stop-color=\"#000\" stop-opacity=\"0.62\"/></radialGradient>\n");
        s.push_str("<pattern id=\"scan\" width=\"4\" height=\"3\" patternUnits=\"userSpaceOnUse\"><rect width=\"4\" height=\"1\" y=\"2\" fill=\"#000\" fill-opacity=\"0.20\"/></pattern>\n");
        s.push_str("<filter id=\"glow\" x=\"-2%\" y=\"-2%\" width=\"104%\" height=\"104%\"><feGaussianBlur stdDeviation=\"1.5\" result=\"b\"/><feMerge><feMergeNode in=\"b\"/><feMergeNode in=\"SourceGraphic\"/></feMerge></filter>\n");
        s.push_str("<linearGradient id=\"shine\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"><stop offset=\"0\" stop-color=\"#fff\" stop-opacity=\"0.07\"/><stop offset=\"0.5\" stop-color=\"#fff\" stop-opacity=\"0\"/></linearGradient>\n");
        let _ = writeln!(s, "<clipPath id=\"screen\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"16\"/></clipPath>", f1(PAD), f1(PAD), f1(tw), f1(th));
        s.push_str("</defs>\n");

        // page + monitor bezel
        let _ = writeln!(s, "<rect width=\"{}\" height=\"{}\" fill=\"#0a0a0a\"/>", f1(vw), f1(vh));
        let _ = writeln!(s, "<rect x=\"1\" y=\"1\" width=\"{}\" height=\"{}\" rx=\"24\" fill=\"#15161a\" stroke=\"#2b2d33\" stroke-width=\"2\"/>", f1(ow - 2.0), f1(oh - 2.0));
        let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"18\" fill=\"#000\" stroke=\"#07080a\" stroke-width=\"3\"/>", f1(PAD - 2.0), f1(PAD - 2.0), f1(tw + 4.0), f1(th + 4.0));

        // screen
        s.push_str("<g clip-path=\"url(#screen)\">\n");
        for (i, iv) in theme_iv.iter().enumerate() {
            if iv.is_empty() {
                continue;
            }
            if let Some((attr, anim)) = vis(iv, still, total) {
                let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#bg{i})\"{attr}>{anim}</rect>", f1(PAD), f1(PAD), f1(tw), f1(th));
            }
        }

        // rows: one <g> per distinct row content, shown during its intervals
        let mut seg: HashMap<u32, Vec<(u64, u64)>> = HashMap::new();
        for f in &self.frames {
            for &id in &f.rows {
                seg.entry(id).or_default().push((f.start, f.start + f.dur));
            }
        }
        let _ = writeln!(s, "<g filter=\"url(#glow)\" transform=\"translate({} {})\" font-family=\"{}\" font-size=\"{}\" text-rendering=\"geometricPrecision\" style=\"font-kerning:none;font-variant-ligatures:none\" xml:space=\"preserve\">", f1(PAD), f1(PAD), FONT_STACK, f1(FONT));
        let mut ids: Vec<u32> = seg.keys().copied().collect();
        ids.sort();
        for id in ids {
            let frag = &self.table[id as usize];
            if frag.is_empty() {
                continue;
            }
            let mut iv = seg.remove(&id).unwrap();
            merge(&mut iv);
            if let Some((attr, anim)) = vis(&iv, still, total) {
                let _ = writeln!(s, "<g{attr}>{frag}{anim}</g>");
            }
        }
        s.push_str("</g>\n");

        // CRT overlays
        let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#scan)\"/>", f1(PAD), f1(PAD), f1(tw), f1(th));
        let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#vig)\"/>", f1(PAD), f1(PAD), f1(tw), f1(th));
        let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#shine)\"/>", f1(PAD), f1(PAD), f1(tw), f1(th * 0.6));
        // degauss flash on every palette change, and a few power flickers
        let switches: Vec<u64> = if still.is_some() { Vec::new() } else { self.frames.windows(2).filter(|w| w[0].theme != w[1].theme).map(|w| w[1].start).collect() };
        if !switches.is_empty() {
            let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#fff\" opacity=\"0\">{}</rect>", f1(PAD), f1(PAD), f1(tw), f1(th), pulses(&switches, 0.20, 260, total));
        }
        let mut fl = Vec::new();
        let mut t = if still.is_some() { total } else { 6200u64 };
        let mut x = 7u64;
        while t < total.saturating_sub(800) {
            fl.push(t);
            x = x * 1103 % 1009;
            t += 7000 + (x % 5) * 700;
        }
        if !fl.is_empty() {
            let _ = writeln!(s, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#000\" opacity=\"0\">{}</rect>", f1(PAD), f1(PAD), f1(tw), f1(th), pulses(&fl, 0.12, 90, total));
        }
        s.push_str("</g>\n");

        // power LED, tinted by the current phosphor
        for (i, iv) in theme_iv.iter().enumerate() {
            if iv.is_empty() {
                continue;
            }
            if let Some((attr, anim)) = vis(iv, still, total) {
                let _ = writeln!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"2.4\" fill=\"{}\"{attr}>{anim}</circle>", f1(ow - 34.0), f1(oh - 9.0), hex(self.themes[i].fg));
            }
        }

        // captions
        let cy = oh;
        let _ = writeln!(s, "<rect x=\"0\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#0e0f12\"/>", f1(cy), f1(vw), f1(CAP_H));
        let mut cap_iv: HashMap<(usize, usize), Vec<(u64, u64)>> = HashMap::new();
        for f in &self.frames {
            if let Some(c) = f.caption {
                cap_iv.entry((c, f.theme)).or_default().push((f.start, f.start + f.dur));
            }
        }
        let mut keys: Vec<(usize, usize)> = cap_iv.keys().copied().collect();
        keys.sort();
        for (c, t) in keys {
            let mut iv = cap_iv.remove(&(c, t)).unwrap();
            merge(&mut iv);
            let cap = &self.captions[c];
            let th = &self.themes[t];
            let Some((attr, anim)) = vis(&iv, still, total) else { continue };
            let _ = writeln!(
                s,
                "<g{attr}><text x=\"24\" y=\"{}\" fill=\"{}\" font-family=\"-apple-system,'Segoe UI',Helvetica,Arial,sans-serif\" font-size=\"16\" font-weight=\"600\">{}</text><text x=\"{}\" y=\"{}\" text-anchor=\"end\" fill=\"{}\" font-family=\"{}\" font-size=\"13\" style=\"font-kerning:none\">{}</text>{anim}</g>",
                f1(cy + 30.0),
                hex(th.bright),
                esc(&cap.title),
                f1(vw - 24.0),
                f1(cy + 30.0),
                hex(th.dim.mix(th.fg, 0.55)),
                FONT_STACK,
                esc(&cap.keys),
            );
        }
        // timeline
        let _ = writeln!(s, "<rect x=\"0\" y=\"{}\" width=\"{}\" height=\"2\" fill=\"#23252b\"/>", f1(cy + CAP_H - 2.0), f1(vw));
        if still.is_none() {
        let _ = writeln!(s, "<rect x=\"0\" y=\"{}\" width=\"0\" height=\"2\" fill=\"#8a8f99\"><animate attributeName=\"width\" from=\"0\" to=\"{}\" dur=\"{}s\" repeatCount=\"indefinite\"/></rect>", f1(cy + CAP_H - 2.0), f1(vw), f1(total as f64 / 1000.0));
        }
        s.push_str("</svg>\n");
        s
    }
}

// ---- the script -------------------------------------------------------------------

fn script(r: &mut Recorder) {
    let (nav, typ) = (90, 60);

    r.caption("phrack-tui", "read Phrack in a terminal that looks the part");
    r.boot();
    r.caption("First run: fetch the archive", "Download issues?   ⏎ / y");
    r.wait(1500);
    r.keys("<CR>", 400);

    r.caption("Philes stream in as they download", "live progress  ·  esc aborts  ·  :download tops up later");
    r.download_replay();
    r.seed_read_marks();
    r.wait(500);

    r.caption("Browse every issue", "j k  G  25G  l  ⏎");
    r.wait(300);
    r.keys("G", nav);
    r.wait(350);
    r.keys("kkk", 110);
    r.keys("12G", nav);
    r.wait(300);
    r.keys("25G", nav);
    r.wait(350);
    r.keys("l", 200);
    r.keys("jj", 140);
    r.wait(450);

    r.caption("Read like you are in vim", "j  w  }  ^d  /pattern  n  v  gb");
    r.keys("<CR>", 250);
    r.wait(400);
    r.keys("jjjjjj", 80);
    r.keys("wwww", 90);
    r.keys("<C-d>", 300);
    r.keys("}", 300);
    r.keys("gg", 250);
    r.keys("/switch<CR>", typ);
    r.wait(500);
    r.keys("n", 500);
    r.keys("n", 500);
    r.keys("gb", 900); // a bookmark, so the later browser has something in it
    r.wait(700);

    r.caption("Jump between sections", "]]  [[  gs");
    r.keys("gg", 200);
    r.keys("]]", 600);
    r.keys("gs", 800);
    r.keys("j", 300);
    r.wait(300);
    r.keys("<CR>", 500);
    r.wait(500);

    r.caption("Tab-complete the command line", ":the⇥  ⇥  ⇥  ⏎   —   amber · crt · dracula");
    r.keys(":the<Tab>", 140);
    r.wait(400);
    r.keys("<Tab>", 600);
    r.keys("<Tab>", 700);
    r.wait(300);
    r.keys("<CR>", 300);
    r.wait(900);

    r.caption("Follow Phrack cross-references", "]r  f  a  ⏎        ^o  jumps back");
    r.keys(":51/5<CR>", typ);
    r.wait(500);
    r.keys("]r", 700);
    r.keys("f", 1200);
    r.keys("a", 700);
    r.wait(800);
    r.keys("<C-o>", 500);
    r.wait(700);

    r.caption("Fuzzy finder over every phile", "^p");
    r.keys("<C-p>", 400);
    r.keys("halvar", 85);
    r.wait(600);
    r.keys("<CR>", 400);
    r.wait(600);

    r.caption("Grep the whole archive", ":grep  ⏎  — opens at the hit");
    r.keys(":grep metal shop<CR>", 50);
    r.snap(300);
    r.wait_grep();
    r.wait(900);
    r.keys("jjj", 160);
    r.wait(400);
    r.keys("<CR>", 400);
    r.wait(700);

    r.caption("Three palettes", ":theme amber | crt | dracula");
    r.keys(":theme dr<Tab><CR>", 90);
    r.wait(1100);

    r.caption("Bookmark a line", "gb  ]b  [b  gB");
    r.keys("gb", 1300);
    r.wait(500);

    r.caption("Save a selection as a snippet", "V  j  s  name  ⏎");
    r.keys("9k", 160); // up to the table of contents of issue 1
    r.keys("V", 300);
    r.keys("jjj", 230);
    r.wait(400);
    r.keys("s", 500);
    r.keys("<C-u>Issue 1 contents<CR>", 60);
    r.wait(800);
    r.keys("j", 250);
    r.keys("k", 900);

    r.caption("Browse your collection", "gS  gB  ⇥  ⏎ jumps to the exact selection");
    r.keys("gS", 1500);
    r.keys("<Tab>", 1300);
    r.keys("j", 600);
    r.wait(400);
    r.keys("<CR>", 500);
    r.wait(800);

    r.caption("Phrack-aware navigation", "]i  ]a  gA  gr  gu");
    r.keys("]i", 700);
    r.keys("]a", 600);
    r.keys("gA", 500);
    r.keys("taran", 90);
    r.wait(700);
    r.keys("<Esc><Esc>", 200);
    r.keys("gr", 1100);
    r.wait(400);

    r.caption("…and back to amber", ":theme amber");
    r.keys(":theme am<Tab><CR>", 90);
    r.wait(900);
    r.keys("q", 600);
    r.caption("Everything is in F1", "F1  ·  :help");
    r.keys("<F1>", 900);
    r.keys("jjjjjjjj", 80);
    r.wait(700);
    r.keys("<Esc>", 500);
    r.wait(1300);
}

pub fn record(lib: Library, out: &str) -> std::io::Result<()> {
    let mut r = Recorder::new(lib);
    script(&mut r);
    let svg = r.svg(None);
    // PHRACK_DEMO_STILLS="2500,14000" also writes static previews of those instants
    if let Ok(list) = std::env::var("PHRACK_DEMO_STILLS") {
        for ms in list.split(',').filter_map(|v| v.trim().parse::<u64>().ok()) {
            fs::write(format!("{out}.still-{ms}.svg"), r.svg(Some(ms)))?;
        }
    }
    if let Some(dir) = std::path::Path::new(out).parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    fs::write(out, &svg)?;
    if std::env::var_os("PHRACK_DEMO_VERBOSE").is_some() {
        for c in &r.captions {
            eprintln!("{:>7.1}s  {}", c.at as f64 / 1000.0, c.title);
        }
    }
    eprintln!(
        "wrote {out}: {:.1}s, {} frames, {} distinct rows, {:.0} KB",
        r.t as f64 / 1000.0,
        r.frames.len(),
        r.table.len(),
        svg.len() as f64 / 1024.0
    );
    Ok(())
}
