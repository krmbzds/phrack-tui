//! A cell buffer with diff-based flushing. All drawing goes through here so
//! the CRT post-processing can operate on the finished frame.

use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor},
};
use unicode_width::UnicodeWidthChar;

use crate::theme::Rgb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Style {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub ul: bool,
}

impl Style {
    pub fn new(fg: Rgb, bg: Rgb) -> Style {
        Style { fg, bg, bold: false, ul: false }
    }
    pub fn bold(mut self) -> Style {
        self.bold = true;
        self
    }
    pub fn ul(mut self) -> Style {
        self.ul = true;
        self
    }
    pub fn fg(mut self, c: Rgb) -> Style {
        self.fg = c;
        self
    }
    pub fn bg(mut self, c: Rgb) -> Style {
        self.bg = c;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// `'\0'` marks the right half of a double-width glyph.
    pub ch: char,
    pub st: Style,
}

#[derive(Clone)]
pub struct Buffer {
    pub w: u16,
    pub h: u16,
    pub cells: Vec<Cell>,
}

impl Buffer {
    pub fn new(w: u16, h: u16, fill: Style) -> Buffer {
        Buffer { w, h, cells: vec![Cell { ch: ' ', st: fill }; w as usize * h as usize] }
    }

    #[inline]
    fn idx(&self, x: u16, y: u16) -> usize {
        y as usize * self.w as usize + x as usize
    }

    pub fn get(&self, x: u16, y: u16) -> &Cell {
        &self.cells[self.idx(x, y)]
    }

    pub fn get_mut(&mut self, x: u16, y: u16) -> &mut Cell {
        let i = self.idx(x, y);
        &mut self.cells[i]
    }

    pub fn clear(&mut self, st: Style) {
        for c in &mut self.cells {
            *c = Cell { ch: ' ', st };
        }
    }

    pub fn set(&mut self, x: u16, y: u16, ch: char, st: Style) {
        if x < self.w && y < self.h {
            let i = self.idx(x, y);
            self.cells[i] = Cell { ch, st };
        }
    }

    /// Print `s` at (x, y), clipped to `max_w` columns. Returns columns used.
    pub fn put(&mut self, x: u16, y: u16, s: &str, st: Style, max_w: usize) -> usize {
        let mut used = 0usize;
        for ch in s.chars() {
            let w = ch.width().unwrap_or(0);
            if w == 0 {
                continue;
            }
            if used + w > max_w || x as usize + used + w > self.w as usize {
                break;
            }
            self.set(x + used as u16, y, ch, st);
            if w == 2 {
                self.set(x + used as u16 + 1, y, '\0', st);
            }
            used += w;
        }
        used
    }

    pub fn fill(&mut self, x: u16, y: u16, w: u16, h: u16, ch: char, st: Style) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.set(xx, yy, ch, st);
            }
        }
    }

    /// Plain-text dump (for tests and `--dump`).
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for y in 0..self.h {
            let mut line = String::new();
            for x in 0..self.w {
                let c = self.get(x, y).ch;
                if c != '\0' {
                    line.push(c);
                }
            }
            s.push_str(line.trim_end());
            s.push('\n');
        }
        s
    }
}

pub struct Terminal {
    prev: Option<Buffer>,
    pub truecolor: bool,
}

impl Terminal {
    pub fn new() -> Terminal {
        Terminal { prev: None, truecolor: detect_truecolor() }
    }

    pub fn invalidate(&mut self) {
        self.prev = None;
    }

    fn color(&self, c: Rgb) -> Color {
        if self.truecolor {
            Color::Rgb { r: c.0, g: c.1, b: c.2 }
        } else {
            Color::AnsiValue(to_ansi256(c))
        }
    }

    pub fn flush(&mut self, buf: &Buffer, out: &mut impl Write) -> io::Result<()> {
        let full = self.prev.as_ref().is_none_or(|p| p.w != buf.w || p.h != buf.h);
        let mut cur: Option<(Rgb, Rgb, bool, bool)> = None;
        let mut next_pos: Option<(u16, u16)> = None;
        queue!(out, SetAttribute(Attribute::Reset))?;
        for y in 0..buf.h {
            for x in 0..buf.w {
                let c = buf.get(x, y);
                if c.ch == '\0' {
                    continue;
                }
                if !full {
                    let p = self.prev.as_ref().unwrap().get(x, y);
                    if p == c {
                        continue;
                    }
                }
                if next_pos != Some((x, y)) {
                    queue!(out, MoveTo(x, y))?;
                }
                let want = (c.st.fg, c.st.bg, c.st.bold, c.st.ul);
                if cur != Some(want) {
                    if cur.is_none_or(|k| k.2 != want.2 || k.3 != want.3) {
                        queue!(
                            out,
                            SetAttribute(Attribute::Reset),
                            SetAttribute(if want.2 { Attribute::Bold } else { Attribute::NormalIntensity }),
                            SetAttribute(if want.3 { Attribute::Underlined } else { Attribute::NoUnderline })
                        )?;
                        cur = None;
                    }
                    if cur.is_none_or(|k| k.0 != want.0) {
                        queue!(out, SetForegroundColor(self.color(want.0)))?;
                    }
                    if cur.is_none_or(|k| k.1 != want.1) {
                        queue!(out, SetBackgroundColor(self.color(want.1)))?;
                    }
                    cur = Some(want);
                }
                queue!(out, Print(c.ch))?;
                let adv = if c.ch.width() == Some(2) { 2 } else { 1 };
                next_pos = Some((x + adv, y));
            }
        }
        queue!(out, SetAttribute(Attribute::Reset))?;
        out.flush()?;
        self.prev = Some(buf.clone());
        Ok(())
    }
}

pub fn detect_truecolor() -> bool {
    let ct = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
    ct.contains("truecolor") || ct.contains("24bit") || std::env::var("PHRACK_TRUECOLOR").is_ok()
}

fn to_ansi256(c: Rgb) -> u8 {
    let (r, g, b) = (c.0 as i32, c.1 as i32, c.2 as i32);
    let q = |v: i32| -> i32 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            ((v - 35) / 40).min(5)
        }
    };
    let (qr, qg, qb) = (q(r), q(g), q(b));
    let lv = |i: i32| if i == 0 { 0 } else { 55 + 40 * i };
    let cube = (lv(qr), lv(qg), lv(qb));
    let cube_idx = 16 + 36 * qr + 6 * qg + qb;
    let avg = (r + g + b) / 3;
    let gi = ((avg - 8).max(0) / 10).min(23);
    let gv = 8 + 10 * gi;
    let d = |a: (i32, i32, i32)| (a.0 - r).pow(2) + (a.1 - g).pow(2) + (a.2 - b).pow(2);
    if d((gv, gv, gv)) < d(cube) {
        (232 + gi) as u8
    } else {
        cube_idx as u8
    }
}
