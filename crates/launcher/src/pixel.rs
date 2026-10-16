//! Custom pixel-art scenes, one per game: a typical in-game moment redrawn
//! from scratch on a small canvas and scaled up without smoothing. Every
//! sprite here is original — no game's art, characters or logos are used
//! (the Super Mario Land scene, for example, has its own little explorer in
//! the Game Boy's four greens, not Nintendo's characters).
//!
//! A scene draws into any canvas size (tiles, Steam capsules and heroes have
//! different shapes), so positions are relative to the canvas.

type C = [u8; 3];

pub struct Px {
    pub w: i32,
    pub h: i32,
    d: Vec<C>,
}

/// Deterministic little RNG (same art on every machine).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: &str) -> Rng {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in seed.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        Rng(h | 1)
    }
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn int(&mut self, a: i32, b: i32) -> i32 {
        if b <= a { a } else { a + (self.next() % (b - a) as u32) as i32 }
    }
    fn chance(&mut self, p: u32) -> bool {
        self.next() % 100 < p
    }
}

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

fn mix(a: C, b: C, t: f32) -> C {
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

impl Px {
    pub fn new(w: i32, h: i32) -> Px {
        Px { w, h, d: vec![[0, 0, 0]; (w * h) as usize] }
    }
    fn set(&mut self, x: i32, y: i32, c: C) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.d[(y * self.w + x) as usize] = c;
        }
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: C) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }
    /// Vertical gradient with ordered dithering between `steps` colours.
    fn sky(&mut self, y0: i32, y1: i32, stops: &[C]) {
        let n = (stops.len() - 1) as f32;
        for y in y0..y1 {
            let t = (y - y0) as f32 / (y1 - y0).max(1) as f32 * n;
            let i = (t.floor() as usize).min(stops.len() - 2);
            let f = t - i as f32;
            for x in 0..self.w {
                let thr = BAYER[(y & 3) as usize][(x & 3) as usize] as f32 / 16.0;
                self.set(x, y, if f > thr { stops[i + 1] } else { stops[i] });
            }
        }
    }
    fn disc(&mut self, cx: i32, cy: i32, r: i32, c: C) {
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y <= r * r + r / 2 {
                    self.set(cx + x, cy + y, c);
                }
            }
        }
    }
    /// A sprite from text rows (`.` transparent, other chars index `pal`),
    /// each sprite pixel drawn as a `k`×`k` block.
    fn sprite_k(&mut self, x: i32, y: i32, rows: &[&str], pal: &[(char, C)], flip: bool, k: i32) {
        for (j, row) in rows.iter().enumerate() {
            let chars: Vec<char> = row.chars().collect();
            let w = chars.len() as i32;
            for (i, ch) in chars.iter().enumerate() {
                if *ch == '.' {
                    continue;
                }
                if let Some((_, c)) = pal.iter().find(|(kk, _)| kk == ch) {
                    let xi = if flip { w - 1 - i as i32 } else { i as i32 };
                    self.rect(x + xi * k, y + j as i32 * k, k, k, *c);
                }
            }
        }
    }

    /// Blend a soft glow (additive-ish) around a point.
    fn glow(&mut self, cx: i32, cy: i32, r: i32, c: C, strength: f32) {
        for y in -r..=r {
            for x in -r..=r {
                let d = ((x * x + y * y) as f32).sqrt() / r as f32;
                if d > 1.0 {
                    continue;
                }
                let (px, py) = (cx + x, cy + y);
                if px < 0 || py < 0 || px >= self.w || py >= self.h {
                    continue;
                }
                let thr = BAYER[(py & 3) as usize][(px & 3) as usize] as f32 / 16.0;
                let a = (1.0 - d) * strength;
                if a > thr * 0.9 {
                    let i = (py * self.w + px) as usize;
                    self.d[i] = mix(self.d[i], c, (a * 0.7).min(0.85));
                }
            }
        }
    }

    /// Nearest-neighbour upscale to RGBA.
    pub fn upscale(&self, s: i32) -> (u32, u32, Vec<u8>) {
        let (w, h) = (self.w * s, self.h * s);
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = self.d[((y / s) * self.w + x / s) as usize];
                out.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        (w as u32, h as u32, out)
    }
}

/// Draw the scene for `id` if there is one.
pub fn scene(id: &str, w: i32, h: i32) -> Option<Px> {
    let mut p = Px::new(w, h);
    let mut r = Rng::new(id);
    match id {
        "funny-furries" => furries(&mut p, &mut r),
        "super-mario-land-port" => gameboy_desert(&mut p, &mut r),
        "gtetrinet-ng" => tetrinet(&mut p, &mut r),
        "s25coop" => settlers(&mut p, &mut r),
        "opents-coop" => tiberium(&mut p, &mut r),
        "diablo-splitscreen" => cathedral(&mut p, &mut r),
        "diablo2-couch" => desert_town(&mut p, &mut r),
        "drmario64-vs" => pill_bottles(&mut p, &mut r),
        "spacehog" => space_shooter(&mut p, &mut r),
        _ => return None,
    }
    Some(p)
}

// Shared layout: the action sits in the upper ~62 % (the launcher draws the
// title over the bottom), characters are drawn at 2× (3× on tall canvases).
fn layout(p: &Px) -> (i32, i32) {
    (if p.h >= 150 { 3 } else { 2 }, p.h * 62 / 100)
}

// ---------------------------------------------------------------- Funny Furries

const FURRY: [&str; 11] = [
    "..o.....o..",
    ".ofo...ofo.",
    ".offooooffo",
    "offfffffffo",
    "ofwpfffwpfo",
    "offffnffffo",
    ".offbbbffo.",
    "..obbbbbo..",
    ".oofbbbfoo.",
    "of.ofoof.fo",
    "...oo..oo..",
];

fn furry_pal(fur: C, belly: C) -> Vec<(char, C)> {
    vec![('o', [24, 16, 22]), ('f', fur), ('b', belly), ('w', [255, 255, 255]), ('p', [20, 20, 30]), ('n', [240, 120, 150])]
}

fn furries(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    p.sky(0, h, &[[16, 14, 46], [42, 28, 84], [88, 48, 112], [150, 80, 110]]);
    for _ in 0..w * h / 110 {
        let (x, y) = (r.int(0, w), r.int(0, gy));
        p.set(x, y, [230, 230, 255]);
    }
    p.disc(w * 7 / 8, h / 7, 4 + k, [250, 246, 220]);
    p.disc(w * 7 / 8 + 2, h / 7 - 1, 3 + k, [30, 22, 64]);
    for x in 0..w {
        let y = gy - 8 + ((x as f32 / 13.0).sin() * 3.0 + (x as f32 / 5.0).cos() * 1.5) as i32;
        p.rect(x, y, 1, h - y, [150, 168, 210]);
        p.rect(x, y + 3, 1, h - y, [110, 126, 180]);
    }
    p.rect(0, gy, w, h - gy, [236, 242, 255]);
    p.rect(0, gy + 3, w, h - gy, [200, 212, 240]);
    // Christmas tree with lights.
    let tree = [
        ".....y.....",
        ".....g.....",
        "....ggg....",
        "...gglgg...",
        "....ggg....",
        "...ggggg...",
        "..gglgggg..",
        "...ggggg...",
        "..ggggggg..",
        ".ggggglggg.",
        "ggggggggggg",
        ".....t.....",
        ".....t.....",
    ];
    let tp = [('y', [255, 220, 60]), ('g', [26, 120, 60]), ('l', [255, 80, 80]), ('t', [96, 56, 32])];
    p.sprite_k(w / 14, gy - 13 * k, &tree, &tp, false, k);
    // Two floating platforms.
    let plat = |p: &mut Px, x: i32, y: i32, pw: i32| {
        p.rect(x, y, pw, 3, [120, 74, 40]);
        p.rect(x, y + 3, pw, 1, [80, 48, 26]);
        p.rect(x - 1, y - 1, pw + 2, 2, [244, 248, 255]);
    };
    let (p1x, p1y, p1w) = (w * 9 / 20, gy - 15 * k, w / 4);
    plat(p, p1x, p1y, p1w);
    let (p2x, p2y, p2w) = (w * 3 / 4, gy - 9 * k, w / 6);
    plat(p, p2x, p2y, p2w);
    // Gifts.
    for (i, col) in [[220, 50, 60], [60, 140, 230], [250, 190, 40]].iter().enumerate() {
        let x = w / 5 + i as i32 * 3 * k;
        p.rect(x, gy - 2 * k, 2 * k + 1, 2 * k, *col);
        p.rect(x + k, gy - 2 * k, 1, 2 * k, [255, 255, 255]);
    }
    // Furries: one on the ground punching, one on the platform, one jetpacking.
    let fx = w * 3 / 10;
    p.sprite_k(fx, gy - 11 * k, &FURRY, &furry_pal([238, 140, 40], [252, 214, 160]), false, k);
    for (dx, dy) in [(0, -2), (0, 2), (-2, 0), (2, 0), (-1, -1), (1, 1), (1, -1), (-1, 1)] {
        p.set(fx + 11 * k + 2 + dx, gy - 6 * k + dy, [255, 240, 120]);
    }
    p.sprite_k(fx + 12 * k + 3, gy - 11 * k, &FURRY, &furry_pal([90, 190, 90], [220, 246, 200]), true, k);
    p.sprite_k(p1x + p1w / 2 - 5 * k, p1y - 11 * k - 1, &FURRY, &furry_pal([150, 150, 170], [226, 226, 236]), true, k);
    let (jx, jy) = (p2x + 2, p2y - 22 * k);
    p.sprite_k(jx, jy, &FURRY, &furry_pal([220, 90, 160], [250, 210, 230]), false, k);
    p.rect(jx + 4 * k, jy + 11 * k, 3 * k / 2 + 1, 2 * k, [255, 200, 60]);
    p.rect(jx + 4 * k + 1, jy + 13 * k, k, 2 * k, [255, 110, 40]);
    // A rocket across the arena with a smoke trail.
    let (rx, ry) = (p1x - 6, p1y - 6 * k);
    for i in 0..14 {
        p.set(rx - i * 2, ry + (i % 2), [200, 200, 210]);
    }
    p.rect(rx, ry - 1, 2 * k, k + 1, [220, 220, 230]);
    p.rect(rx + 2 * k, ry, 1, 1, [255, 80, 40]);
    for _ in 0..w * h / 80 {
        p.set(r.int(0, w), r.int(0, h), [255, 255, 255]);
    }
}

// ---------------------------------------------------------------- Super Mario Land (GB greens)

fn gameboy_desert(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    let g = [[15, 56, 15], [48, 98, 48], [139, 172, 15], [155, 188, 15]];
    p.rect(0, 0, w, h, g[3]);
    p.disc(w * 4 / 5, h / 6, 3 + k * 2, g[2]);
    for (cx, s) in [(w / 7, gy * 2 / 5), (w / 3, gy / 3), (w * 3 / 5, gy * 2 / 5), (w * 9 / 10, gy / 4)] {
        for i in 0..s {
            p.rect(cx - i, gy - s + i, i * 2 + 1, 1, if i % 4 == 0 { g[1] } else { g[2] });
            p.set(cx + i, gy - s + i, g[1]);
        }
    }
    for px in [w / 12, w / 2] {
        p.rect(px, gy - 8 * k, 2, 8 * k, g[1]);
        for (dx, dy) in [(-6, 0), (-4, -1), (4, -1), (6, 0), (-5, 1), (5, 1), (0, -1), (1, -1)] {
            p.rect(px + dx, gy - 8 * k - 1 + dy, 2, 1, g[0]);
        }
    }
    // Brick ground.
    p.rect(0, gy, w, h - gy, g[1]);
    for y in (gy..h).step_by(4) {
        for x in (0..w).step_by(8) {
            p.rect(x + if (y - gy) / 4 % 2 == 0 { 0 } else { 4 }, y, 1, 4, g[0]);
        }
        p.rect(0, y, w, 1, g[0]);
    }
    // Tubes.
    for (tx, th) in [(w * 7 / 10, 7 * k), (w * 17 / 20, 11 * k)] {
        let tw = 5 * k;
        p.rect(tx, gy - th, tw, th, g[2]);
        p.rect(tx + 1, gy - th, k, th, g[3]);
        p.rect(tx + tw - k, gy - th, k, th, g[1]);
        p.rect(tx - k, gy - th - 2 * k, tw + 2 * k, 2 * k, g[1]);
        p.rect(tx - k + 1, gy - th - 2 * k + 1, tw + 2 * k - 2, 2 * k - 2, g[2]);
    }
    // Blocks and coins.
    let bs = 4 * k;
    let by = gy - 13 * k;
    for i in 0..4 {
        let bx = w / 3 + i * (bs + 1);
        p.rect(bx, by, bs, bs, g[0]);
        p.rect(bx + 1, by + 1, bs - 2, bs - 2, g[2]);
        if i == 1 {
            p.rect(bx + bs / 2 - 1, by + bs / 2 - 1, 2, 2, g[0]);
        }
    }
    for i in 0..3 {
        let (cx, cy) = (w / 3 + bs / 2 + i * (bs + 1) + bs / 2, by - 3 * k - (i % 2) * k);
        p.rect(cx, cy, k + 1, 2 * k + 1, g[0]);
        p.rect(cx + 1, cy + 1, (k - 1).max(1), (2 * k - 1).max(1), g[2]);
    }
    // Our own explorer jumping, and two beetles.
    let hero = [
        "...0000...",
        "..022220..",
        ".00000000.",
        "..033330..",
        "..030303..",
        "..033330..",
        "...0110...",
        "..011110..",
        ".03011030.",
        "...0110...",
        "..00..00..",
        ".00....00.",
    ];
    let hp = [('0', g[0]), ('1', g[1]), ('2', g[2]), ('3', g[3])];
    p.sprite_k(w / 4 - 2, by - 6 * k, &hero, &hp, false, k);
    let beetle = ["..0000..", ".011110.", "01211210", "01111110", ".0.00.0.", "0..00..0"];
    p.sprite_k(w / 2 + 4 * k, gy - 6 * k, &beetle, &hp, false, k);
    p.sprite_k(w / 2 + 14 * k, gy - 6 * k, &beetle, &hp, true, k);
    let _ = r;
}

// ---------------------------------------------------------------- GTetrinet NG

fn tetrinet(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (_, gy) = layout(p);
    p.sky(0, h, &[[14, 22, 30], [10, 30, 34], [8, 16, 22]]);
    let top = 3;
    let cell = ((gy + 6 - top) / 20).max(2);
    let fw = 10 * cell;
    let fh = 20 * cell;
    let n = ((w - 8) / (fw + 6)).clamp(1, 6);
    let x0 = (w - n * (fw + 6) + 6) / 2;
    let colors: [C; 7] = [[230, 70, 70], [240, 170, 40], [240, 230, 60], [70, 210, 90], [60, 200, 230], [80, 110, 240], [190, 90, 230]];
    for f in 0..n {
        let fx = x0 + f * (fw + 6);
        p.rect(fx - 1, top - 1, fw + 2, fh + 2, [120, 150, 160]);
        p.rect(fx, top, fw, fh, [6, 10, 14]);
        let height = r.int(6, 15);
        for row in 0..height {
            for col in 0..10 {
                if r.chance(if row == height - 1 { 45 } else { 82 }) {
                    let c = if r.chance(5) { [255, 255, 255] } else { colors[r.int(0, 7) as usize] };
                    let (cx, cy) = (fx + col * cell, top + fh - (row + 1) * cell);
                    p.rect(cx, cy, cell, cell, c);
                    p.rect(cx, cy, cell, 1, mix(c, [255, 255, 255], 0.45));
                    p.rect(cx + cell - 1, cy, 1, cell, mix(c, [0, 0, 0], 0.4));
                }
            }
        }
        let shapes: [&[(i32, i32)]; 4] =
            [&[(0, 0), (1, 0), (2, 0), (1, 1)], &[(0, 0), (1, 0), (0, 1), (1, 1)], &[(0, 0), (0, 1), (0, 2), (0, 3)], &[(1, 0), (2, 0), (0, 1), (1, 1)]];
        let sh = shapes[r.int(0, 4) as usize];
        let c = colors[r.int(0, 7) as usize];
        let (px0, py0) = (r.int(1, 7), r.int(1, 5));
        for (dx, dy) in sh {
            let (cx, cy) = (fx + (px0 + dx) * cell, top + (py0 + dy) * cell);
            p.rect(cx, cy, cell, cell, c);
            p.rect(cx, cy, cell, 1, mix(c, [255, 255, 255], 0.45));
        }
    }
    if n >= 2 {
        let (ax, bx, y) = (x0 + fw + 1, x0 + fw + 5 + fw / 2, top + fh / 3);
        for x in ax..bx {
            p.set(x, y + ((x - ax) as f32 / 4.0).sin() as i32, [255, 240, 120]);
        }
        p.glow(bx, y, 5, [255, 240, 120], 1.0);
    }
}

// ---------------------------------------------------------------- The Settlers II: Coop

fn settlers(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    let sky_h = h / 6;
    p.sky(0, sky_h, &[[110, 170, 230], [170, 210, 240]]);
    for _ in 0..3 {
        let (cx, cy) = (r.int(0, w), r.int(2, sky_h - 3));
        for i in 0..4 {
            p.disc(cx + i * 3, cy - if i % 2 == 1 { 1 } else { 0 }, 2, [250, 250, 255]);
        }
    }
    for y in sky_h..h {
        for x in 0..w {
            let t = BAYER[(y & 3) as usize][(x & 3) as usize];
            p.set(
                x,
                y,
                if t < 3 {
                    [70, 130, 40]
                } else if t < 12 {
                    [90, 156, 52]
                } else {
                    [110, 172, 62]
                },
            );
        }
    }
    for x in 0..w {
        let m = ((x as f32 / 9.0).sin() * 3.0 + (x as f32 / 3.7).cos() * 1.5) as i32;
        p.rect(x, sky_h - 4 - m.max(0), 1, 4 + m.max(0), [120, 130, 140]);
        if m > 1 {
            p.set(x, sky_h - 4 - m, [240, 240, 250]);
        }
    }
    for y in sky_h..h {
        let x = (w as f32 * 0.86 + ((y as f32) / 6.0).sin() * 4.0) as i32;
        p.rect(x, y, 2 + k * 2, 1, [60, 120, 200]);
        p.set(x + k, y, [130, 180, 240]);
    }
    let fields = [(w / 14, gy - 6 * k), (w * 3 / 5, gy - 2 * k)];
    for (fx, fy) in fields {
        for row in 0..3 + k {
            p.rect(fx, fy + row * 2, w / 7, 1, [222, 190, 70]);
            p.rect(fx, fy + row * 2 + 1, w / 7, 1, [180, 150, 50]);
        }
    }
    let hq = (w / 2, sky_h + (gy - sky_h) / 2);
    let sites = [(w / 5, sky_h + (gy - sky_h) / 3), (w * 7 / 10, sky_h + (gy - sky_h) / 4), (w / 3, gy), (w * 7 / 10, gy + k * 2)];
    for (sx, sy) in sites {
        let steps = (sx - hq.0).abs().max((sy - hq.1).abs()).max(1);
        for i in 0..=steps {
            let x = hq.0 + (sx - hq.0) * i / steps;
            let y = hq.1 + 4 * k + (sy - hq.1) * i / steps;
            p.rect(x, y, 2, 1, [170, 130, 80]);
        }
        let (mx, my) = ((hq.0 + sx) / 2, (hq.1 + 4 * k + sy) / 2);
        p.rect(mx, my - 2 * k, 1, 2 * k, [80, 60, 40]);
        p.rect(mx + 1, my - 2 * k, k + 1, k, [220, 40, 40]);
    }
    let keep = [
        "......f.....",
        "......ffbb..",
        "......f.....",
        ".....rrr....",
        "....rrrrr...",
        "s.s.ttttt.s.",
        "ssssttttts.s",
        "ssssssssssss",
        "sssddsssssss",
        "sssddsssswss",
        "sssddsssssss",
    ];
    let kp = [
        ('f', [80, 60, 40]),
        ('b', [40, 80, 220]),
        ('r', [180, 50, 50]),
        ('t', [130, 130, 140]),
        ('s', [150, 150, 162]),
        ('d', [70, 50, 40]),
        ('w', [40, 40, 60]),
    ];
    p.sprite_k(hq.0 - 6 * k, hq.1 - 7 * k, &keep, &kp, false, k);
    let house = ["...rr...", "..rrrr..", ".rrrrrr.", "rrrrrrrr", ".wwwwww.", ".wdwwww.", ".wdwwgw.", ".wwwwww."];
    let hp = [('r', [190, 60, 40]), ('w', [222, 200, 160]), ('d', [110, 70, 40]), ('g', [140, 200, 230])];
    for (i, (sx, sy)) in sites.iter().enumerate() {
        p.sprite_k(sx - 4 * k, sy - 6 * k, &house, &hp, i % 2 == 1, k);
    }
    for _ in 0..(w / 9) {
        let (tx, ty) = (r.int(0, w), r.int(sky_h + 2, h - 4));
        if (tx - hq.0).abs() < 9 * k && (ty - hq.1).abs() < 9 * k {
            continue;
        }
        p.rect(tx, ty, 1, 2 + k, [90, 60, 30]);
        p.disc(tx, ty - 2, 1 + k / 2 + 1, [40, 100, 40]);
        p.set(tx - 1, ty - 3, [70, 140, 60]);
    }
    let carrier = [".c.", "fff", ".s.", ".s.", "l.l"];
    for (i, (sx, sy)) in sites.iter().enumerate() {
        let (cx, cy) = ((hq.0 * 2 + sx) / 3, (hq.1 + 4 * k) * 2 / 3 + sy / 3);
        let ware = [[200, 140, 60], [160, 160, 170], [230, 210, 120], [140, 90, 50]][i % 4];
        p.sprite_k(cx, cy - 5 * k, &carrier, &[('c', ware), ('f', [240, 200, 160]), ('s', [60, 90, 180]), ('l', [60, 40, 30])], false, k);
    }
}

// ---------------------------------------------------------------- OpenTS Co-op (Tiberian Sun)

fn tiberium(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    let horizon = gy * 3 / 5;
    p.sky(0, horizon, &[[12, 10, 28], [40, 24, 60], [90, 50, 70]]);
    let mut x = w * 2 / 5;
    for y in 0..horizon * 2 / 3 {
        x += r.int(-1, 2);
        p.set(x, y, [200, 220, 255]);
    }
    p.glow(x, horizon * 2 / 3, 10, [150, 170, 255], 0.6);
    for y in horizon..h {
        for xx in 0..w {
            let t = BAYER[(y & 3) as usize][(xx & 3) as usize];
            p.set(
                xx,
                y,
                if t < 5 {
                    [44, 34, 30]
                } else if t < 13 {
                    [62, 48, 38]
                } else {
                    [80, 62, 46]
                },
            );
        }
    }
    for xx in 0..w {
        let y = horizon + ((xx as f32 / 7.0).sin() * 2.0) as i32;
        p.rect(xx, y - 3, 1, 3, [70, 56, 50]);
    }
    for _ in 0..(w / 7) {
        let (cx, cy) = (r.int(0, w), r.int(horizon + 4, h));
        p.glow(cx, cy, 6, [80, 255, 90], 0.5);
        for _ in 0..4 {
            let (dx, ht) = (r.int(-3, 4), r.int(2, 3 + 2 * k));
            for i in 0..ht {
                p.set(cx + dx, cy - i, if i == ht - 1 { [200, 255, 200] } else { [60, 220, 80] });
                if i < ht / 2 {
                    p.set(cx + dx + 1, cy - i, [30, 150, 50]);
                }
            }
        }
    }
    let mech = ["...dddd...", "..dbbbbd..", "dddbyybddd", "dmdbbbbdmd", ".d.dddd.d.", "...d..d...", "..dd..dd..", ".dd....dd."];
    let mp = [('d', [40, 44, 50]), ('b', [170, 160, 120]), ('y', [255, 200, 60]), ('m', [120, 120, 130])];
    let (mx, my) = (w * 3 / 5, gy - 8 * k);
    p.sprite_k(mx, my, &mech, &mp, false, k);
    let tank = ["....dddd.....", "...dbbbbdddd.", ".ddbbbbbbbd..", "dbbbbbbbbbbd.", "ddddddddddddd", ".d.d.d.d.d.d."];
    let (tx, ty) = (w / 6, gy - 6 * k);
    p.sprite_k(tx, ty, &tank, &[('d', [40, 44, 50]), ('b', [150, 140, 110])], false, k);
    p.glow(tx + 12 * k, ty + k, 4 + k, [255, 220, 120], 1.0);
    let (sx, sy, ex, ey) = (tx + 13 * k, ty + k, mx, my + 3 * k);
    for i in 0..(ex - sx) {
        if i % 3 != 0 {
            p.set(sx + i, sy + (ey - sy) * i / (ex - sx).max(1), [255, 230, 150]);
        }
    }
    p.glow(mx, my + 3 * k, 3 + k, [255, 160, 80], 1.0);
    let harv = ["..dddd....", "..dggd....", "dddddddddd", "dbbbbbbbbd", "dbbbbbbbbd", "dddddddddd", ".d.d..d.d."];
    p.sprite_k(w * 4 / 5, gy - 7 * k, &harv, &[('d', [40, 44, 50]), ('b', [130, 120, 100]), ('g', [120, 220, 255])], true, k);
}

// ---------------------------------------------------------------- Diablo Splitscreen (cathedral)

fn cathedral(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    p.rect(0, 0, w, h, [10, 8, 10]);
    let wall = gy * 3 / 5;
    p.rect(0, 0, w, wall, [52, 44, 44]);
    for x in (0..w).step_by(6) {
        for y in (0..wall).step_by(4) {
            p.rect(x + if y / 4 % 2 == 0 { 0 } else { 3 }, y, 1, 4, [38, 32, 32]);
        }
        p.rect(0, (x / 6 % 3) * 4, 0, 0, [0, 0, 0]);
    }
    for y in (0..wall).step_by(4) {
        p.rect(0, y, w, 1, [38, 32, 32]);
    }
    let n = (w / 32).max(2);
    for i in 0..n {
        let cx = (i * 2 + 1) * w / (n * 2);
        for y in 4..wall {
            let half = if y < 10 { (y - 4) / 2 + 2 } else { 5 };
            p.rect(cx - half, y, half * 2, 1, [14, 10, 12]);
        }
        let tx = cx + 10;
        p.rect(tx, wall / 2, 1, 4, [90, 60, 30]);
        p.glow(tx, wall / 2 - 1, 12, [255, 150, 60], 0.9);
        p.rect(tx, wall / 2 - 2, 1, 2, [255, 210, 90]);
    }
    for y in wall..h {
        let depth = (y - wall) as f32 / (h - wall) as f32;
        let size = (3.0 + depth * 6.0) as i32;
        for x in 0..w {
            let even = ((x / size) + (y / size.max(1))) % 2 == 0;
            let base = if even { [74, 66, 60] } else { [58, 50, 48] };
            p.set(x, y, mix(base, [0, 0, 0], 0.45 - depth * 0.3));
        }
    }
    for i in 0..n {
        p.glow((i * 2 + 1) * w / (n * 2) + 10, gy - 2, 16, [200, 120, 60], 0.35);
    }
    let warrior = ["....hh....", "...hssh...", "...hsdh...", "..aaaaaa..", "gaaaaaaaab", "gaaaaaaa.b", "g.aaaaa..b", "..ll.ll...", "..ll.ll...", ".bb...bb.."];
    let wp = [
        ('h', [150, 150, 160]),
        ('s', [220, 180, 150]),
        ('d', [40, 30, 30]),
        ('a', [140, 34, 34]),
        ('g', [170, 140, 60]),
        ('b', [200, 200, 220]),
        ('l', [70, 60, 60]),
    ];
    p.sprite_k(w / 3, gy - 10 * k, &warrior, &wp, false, k);
    let rogue = ["...hh....", "..hssh...", "..hssh..b", ".gggggg.b", "g.gggg..b", "..gggg..b", "..l..l...", ".ll..ll.."];
    p.sprite_k(
        w / 3 - 12 * k,
        gy - 8 * k,
        &rogue,
        &[('h', [90, 60, 40]), ('s', [220, 180, 150]), ('g', [50, 90, 60]), ('b', [150, 110, 60]), ('l', [60, 50, 40])],
        false,
        k,
    );
    let skel = ["..www..", ".wdwdw.", "..www..", ".wwwww.", "w.www.w", "..w.w..", ".w...w."];
    let sp = [('w', [224, 220, 204]), ('d', [20, 10, 10])];
    p.sprite_k(w / 3 + 12 * k, gy - 7 * k, &skel, &sp, true, k);
    p.sprite_k(w / 2 + 12 * k, gy - 7 * k + 2, &skel, &sp, true, k);
    for (dx, dy) in [(0, -2), (0, 2), (-2, 0), (2, 0)] {
        p.set(w / 3 + 10 * k + dx, gy - 6 * k + dy, [255, 240, 200]);
    }
    p.rect(w * 3 / 4, gy - 2 * k, 2 * k - 1, 2 * k, [210, 30, 40]);
    p.glow(w * 3 / 4 + k, gy - k, 3 + k, [255, 60, 60], 0.6);
    let _ = r;
}

// ---------------------------------------------------------------- Diablo 2 Couch (desert town)

fn desert_town(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    let horizon = gy * 3 / 5;
    p.sky(0, horizon, &[[60, 110, 170], [180, 160, 130], [240, 200, 140]]);
    p.glow(w / 6, horizon / 3, 14, [255, 240, 200], 1.0);
    p.disc(w / 6, horizon / 3, 3 + k, [255, 250, 230]);
    for (i, col) in [[214, 170, 110], [200, 150, 90], [186, 136, 80]].iter().enumerate() {
        for x in 0..w {
            let y = horizon + i as i32 * (h - horizon) / 5 + ((x as f32 / (14.0 + i as f32 * 5.0)).sin() * 2.0) as i32;
            p.rect(x, y, 1, h - y, *col);
        }
    }
    for i in 0..(w / 44).max(2) {
        let bx = w / 3 + i * 22 + 4;
        let by = horizon + 3;
        p.rect(bx, by - 10, 16, 10, [226, 196, 150]);
        p.rect(bx, by - 10, 16, 1, [244, 220, 180]);
        p.disc(bx + 8, by - 11, 4, [80, 120, 170]);
        p.rect(bx + 3, by - 6, 3, 6, [120, 80, 50]);
        p.rect(bx + 10, by - 8, 2, 2, [60, 40, 30]);
    }
    for px in [w / 4, w * 5 / 6] {
        let base = horizon + 4;
        for i in 0..(7 * k) {
            p.set(px + i / 5, base - i, [110, 80, 50]);
        }
        for (dx, dy) in [(-5, 1), (-3, 0), (-1, -1), (1, -1), (3, 0), (5, 1), (-4, 2), (4, 2)] {
            p.rect(px + k + dx, base - 7 * k - 1 + dy, 3, 1, [50, 120, 60]);
        }
    }
    let sorc = ["...hh...", "..hssh..", "..hsss..", ".pppppp.", "ppppppp.", "c.ppppp.", "..pppp..", "..pppp..", ".pppppp."];
    let (sx, sy) = (w / 2 - 12 * k, gy - 9 * k);
    p.sprite_k(sx, sy, &sorc, &[('h', [230, 210, 120]), ('s', [230, 190, 160]), ('p', [120, 60, 150]), ('c', [150, 220, 255])], false, k);
    let pal = ["...hh...", "..hssh..", ".aaaaaa.", "aaaaaaab", "a.aaaa.b", "..aaaa.b", "..l..l..", ".ll..ll."];
    p.sprite_k(
        sx - 10 * k,
        sy + k,
        &pal,
        &[('h', [200, 200, 210]), ('s', [210, 170, 140]), ('a', [200, 190, 160]), ('b', [230, 230, 240]), ('l', [80, 70, 60])],
        false,
        k,
    );
    let (bx, by) = (sx + 8 * k, sy + 4 * k);
    let target = w / 2 + 10 * k;
    for i in 0..(target - bx) {
        p.set(bx + i, by - i / 6, [160, 220, 255]);
    }
    p.glow(target, by - (target - bx) / 6, 4 + k, [170, 230, 255], 1.0);
    let scarab = ["..dddd..", ".dggggd.", "dggddggd", "dggggggd", ".dddddd.", "d.d..d.d"];
    p.sprite_k(target + 2, gy - 6 * k, &scarab, &[('d', [30, 30, 40]), ('g', [60, 120, 110])], false, k);
    let _ = r;
}

// ---------------------------------------------------------------- Dr. Mario 64 VS

/// Two medicine bottles side by side (versus), capsules and germs of our own.
fn pill_bottles(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    p.sky(0, h, &[[30, 40, 90], [70, 40, 110], [30, 20, 60]]);
    for y in (0..h).step_by(6) {
        for x in (0..w).step_by(6) {
            if (x / 6 + y / 6) % 2 == 0 {
                p.set(x + 3, y + 3, [90, 80, 150]);
            }
        }
    }
    let red: C = [232, 60, 70];
    let yel: C = [250, 210, 50];
    let blu: C = [60, 140, 240];
    let cols = [red, yel, blu];
    let cell = k + 1;
    let (bw, bh) = (8 * cell, (gy - 6).min(16 * cell));
    let top = 4;
    for (side, bx) in [(0, w / 2 - bw - 6 * cell), (1, w / 2 + 6 * cell)] {
        // Bottle: neck, glass, liquid tint.
        p.rect(bx - 2, top + 2 * cell - 2, bw + 4, bh + 4, [200, 220, 240]);
        p.rect(bx, top + 2 * cell, bw, bh, [16, 18, 40]);
        p.rect(bx + bw / 2 - 2 * cell, top - 1, 4 * cell, 2 * cell + 1, [200, 220, 240]);
        p.rect(bx + bw / 2 - 2 * cell + 2, top, 4 * cell - 4, 2 * cell, [16, 18, 40]);
        let rows = bh / cell;
        // Germs in the lower part.
        for _ in 0..(6 + side * 2) {
            let (cx, cy) = (r.int(0, 8), r.int(rows / 2, rows));
            let c = cols[r.int(0, 3) as usize];
            let (x, y) = (bx + cx * cell, top + 2 * cell + cy * cell - cell);
            p.rect(x, y, cell, cell, c);
            p.set(x, y, [255, 255, 255]);
            if cell > 2 {
                p.set(x + cell - 1, y, [255, 255, 255]);
                p.rect(x + 1, y + cell - 1, cell - 2, 1, [20, 20, 30]);
            }
        }
        // Settled capsule halves.
        for _ in 0..(10 + side * 4) {
            let (cx, cy) = (r.int(0, 8), r.int(rows * 2 / 3, rows));
            let c = cols[r.int(0, 3) as usize];
            let (x, y) = (bx + cx * cell, top + 2 * cell + cy * cell - cell);
            p.rect(x, y, cell, cell, mix(c, [0, 0, 0], 0.15));
            p.set(x, y, mix(c, [255, 255, 255], 0.5));
        }
        // The falling capsule (two colours).
        let (fx, fy) = (bx + r.int(1, 6) * cell, top + 2 * cell + r.int(1, rows / 3) * cell);
        let (a, b) = (cols[r.int(0, 3) as usize], cols[r.int(0, 3) as usize]);
        p.rect(fx, fy, cell, cell, a);
        p.rect(fx + cell, fy, cell, cell, b);
        p.set(fx, fy, mix(a, [255, 255, 255], 0.6));
    }
    // A germ sneaking between the bottles, and an attack streak.
    let germ = ["..oooo..", ".oggggo.", "ogwggwgo", "ogppggpo", "oggggggo", ".oggggo.", "o.o..o.o"];
    p.sprite_k(w / 2 - 4 * k, gy - 7 * k, &germ, &[('o', [20, 30, 20]), ('g', [80, 200, 90]), ('w', [255, 255, 255]), ('p', [20, 20, 30])], false, k);
    for x in (w / 2 - 6 * cell)..(w / 2 + 6 * cell) {
        if x % 2 == 0 {
            p.set(x, top + bh / 3, [255, 240, 140]);
        }
    }
}

// ------------------------------------------------------------ Operation Spacehog

fn space_shooter(p: &mut Px, r: &mut Rng) {
    let (w, h) = (p.w, p.h);
    let (k, gy) = layout(p);
    p.sky(0, h, &[[4, 4, 16], [14, 8, 34], [6, 6, 20]]);
    // A violet nebula and a ringed planet far behind.
    for _ in 0..4 {
        let (cx, cy) = (r.int(w / 3, w), r.int(0, gy));
        p.glow(cx, cy, 10 + h / 6, [110, 50, 150], 0.35);
    }
    let (px, py, pr) = (w * 4 / 5, gy / 4, 3 + h / 14);
    p.disc(px, py, pr, [60, 70, 120]);
    p.disc(px - pr / 3, py - pr / 3, pr * 2 / 3, [80, 92, 150]);
    for x in -pr * 2..=pr * 2 {
        let y = py + x / 4;
        if x.abs() > pr / 2 || y > py {
            p.set(px + x, y, [190, 170, 120]);
        }
    }
    // Stars, a few streaking past.
    for _ in 0..(w * h / 40) {
        let (x, y) = (r.int(0, w), r.int(0, h));
        let c = if r.chance(15) { [255, 255, 255] } else { [120, 120, 170] };
        p.set(x, y, c);
        if r.chance(8) {
            for i in 1..(2 + 2 * k) {
                p.set(x + i, y, mix(c, [6, 6, 20], i as f32 / (2.0 + 2.0 * k as f32)));
            }
        }
    }
    // Two fighters (green and orange), flying right with their engines lit.
    let ship = ["..ddd......", "..dbbd.....", "ddbbbbbdd..", "fdbbccbbbbd", "fdbbccbbbbd", "ddbbbbbdd..", "..dbbd.....", "..ddd......"];
    let fighters = [([70, 200, 110], w / 10, gy / 3), ([240, 150, 50], w / 6, gy * 2 / 3)];
    for (body, sx, sy) in fighters {
        let pal = [('d', [30, 34, 44]), ('b', body), ('c', [170, 230, 255]), ('f', [120, 160, 255])];
        let y = sy - 4 * k;
        p.sprite_k(sx, y, &ship, &pal, false, k);
        p.glow(sx - k, y + 4 * k, 3 + k, [110, 150, 255], 0.9);
        // Twin lasers.
        let (lx, ly) = (sx + 11 * k, y + 4 * k);
        for (i, x) in (lx..(lx + w / 4)).enumerate() {
            if i % 9 < 5 {
                p.set(x, ly - k, [255, 240, 140]);
                p.set(x, ly + k - 1, [255, 240, 140]);
            }
        }
    }
    // A wave of space hogs (ears, snout, tusks) coming the other way.
    let alien = [".pp...pp.", "ppppppppp", "pwkpppwkp", "ppnnnnnpp", "tpnknknpt", "ppnnnnnpp", ".ppp.ppp."];
    let apal = [('p', [225, 110, 165]), ('w', [255, 255, 255]), ('k', [90, 20, 50]), ('n', [250, 175, 205]), ('t', [250, 240, 210])];
    for i in 0..5 {
        let ax = w / 2 + i * (w / 11) + r.int(0, 3);
        let ay = gy / 6 + ((i as f32 * 1.3).sin() * gy as f32 / 4.0) as i32 + gy / 4;
        p.sprite_k(ax, ay, &alien, &apal, true, k);
    }
    // One just hit: an explosion where the lasers meet it.
    let (ex, ey) = (w / 2 - 2 * k, gy / 2);
    p.glow(ex, ey, 5 + 2 * k, [255, 170, 60], 1.0);
    p.disc(ex, ey, k + 1, [255, 250, 200]);
    for _ in 0..10 {
        p.set(ex + r.int(-4 * k, 4 * k), ey + r.int(-3 * k, 3 * k), [255, 200, 90]);
    }
    // A power-up capsule drifting by.
    let pod = [".yyy.", "yrrry", "yrwry", "yrrry", ".yyy."];
    p.sprite_k(w * 2 / 5, gy * 5 / 6 - 2 * k, &pod, &[('y', [250, 210, 60]), ('r', [220, 60, 50]), ('w', [255, 255, 255])], false, k);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_fits_every_shape() {
        for id in [
            "funny-furries",
            "super-mario-land-port",
            "gtetrinet-ng",
            "s25coop",
            "opents-coop",
            "diablo-splitscreen",
            "diablo2-couch",
            "drmario64-vs",
            "spacehog",
        ] {
            for (w, h) in [(184, 86), (120, 180), (384, 124), (52, 52)] {
                let p = scene(id, w, h).unwrap();
                let (_, _, rgba) = p.upscale(2);
                assert_eq!(rgba.len(), (w * h * 16) as usize);
            }
        }
        assert!(scene("unknown", 10, 10).is_none());
    }
}
