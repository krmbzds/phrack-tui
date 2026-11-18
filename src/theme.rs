//! Colour palettes. The default `crt` theme is a P1-phosphor green whose
//! accents are lifted from the dracula-derived scheme in the nvim config
//! (`#8AFF80` green, `#FFFF80` yellow, `#80FFEA` cyan, `#FF80BF` pink ...).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn scale(self, f: f32) -> Rgb {
        let s = |c: u8| ((c as f32) * f).round().clamp(0.0, 255.0) as u8;
        Rgb(s(self.0), s(self.1), s(self.2))
    }

    pub fn mix(self, o: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| ((a as f32) * (1.0 - t) + (b as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2))
    }

    pub fn add(self, o: Rgb) -> Rgb {
        Rgb(self.0.saturating_add(o.0), self.1.saturating_add(o.1), self.2.saturating_add(o.2))
    }

    pub fn lum(self) -> f32 {
        (0.2126 * self.0 as f32 + 0.7152 * self.1 as f32 + 0.0722 * self.2 as f32) / 255.0
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    /// Phosphor designation shown on the boot screen.
    pub phosphor: &'static str,
    /// Flat background colour (cells with this bg get the CRT bulge gradient).
    pub bg: Rgb,
    /// Background in the middle of the "tube".
    pub bg_center: Rgb,
    pub fg: Rgb,
    pub bright: Rgb,
    pub dim: Rgb,
    pub faint: Rgb,
    pub heading: Rgb,
    pub link: Rgb,
    pub mark: Rgb,
    pub warn: Rgb,
    pub err: Rgb,
    pub cursorline: Rgb,
    pub select: Rgb,
    pub search_bg: Rgb,
    pub search_fg: Rgb,
    pub cur_bg: Rgb,
    pub cur_fg: Rgb,
    pub border: Rgb,
    pub status_bg: Rgb,
    pub status_fg: Rgb,
    pub glow: Rgb,
    /// Background for text covered by a saved snippet (must differ from `select`).
    pub snippet: Rgb,
}

pub fn all() -> Vec<Theme> {
    vec![
        Theme {
            name: "amber",
            phosphor: "P3 AMBER",
            bg: Rgb::hex(0x0b0600),
            bg_center: Rgb::hex(0x1a0f02),
            fg: Rgb::hex(0xffb000),
            bright: Rgb::hex(0xffd36b),
            dim: Rgb::hex(0xb37800),
            faint: Rgb::hex(0x6b4700),
            heading: Rgb::hex(0xffe3a0),
            link: Rgb::hex(0xffe680),
            mark: Rgb::hex(0xff9580),
            warn: Rgb::hex(0xffff99),
            err: Rgb::hex(0xff6040),
            cursorline: Rgb::hex(0x2a1a00),
            select: Rgb::hex(0x503400),
            search_bg: Rgb::hex(0xfff0b0),
            search_fg: Rgb::hex(0x1a1000),
            cur_bg: Rgb::hex(0xff9580),
            cur_fg: Rgb::hex(0x200800),
            border: Rgb::hex(0x8a5e00),
            status_bg: Rgb::hex(0x3d2800),
            status_fg: Rgb::hex(0xffcc55),
            glow: Rgb::hex(0xffa000),
            snippet: Rgb::hex(0x123a5c),
        },
        Theme {
            name: "crt",
            phosphor: "P1 GREEN",
            bg: Rgb::hex(0x030b05),
            bg_center: Rgb::hex(0x07190c),
            fg: Rgb::hex(0x5cff6e),
            bright: Rgb::hex(0xa2ff99),
            dim: Rgb::hex(0x2fa84a),
            faint: Rgb::hex(0x17612b),
            heading: Rgb::hex(0xc8ffc0),
            link: Rgb::hex(0x80ffea),
            mark: Rgb::hex(0xff80bf),
            warn: Rgb::hex(0xffff80),
            err: Rgb::hex(0xff9580),
            cursorline: Rgb::hex(0x0c2c16),
            select: Rgb::hex(0x14502a),
            search_bg: Rgb::hex(0xffff80),
            search_fg: Rgb::hex(0x10140a),
            cur_bg: Rgb::hex(0xff80bf),
            cur_fg: Rgb::hex(0x10040a),
            border: Rgb::hex(0x1f8a3a),
            status_bg: Rgb::hex(0x0f3d1d),
            status_fg: Rgb::hex(0x8aff80),
            glow: Rgb::hex(0x2dff55),
            snippet: Rgb::hex(0x0b3f55),
        },
        // Colours taken from the nvim `colorscheme.lua` (bg, greens, comment, selection,
        // accents), with green promoted to text colour. cursorline, bg_center and
        // snippet are not in that file; they are picked to sit next to it.
        Theme {
            name: "dracula",
            phosphor: "SYNTH VIOLET",
            bg: Rgb::hex(0x22212c),
            bg_center: Rgb::hex(0x2a2937),
            fg: Rgb::hex(0x8aff80),
            bright: Rgb::hex(0xa2ff99),
            dim: Rgb::hex(0x7970a9),
            faint: Rgb::hex(0x4b5263),
            heading: Rgb::hex(0xff80bf),
            link: Rgb::hex(0x80ffea),
            mark: Rgb::hex(0xff99cc),
            warn: Rgb::hex(0xffff80),
            err: Rgb::hex(0xff9580),
            cursorline: Rgb::hex(0x2f2e3d),
            select: Rgb::hex(0x454158),
            search_bg: Rgb::hex(0xffff80),
            search_fg: Rgb::hex(0x17161d),
            cur_bg: Rgb::hex(0xff80bf),
            cur_fg: Rgb::hex(0x17161d),
            border: Rgb::hex(0x9580ff),
            status_bg: Rgb::hex(0x454158),
            status_fg: Rgb::hex(0xf8f8f2),
            glow: Rgb::hex(0x8aff80),
            snippet: Rgb::hex(0x1f4a5a),
        },
    ]
}

pub fn by_name(name: &str) -> Option<Theme> {
    all().into_iter().find(|t| t.name.eq_ignore_ascii_case(name))
}
