//! Procedural artwork for games without their own: launcher tiles and the
//! complete Steam artwork set (portrait capsule, wide capsule, hero, logo,
//! icon). Everything is drawn from the descriptor's name, accent colour and
//! motif — no game assets are used, so nothing here infringes anyone's art.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, RadialGradient, Rect, SpreadMode, Stroke, Transform};

pub const DISPLAY_FONT: &[u8] = include_bytes!("../../../assets/fonts/RussoOne-Regular.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Launcher library tile, 920×430 (same as Steam's wide capsule).
    Tile,
    /// Steam portrait capsule 600×900.
    Capsule,
    /// Steam hero 1920×620 (no text: Steam draws the logo over it).
    Hero,
    /// Transparent logo 1280×400.
    Logo,
    /// Square icon 256×256.
    Icon,
}

impl Kind {
    pub fn size(self) -> (u32, u32) {
        match self {
            Kind::Tile => (920, 430),
            Kind::Capsule => (600, 900),
            Kind::Hero => (1920, 620),
            Kind::Logo => (1280, 400),
            Kind::Icon => (256, 256),
        }
    }
}

pub struct Spec<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub accent: &'a str,
    pub motif: &'a str,
}

/// Small deterministic RNG so every game's art is stable.
struct Rng(u64);

impl Rng {
    fn new(seed: &str) -> Rng {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in seed.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        Rng(h | 1)
    }
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 10_000) as f32 / 10_000.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

#[derive(Clone, Copy)]
struct Rgb(f32, f32, f32);

impl Rgb {
    fn parse(hex: &str) -> Rgb {
        let h = hex.trim_start_matches('#');
        let v = u32::from_str_radix(h, 16).unwrap_or(0x7c5cff);
        Rgb(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0)
    }
    fn mix(self, o: Rgb, t: f32) -> Rgb {
        Rgb(self.0 + (o.0 - self.0) * t, self.1 + (o.1 - self.1) * t, self.2 + (o.2 - self.2) * t)
    }
    fn dark(self, t: f32) -> Rgb {
        self.mix(Rgb(0.04, 0.04, 0.08), t)
    }
    fn light(self, t: f32) -> Rgb {
        self.mix(Rgb(1.0, 1.0, 1.0), t)
    }
    fn c(self, a: f32) -> Color {
        Color::from_rgba(self.0.clamp(0.0, 1.0), self.1.clamp(0.0, 1.0), self.2.clamp(0.0, 1.0), a.clamp(0.0, 1.0)).unwrap()
    }
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn rect(px: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w.max(0.5), h.max(0.5)) {
        px.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill(px: &mut Pixmap, path: Option<tiny_skia::Path>, p: &Paint) {
    if let Some(path) = path {
        px.fill_path(&path, p, FillRule::Winding, Transform::identity(), None);
    }
}

fn circle(px: &mut Pixmap, x: f32, y: f32, r: f32, c: Color) {
    fill(px, PathBuilder::from_circle(x, y, r.max(0.5)), &paint(c));
}

fn glow(px: &mut Pixmap, x: f32, y: f32, r: f32, c: Rgb, a: f32) {
    let stops = vec![GradientStop::new(0.0, c.c(a)), GradientStop::new(1.0, c.c(0.0))];
    if let Some(sh) = RadialGradient::new(Point::from_xy(x, y), Point::from_xy(x, y), r, stops, SpreadMode::Pad, Transform::identity()) {
        let mut p = Paint::default();
        p.shader = sh;
        p.anti_alias = true;
        fill(px, PathBuilder::from_circle(x, y, r), &p);
    }
}

fn background(px: &mut Pixmap, acc: Rgb, motif: &str) {
    let (w, h) = (px.width() as f32, px.height() as f32);
    let (top, mid, bottom) = match motif {
        "pipes" => (Rgb(0.36, 0.62, 0.98), Rgb(0.55, 0.78, 1.0), Rgb(0.75, 0.88, 1.0)),
        "ruins" => (Rgb(0.05, 0.06, 0.16), Rgb(0.18, 0.12, 0.2), Rgb(0.45, 0.28, 0.16)),
        "castle" => (acc.dark(0.3), Rgb(0.55, 0.3, 0.35).dark(0.3), Rgb(0.95, 0.6, 0.3).dark(0.3)),
        _ => (acc.dark(0.25), acc.dark(0.72), acc.dark(0.9)),
    };
    let stops = vec![GradientStop::new(0.0, top.c(1.0)), GradientStop::new(0.55, mid.c(1.0)), GradientStop::new(1.0, bottom.c(1.0))];
    let mut p = Paint::default();
    let end = if matches!(motif, "pipes" | "ruins" | "castle") { Point::from_xy(0.0, h) } else { Point::from_xy(w * 0.6, h) };
    p.shader = LinearGradient::new(Point::from_xy(0.0, 0.0), end, stops, SpreadMode::Pad, Transform::identity()).unwrap();
    px.fill_rect(Rect::from_xywh(0.0, 0.0, w, h).unwrap(), &p, Transform::identity(), None);
}

fn motif(px: &mut Pixmap, m: &str, acc: Rgb, rng: &mut Rng) {
    let (w, h) = (px.width() as f32, px.height() as f32);
    let u = w.min(h);
    match m {
        "blocks" => {
            let cell = u / 9.0;
            let shapes: [&[(i32, i32)]; 5] = [
                &[(0, 0), (1, 0), (2, 0), (3, 0)],
                &[(0, 0), (1, 0), (0, 1), (1, 1)],
                &[(0, 0), (1, 0), (2, 0), (1, 1)],
                &[(0, 0), (0, 1), (1, 1), (2, 1)],
                &[(1, 0), (2, 0), (0, 1), (1, 1)],
            ];
            let cols = (w / cell) as i32 + 1;
            // A settled stack at the bottom, a few pieces falling.
            for cx in 0..cols {
                let height = (rng.range(0.0, 3.5)) as i32;
                for cy in 0..height {
                    let hue = acc.mix(Rgb(rng.next(), rng.next(), rng.next()), 0.45);
                    block(px, cx as f32 * cell, h - (cy + 1) as f32 * cell, cell, hue);
                }
            }
            for _ in 0..(w / (cell * 4.0)) as usize + 2 {
                let s = shapes[(rng.next() * 5.0) as usize % 5];
                let ox = (rng.range(0.0, cols as f32 - 3.0) as i32) as f32 * cell;
                let oy = rng.range(-cell, h * 0.6);
                let hue = acc.mix(Rgb(rng.next(), rng.next(), rng.next()), 0.6).light(0.1);
                for (dx, dy) in s {
                    block(px, ox + *dx as f32 * cell, oy + *dy as f32 * cell, cell, hue);
                }
            }
        }
        "castle" => {
            glow(px, w * 0.72, h * 0.32, u * 0.55, Rgb(1.0, 0.75, 0.35), 0.55);
            for layer in 0..2 {
                let base = h * (0.72 + layer as f32 * 0.12);
                let col = acc.dark(0.6 + layer as f32 * 0.25).c(1.0);
                let mut pb = PathBuilder::new();
                pb.move_to(0.0, h);
                let mut x = 0.0;
                pb.line_to(0.0, base);
                while x < w {
                    let seg = rng.range(u * 0.08, u * 0.2);
                    let top = base - rng.range(0.0, u * 0.12) * (1.0 - layer as f32 * 0.5);
                    pb.line_to(x, top);
                    // battlements
                    let n = (seg / (u * 0.025)) as usize;
                    for i in 0..n {
                        let bx = x + i as f32 * seg / n as f32;
                        let d = if i % 2 == 0 { u * 0.02 } else { 0.0 };
                        pb.line_to(bx, top - d);
                        pb.line_to(bx + seg / n as f32, top - d);
                    }
                    x += seg;
                    pb.line_to(x, top);
                    if rng.next() > 0.6 {
                        // tower with roof
                        let tw = u * 0.06;
                        let th = rng.range(u * 0.12, u * 0.28);
                        pb.line_to(x, top - th);
                        pb.line_to(x + tw / 2.0, top - th - tw);
                        pb.line_to(x + tw, top - th);
                        pb.line_to(x + tw, top);
                        x += tw;
                    }
                }
                pb.line_to(w, h);
                pb.close();
                fill(px, pb.finish(), &paint(col));
            }
        }
        "crypt" => {
            glow(px, w * 0.5, h * 0.95, u * 0.9, Rgb(0.9, 0.2, 0.1), 0.6);
            let arches = (w / (u * 0.38)) as usize + 1;
            let aw = w / arches as f32;
            for i in 0..arches {
                let x = i as f32 * aw;
                let mut pb = PathBuilder::new();
                pb.move_to(x + aw * 0.15, h);
                pb.line_to(x + aw * 0.15, h * 0.45);
                pb.quad_to(x + aw * 0.5, h * 0.05, x + aw * 0.85, h * 0.45);
                pb.line_to(x + aw * 0.85, h);
                pb.close();
                fill(px, pb.finish(), &paint(Rgb(0.0, 0.0, 0.0).c(0.35)));
                if let Some(p) = {
                    let mut pb = PathBuilder::new();
                    pb.move_to(x + aw * 0.15, h);
                    pb.line_to(x + aw * 0.15, h * 0.45);
                    pb.quad_to(x + aw * 0.5, h * 0.05, x + aw * 0.85, h * 0.45);
                    pb.line_to(x + aw * 0.85, h);
                    pb.finish()
                } {
                    px.stroke_path(&p, &paint(acc.light(0.2).c(0.35)), &Stroke { width: u * 0.012, ..Default::default() }, Transform::identity(), None);
                }
                // candles
                let cx = x + aw * 0.5;
                rect(px, cx - u * 0.008, h * 0.78, u * 0.016, u * 0.08, Rgb(0.9, 0.85, 0.7).c(0.9));
                glow(px, cx, h * 0.77, u * 0.06, Rgb(1.0, 0.7, 0.3), 0.8);
            }
        }
        "pipes" => {
            // Sky bands, round hills, tubes and clouds: a generic platformer.
            glow(px, w * 0.2, h * 0.15, u * 0.5, Rgb(1.0, 1.0, 0.9), 0.25);
            for i in 0..3 {
                let cx = rng.range(0.0, w);
                let r = rng.range(u * 0.3, u * 0.6);
                circle(px, cx, h + r * 0.35 - i as f32 * u * 0.02, r, Rgb(0.25, 0.7, 0.35).dark(0.1 + i as f32 * 0.12).c(1.0));
            }
            for _ in 0..4 {
                let cx = rng.range(0.0, w);
                let cy = rng.range(h * 0.08, h * 0.4);
                let r = u * rng.range(0.04, 0.07);
                for k in 0..3 {
                    circle(px, cx + k as f32 * r * 0.9, cy - if k == 1 { r * 0.4 } else { 0.0 }, r, Rgb(1.0, 1.0, 1.0).c(0.85));
                }
            }
            let n = (w / (u * 0.35)) as usize + 1;
            for i in 0..n {
                let pw = u * 0.12;
                let x = (i as f32 + 0.3 + rng.range(-0.15, 0.15)) * w / n as f32;
                let ph = rng.range(u * 0.12, u * 0.3);
                let green = Rgb(0.15, 0.7, 0.35);
                fill(px, round_rect(x, h - ph, pw, ph + 4.0, 3.0), &paint(green.dark(0.15).c(1.0)));
                fill(px, round_rect(x - pw * 0.12, h - ph - u * 0.05, pw * 1.24, u * 0.06, 4.0), &paint(green.c(1.0)));
                rect(px, x + pw * 0.15, h - ph, pw * 0.12, ph, green.light(0.35).c(0.6));
            }
        }
        "tanks" => {
            let r = u * 0.06;
            let dx = r * 1.732;
            let mut y = 0.0;
            let mut row = 0;
            while y < h + r {
                let mut x = if row % 2 == 0 { 0.0 } else { dx / 2.0 };
                while x < w + r {
                    if let Some(p) = hex_path(x, y, r * 0.95) {
                        px.stroke_path(&p, &paint(acc.light(0.3).c(0.08)), &Stroke { width: 1.5, ..Default::default() }, Transform::identity(), None);
                    }
                    x += dx;
                }
                y += r * 1.5;
                row += 1;
            }
            for _ in 0..7 {
                let cx = rng.range(0.0, w);
                let cy = rng.range(h * 0.55, h);
                glow(px, cx, cy, u * 0.12, Rgb(0.3, 1.0, 0.3), 0.35);
                for k in 0..4 {
                    let s = u * rng.range(0.02, 0.05);
                    let ox = rng.range(-s, s);
                    let mut pb = PathBuilder::new();
                    pb.move_to(cx + ox, cy - s * 2.2 - k as f32);
                    pb.line_to(cx + ox + s * 0.6, cy);
                    pb.line_to(cx + ox - s * 0.6, cy);
                    pb.close();
                    fill(px, pb.finish(), &paint(Rgb(0.4, 1.0, 0.45).c(0.85)));
                }
            }
        }
        "arena" => {
            glow(px, w * 0.5, h * 0.4, u * 0.7, acc.light(0.3), 0.45);
            for _ in 0..5 {
                let pw = rng.range(u * 0.25, u * 0.6);
                let x = rng.range(-u * 0.1, w - pw * 0.5);
                let y = rng.range(h * 0.35, h * 0.9);
                fill(px, round_rect(x, y, pw, u * 0.045, u * 0.02), &paint(acc.dark(0.55).c(0.95)));
                rect(px, x + 6.0, y, pw - 12.0, u * 0.008, acc.light(0.5).c(0.5));
            }
            // Sparks and a burst.
            let (bx, by) = (w * rng.range(0.55, 0.8), h * rng.range(0.2, 0.45));
            for i in 0..14 {
                let a = i as f32 / 14.0 * std::f32::consts::TAU + rng.range(-0.1, 0.1);
                let len = u * rng.range(0.1, 0.22);
                let mut pb = PathBuilder::new();
                pb.move_to(bx + a.cos() * u * 0.04, by + a.sin() * u * 0.04);
                pb.line_to(bx + a.cos() * len, by + a.sin() * len);
                if let Some(p) = pb.finish() {
                    px.stroke_path(&p, &paint(Rgb(1.0, 0.9, 0.6).c(0.7)), &Stroke { width: u * 0.008, line_cap: tiny_skia::LineCap::Round, ..Default::default() }, Transform::identity(), None);
                }
            }
            glow(px, bx, by, u * 0.08, Rgb(1.0, 1.0, 0.8), 0.9);
            for _ in 0..3 {
                let (cx, cy) = (rng.range(0.0, w), rng.range(0.0, h * 0.6));
                let hue = Rgb(rng.next(), rng.next() * 0.6, rng.next()).light(0.3);
                glow(px, cx, cy, u * 0.05, hue, 0.8);
            }
            // Snow: it is the Christmas edition.
            for _ in 0..140 {
                let r = u * rng.range(0.003, 0.009);
                circle(px, rng.range(0.0, w), rng.range(0.0, h), r, Rgb(1.0, 1.0, 1.0).c(rng.range(0.35, 0.9)));
            }
        }
        "ruins" => {
            // Moonlit desert with broken columns and an obelisk.
            let (mx, my) = (w * 0.74, h * 0.28);
            glow(px, mx, my, u * 0.35, Rgb(0.8, 0.85, 1.0), 0.45);
            circle(px, mx, my, u * 0.09, Rgb(0.92, 0.94, 1.0).c(0.95));
            for _ in 0..80 {
                circle(px, rng.range(0.0, w), rng.range(0.0, h * 0.5), u * rng.range(0.001, 0.004), Rgb(1.0, 1.0, 1.0).c(rng.range(0.3, 0.9)));
            }
            for k in 0..2 {
                let mut pb = PathBuilder::new();
                let base = h * (0.72 + k as f32 * 0.1);
                pb.move_to(0.0, h);
                let mut x = 0.0;
                while x <= w {
                    pb.line_to(x, base + (x / w * 6.0 + k as f32).sin() * u * 0.04);
                    x += 10.0;
                }
                pb.line_to(w, h);
                pb.close();
                fill(px, pb.finish(), &paint(Rgb(0.55, 0.35, 0.18).dark(0.45 + k as f32 * 0.2).c(1.0)));
            }
            for _ in 0..6 {
                let cw = u * rng.range(0.03, 0.05);
                let ch = u * rng.range(0.08, 0.3);
                let x = rng.range(0.0, w - cw);
                let y = h * rng.range(0.7, 0.8);
                rect(px, x, y - ch, cw, ch, Rgb(0.2, 0.13, 0.12).c(0.95));
                rect(px, x - cw * 0.2, y - ch - u * 0.012, cw * 1.4, u * 0.012, Rgb(0.25, 0.16, 0.14).c(0.95));
            }
            let ox = w * rng.range(0.15, 0.35);
            let mut pb = PathBuilder::new();
            pb.move_to(ox, h * 0.82);
            pb.line_to(ox + u * 0.035, h * 0.3);
            pb.line_to(ox + u * 0.055, h * 0.27);
            pb.line_to(ox + u * 0.075, h * 0.3);
            pb.line_to(ox + u * 0.11, h * 0.82);
            pb.close();
            fill(px, pb.finish(), &paint(Rgb(0.12, 0.08, 0.1).c(1.0)));
            glow(px, ox + u * 0.055, h * 0.45, u * 0.05, Rgb(0.9, 0.2, 0.15), 0.8);
        }
        _ => {
            // Waves: the Warpzone default.
            for k in 0..6 {
                let mut pb = PathBuilder::new();
                let base = h * (0.35 + k as f32 * 0.11);
                let amp = u * rng.range(0.03, 0.08);
                let freq = rng.range(1.0, 2.5) * std::f32::consts::TAU / w;
                let phase = rng.range(0.0, 6.28);
                pb.move_to(0.0, h);
                let mut x = 0.0;
                while x <= w {
                    pb.line_to(x, base + (x * freq + phase).sin() * amp);
                    x += 8.0;
                }
                pb.line_to(w, h);
                pb.close();
                fill(px, pb.finish(), &paint(acc.light(0.1).dark(0.2 + k as f32 * 0.12).c(0.35)));
            }
            glow(px, w * 0.75, h * 0.3, u * 0.4, acc.light(0.5), 0.4);
        }
    }
}

fn hex_path(cx: f32, cy: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for i in 0..6 {
        let a = std::f32::consts::FRAC_PI_6 + i as f32 * std::f32::consts::FRAC_PI_3;
        let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    pb.close();
    pb.finish()
}

fn block(px: &mut Pixmap, x: f32, y: f32, s: f32, c: Rgb) {
    let g = s * 0.06;
    fill(px, round_rect(x + g, y + g, s - 2.0 * g, s - 2.0 * g, s * 0.12), &paint(c.c(0.9)));
    rect(px, x + s * 0.18, y + s * 0.16, s * 0.64, s * 0.12, c.light(0.45).c(0.6));
    rect(px, x + s * 0.18, y + s * 0.72, s * 0.64, s * 0.08, c.dark(0.4).c(0.6));
}

/// Draw `text` with its baseline at (x, y); returns the advance width.
fn text(px: &mut Pixmap, font: &FontRef, size: f32, x: f32, y: f32, s: &str, c: Rgb, alpha: f32) -> f32 {
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let mut caret = x;
    let mut prev = None;
    let (pw, ph) = (px.width() as i32, px.height() as i32);
    let data = px.data_mut();
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            caret += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(scale, ab_glyph::point(caret, y));
        caret += sf.h_advance(id);
        prev = Some(id);
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, cov| {
                let (ix, iy) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if ix < 0 || iy < 0 || ix >= pw || iy >= ph {
                    return;
                }
                let i = ((iy * pw + ix) * 4) as usize;
                let a = (cov * alpha).clamp(0.0, 1.0);
                // premultiplied "over"
                let src = [c.0 * a, c.1 * a, c.2 * a, a];
                for k in 0..4 {
                    let dst = data[i + k] as f32 / 255.0;
                    data[i + k] = ((src[k] + dst * (1.0 - a)) * 255.0).round() as u8;
                }
            });
        }
    }
    caret - x
}

fn text_width(font: &FontRef, size: f32, s: &str) -> f32 {
    let sf = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

/// Break `s` into lines no wider than `max` at `size`.
fn wrap(font: &FontRef, size: f32, s: &str, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if text_width(font, size, &cand) > max && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = cand;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn title_block(px: &mut Pixmap, font: &FontRef, name: &str, max_w: f32, max_size: f32, anchor_x: f32, bottom: f32, center: bool) {
    let mut size = max_size;
    let mut lines = wrap(font, size, name, max_w);
    while (lines.len() > 3 || lines.iter().any(|l| text_width(font, size, l) > max_w)) && size > 12.0 {
        size *= 0.92;
        lines = wrap(font, size, name, max_w);
    }
    let lh = size * 1.05;
    let mut y = bottom - lh * (lines.len() as f32 - 1.0);
    for l in &lines {
        let tw = text_width(font, size, l);
        let x = if center { anchor_x - tw / 2.0 } else { anchor_x };
        let sh = (size * 0.06).max(2.0);
        text(px, font, size, x + sh, y + sh, l, Rgb(0.0, 0.0, 0.0), 0.55);
        text(px, font, size, x, y, l, Rgb(1.0, 1.0, 1.0), 1.0);
        y += lh;
    }
}

fn shade_bottom(px: &mut Pixmap, from: f32) {
    let (w, h) = (px.width() as f32, px.height() as f32);
    let stops = vec![GradientStop::new(0.0, Color::from_rgba8(0, 0, 0, 0)), GradientStop::new(1.0, Color::from_rgba8(6, 6, 14, 200))];
    let mut p = Paint::default();
    p.shader = LinearGradient::new(Point::from_xy(0.0, h * from), Point::from_xy(0.0, h), stops, SpreadMode::Pad, Transform::identity()).unwrap();
    px.fill_rect(Rect::from_xywh(0.0, h * from, w, h * (1.0 - from)).unwrap(), &p, Transform::identity(), None);
}

/// Render artwork; returns straight (non-premultiplied) RGBA8.
pub fn render(spec: &Spec, kind: Kind) -> (u32, u32, Vec<u8>) {
    let (w, h) = kind.size();
    let mut px = Pixmap::new(w, h).unwrap();
    let font = FontRef::try_from_slice(DISPLAY_FONT).expect("bundled font");
    let acc = Rgb::parse(spec.accent);
    let mut rng = Rng::new(spec.id);
    let (wf, hf) = (w as f32, h as f32);
    match kind {
        Kind::Logo => {
            title_block(&mut px, &font, spec.name, wf * 0.92, hf * 0.42, wf / 2.0, hf * 0.62, true);
        }
        Kind::Icon => {
            fill(&mut px, round_rect(8.0, 8.0, wf - 16.0, hf - 16.0, wf * 0.22), &paint(acc.dark(0.45).c(1.0)));
            glow(&mut px, wf * 0.7, hf * 0.3, wf * 0.6, acc.light(0.3), 0.6);
            let initials: String = spec.name.split_whitespace().filter(|w| w.chars().next().is_some_and(|c| c.is_alphanumeric())).filter_map(|w| w.chars().next()).take(2).collect();
            let size = hf * 0.46;
            let tw = text_width(&font, size, &initials);
            text(&mut px, &font, size, (wf - tw) / 2.0 + 4.0, hf * 0.66 + 4.0, &initials, Rgb(0.0, 0.0, 0.0), 0.5);
            text(&mut px, &font, size, (wf - tw) / 2.0, hf * 0.66, &initials, Rgb(1.0, 1.0, 1.0), 1.0);
        }
        _ => {
            background(&mut px, acc, spec.motif);
            motif(&mut px, spec.motif, acc, &mut rng);
            match kind {
                Kind::Tile => {
                    shade_bottom(&mut px, 0.45);
                    title_block(&mut px, &font, spec.name, wf * 0.86, hf * 0.2, wf * 0.06, hf * 0.86, false);
                }
                Kind::Capsule => {
                    shade_bottom(&mut px, 0.4);
                    title_block(&mut px, &font, spec.name, wf * 0.86, hf * 0.12, wf / 2.0, hf * 0.86, true);
                }
                _ => {}
            }
        }
    }
    // Un-premultiply for image encoders and egui.
    let mut out = px.take();
    for p in out.chunks_exact_mut(4) {
        let a = p[3] as f32 / 255.0;
        if a > 0.0 && a < 1.0 {
            for c in &mut p[..3] {
                *c = ((*c as f32 / a).min(255.0)) as u8;
            }
        }
    }
    (w, h, out)
}

pub fn save_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    image::save_buffer(path, rgba, w, h, image::ColorType::Rgba8).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_kind_and_motif() {
        for m in ["arena", "blocks", "castle", "crypt", "pipes", "tanks", "waves"] {
            for k in [Kind::Tile, Kind::Capsule, Kind::Hero, Kind::Logo, Kind::Icon] {
                let (w, h, px) = render(&Spec { id: m, name: "The Settlers II: Coop", accent: "#3b82f6", motif: m }, k);
                assert_eq!(px.len(), (w * h * 4) as usize);
            }
        }
    }
}
