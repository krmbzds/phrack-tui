//! Post-processing that turns a flat terminal frame into something that looks
//! like it is being painted on phosphor: tube bulge/vignette, scanlines,
//! bloom around glyphs and the odd power-supply flicker.

use crate::{buffer::Buffer, theme::{Rgb, Theme}};

#[derive(Clone, Copy, Debug)]
pub struct Fx {
    pub scan: bool,
    pub glow: bool,
    pub vignette: bool,
    pub flicker: bool,
}

impl Default for Fx {
    fn default() -> Fx {
        Fx { scan: true, glow: true, vignette: true, flicker: true }
    }
}

impl Fx {
    pub fn none() -> Fx {
        Fx { scan: false, glow: false, vignette: false, flicker: false }
    }
}

pub fn apply(buf: &mut Buffer, th: &Theme, fx: &Fx, brightness: f32) {
    let (w, h) = (buf.w as usize, buf.h as usize);
    if w == 0 || h == 0 {
        return;
    }

    // 1. bloom: glyphs lift the background of their neighbours.
    let mut halo = vec![0f32; w * h];
    if fx.glow {
        for y in 0..h {
            for x in 0..w {
                let c = buf.get(x as u16, y as u16);
                if c.ch == ' ' || c.ch == '\0' {
                    continue;
                }
                let k = c.st.fg.lum() * if c.st.bold { 0.5 } else { 0.32 };
                halo[y * w + x] += k * 0.5;
                if x > 0 {
                    halo[y * w + x - 1] += k;
                }
                if x + 1 < w {
                    halo[y * w + x + 1] += k;
                }
                if y > 0 {
                    halo[(y - 1) * w + x] += k * 0.6;
                }
                if y + 1 < h {
                    halo[(y + 1) * w + x] += k * 0.6;
                }
            }
        }
    }
    let glow_col = th.glow.scale(0.16);

    let cx = (w as f32 - 1.0) / 2.0;
    let cy = (h as f32 - 1.0) / 2.0;
    for y in 0..h {
        for x in 0..w {
            // distance from the tube centre, 0 at centre .. ~1 in the corners
            let dx = (x as f32 - cx) / cx.max(1.0);
            let dy = (y as f32 - cy) / cy.max(1.0);
            let r2 = ((dx * dx + dy * dy) / 2.0).min(1.0);
            let vig = if fx.vignette { 1.0 - 0.34 * r2 * r2.sqrt().max(0.3) } else { 1.0 };
            let scan = if fx.scan && y % 2 == 1 { 0.91 } else { 1.0 };

            let cell = buf.get_mut(x as u16, y as u16);
            let mut bg = cell.st.bg;
            if bg == th.bg {
                if fx.vignette {
                    bg = th.bg_center.mix(th.bg, (r2 * 1.4).min(1.0));
                }
                let g = halo[y * w + x].min(1.0);
                if g > 0.0 {
                    bg = bg.add(glow_col.scale(g));
                }
            }
            let f = vig * scan * brightness;
            cell.st.bg = bg.scale(vig * (if scan < 1.0 { 0.93 } else { 1.0 }) * brightness);
            cell.st.fg = Rgb::scale(cell.st.fg, f);
        }
    }
}
