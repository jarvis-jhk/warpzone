//! Controller navigation, so the launcher feels native on the Steam Deck and
//! from the couch. A thread owns the gamepads and wakes the UI, so buttons
//! work even while nothing else is happening on screen.
//!
//! | Pad | Does |
//! |---|---|
//! | D-pad / left stick | move the focus to the next control in that direction (held: repeats) |
//! | A (south) | activate (Enter) |
//! | B (east) | back (Escape) |
//! | X (west) | page action: library filter *Installed / All games* |
//! | Y (north) | page action: play or install the focused game |
//! | LB / RB | previous / next tab |
//! | Start (menu) | options of the focused game (change artwork) |
//!
//! The hint bar at the bottom shows these as button glyphs whenever a
//! controller is connected.

use eframe::egui::{self, Event, Key, Modifiers, RawInput};
use gilrs::{Axis, Button, EventType, Gilrs};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Buttons with a meaning of their own (A and B become Enter and Escape).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    X,
    Y,
    Lb,
    Rb,
    Start,
}

enum Msg {
    Dir(Dir),
    Key(Key),
    Btn(Btn),
    Connected(usize),
}

pub struct Pad {
    rx: Option<Receiver<Msg>>,
    /// Controllers connected now.
    pub connected: usize,
    /// When a controller was last used (the hint bar and focus follow it).
    pub last_used: Option<Instant>,
    /// Buttons pressed since the last frame, for the pages to consume.
    pressed: Vec<Btn>,
    /// A direction came in while nothing had the focus: the page should
    /// focus its natural first control.
    pub wants_focus: bool,
    /// Set by a page change made with the controller: focus the new page's
    /// first control in the next frame.
    pub focus_next: bool,
    /// Scripted directions (screenshots of controller navigation), one
    /// every 400 ms.
    pub script: Vec<Dir>,
    script_at: Option<Instant>,
}

const REPEAT_DELAY: Duration = Duration::from_millis(380);
const REPEAT_EVERY: Duration = Duration::from_millis(120);

fn stick(v: f32, neg: Dir, pos: Dir) -> Option<Dir> {
    if v > 0.6 {
        Some(pos)
    } else if v < -0.6 {
        Some(neg)
    } else {
        None
    }
}

fn run(tx: Sender<Msg>, wake: impl Fn()) {
    let Ok(mut g) = Gilrs::new() else { return };
    let _ = tx.send(Msg::Connected(g.gamepads().count()));
    wake();
    // The direction being held and when it repeats next.
    let mut held: Option<(Dir, Instant)> = None;
    let (mut sx, mut sy): (Option<Dir>, Option<Dir>) = (None, None);
    loop {
        let wait = held.map(|(_, at)| at.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(1));
        let mut out: Vec<Msg> = Vec::new();
        if let Some(ev) = g.next_event_blocking(Some(wait)) {
            let press = |d: Dir, held: &mut Option<(Dir, Instant)>, out: &mut Vec<Msg>| {
                out.push(Msg::Dir(d));
                *held = Some((d, Instant::now() + REPEAT_DELAY));
            };
            match ev.event {
                EventType::ButtonPressed(b, _) => match b {
                    Button::DPadUp => press(Dir::Up, &mut held, &mut out),
                    Button::DPadDown => press(Dir::Down, &mut held, &mut out),
                    Button::DPadLeft => press(Dir::Left, &mut held, &mut out),
                    Button::DPadRight => press(Dir::Right, &mut held, &mut out),
                    Button::South => out.push(Msg::Key(Key::Enter)),
                    Button::East => out.push(Msg::Key(Key::Escape)),
                    Button::West => out.push(Msg::Btn(Btn::X)),
                    Button::North => out.push(Msg::Btn(Btn::Y)),
                    Button::LeftTrigger => out.push(Msg::Btn(Btn::Lb)),
                    Button::RightTrigger => out.push(Msg::Btn(Btn::Rb)),
                    Button::Start => out.push(Msg::Btn(Btn::Start)),
                    _ => {}
                },
                EventType::ButtonReleased(b, _) => {
                    let d = match b {
                        Button::DPadUp => Some(Dir::Up),
                        Button::DPadDown => Some(Dir::Down),
                        Button::DPadLeft => Some(Dir::Left),
                        Button::DPadRight => Some(Dir::Right),
                        _ => None,
                    };
                    if d.is_some() && held.map(|h| h.0) == d {
                        held = None;
                    }
                }
                EventType::AxisChanged(axis @ (Axis::LeftStickX | Axis::LeftStickY), v, _) => {
                    // gilrs: up and right are positive.
                    let (slot, now) = match axis {
                        Axis::LeftStickX => (&mut sx, stick(v, Dir::Left, Dir::Right)),
                        _ => (&mut sy, stick(v, Dir::Down, Dir::Up)),
                    };
                    // Hysteresis: a direction ends only once the stick is back near the middle.
                    if let Some(d) = now
                        && now != *slot
                    {
                        *slot = now;
                        press(d, &mut held, &mut out);
                    } else if now.is_none() && v.abs() < 0.3 && slot.is_some() {
                        if held.map(|h| h.0) == *slot {
                            held = None;
                        }
                        *slot = None;
                    }
                }
                EventType::Connected | EventType::Disconnected => out.push(Msg::Connected(g.gamepads().count())),
                _ => {}
            }
        }
        if let Some((d, at)) = held
            && Instant::now() >= at
        {
            out.push(Msg::Dir(d));
            held = Some((d, Instant::now() + REPEAT_EVERY));
        }
        if !out.is_empty() {
            for m in out {
                if tx.send(m).is_err() {
                    return;
                }
            }
            wake();
        }
    }
}

/// Is this a Steam Deck (or Steam's Game Mode)? Then the hints show from the start.
pub fn steam_deck() -> bool {
    std::env::var("SteamDeck").is_ok_and(|v| v == "1")
        || std::env::var_os("SteamGamepadUI").is_some()
        || std::fs::read_to_string("/sys/devices/virtual/dmi/id/board_vendor").is_ok_and(|v| v.trim() == "Valve")
}

impl Pad {
    /// Start reading controllers; `ctx` is woken for every button.
    pub fn start(ctx: egui::Context) -> Pad {
        let (tx, rx) = channel();
        std::thread::Builder::new().name("gamepads".into()).spawn(move || run(tx, move || ctx.request_repaint())).ok();
        Pad { rx: Some(rx), connected: 0, last_used: None, pressed: Vec::new(), wants_focus: false, focus_next: false, script: Vec::new(), script_at: None }
    }

    /// Show controller hints: a controller is connected, or this is a Deck.
    pub fn active(&self) -> bool {
        self.connected > 0 || self.last_used.is_some() || steam_deck()
    }

    /// Was `b` pressed this frame? (Consumes it.)
    pub fn take(&mut self, b: Btn) -> bool {
        let n = self.pressed.len();
        self.pressed.retain(|x| *x != b);
        self.pressed.len() != n
    }

    fn key(raw: &mut RawInput, key: Key) {
        for pressed in [true, false] {
            raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }

    /// Turn this frame's controller input into keys (and page buttons).
    /// `focused`: does a widget have the keyboard focus?
    pub fn feed(&mut self, raw: &mut RawInput, focused: bool) {
        self.pressed.clear();
        self.wants_focus = std::mem::take(&mut self.focus_next);
        if !self.script.is_empty() && self.script_at.is_none_or(|t| t.elapsed() > Duration::from_millis(400)) {
            self.script_at = Some(Instant::now());
            let d = self.script.remove(0);
            self.last_used = Some(Instant::now());
            self.direction(raw, d, focused);
        }
        let Some(rx) = self.rx.take() else { return };
        // The pad is shared with the games: while another window has the
        // focus (a game is running), its buttons are not for us.
        let mine = raw.focused;
        while let Ok(m) = rx.try_recv() {
            if !mine && !matches!(m, Msg::Connected(_)) {
                continue;
            }
            if !matches!(m, Msg::Connected(_)) {
                self.last_used = Some(Instant::now());
            }
            match m {
                Msg::Connected(n) => self.connected = n,
                Msg::Key(k) => Self::key(raw, k),
                Msg::Btn(b) => self.pressed.push(b),
                Msg::Dir(d) => self.direction(raw, d, focused),
            }
        }
        self.rx = Some(rx);
    }

    fn direction(&mut self, raw: &mut RawInput, d: Dir, focused: bool) {
        // With nothing focused the first press only picks a starting point.
        if !focused {
            self.wants_focus = true;
            return;
        }
        let k = match d {
            Dir::Up => Key::ArrowUp,
            Dir::Down => Key::ArrowDown,
            Dir::Left => Key::ArrowLeft,
            Dir::Right => Key::ArrowRight,
        };
        Self::key(raw, k);
    }
}

/// A controller button as the Deck and Xbox pads label it.
#[derive(Clone, Copy)]
pub enum Glyph {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Menu,
    Dpad,
}

/// Draw a button glyph (a filled disc or a shoulder-button pill).
pub fn glyph(ui: &mut egui::Ui, g: Glyph, size: f32) -> egui::Response {
    use egui::{Color32, CornerRadius, FontId, Stroke, Vec2, pos2};
    let (w, letter, ring) = match g {
        Glyph::A => (size, "A", Color32::from_rgb(88, 196, 96)),
        Glyph::B => (size, "B", Color32::from_rgb(232, 84, 84)),
        Glyph::X => (size, "X", Color32::from_rgb(80, 140, 240)),
        Glyph::Y => (size, "Y", Color32::from_rgb(240, 196, 60)),
        Glyph::Lb => (size * 1.7, "LB", Color32::from_rgb(170, 170, 200)),
        Glyph::Rb => (size * 1.7, "RB", Color32::from_rgb(170, 170, 200)),
        Glyph::Menu => (size, "", Color32::from_rgb(170, 170, 200)),
        Glyph::Dpad => (size, "", Color32::from_rgb(170, 170, 200)),
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, size), egui::Sense::hover());
    let p = ui.painter();
    let fill = Color32::from_rgb(38, 38, 58);
    let c = rect.center();
    match g {
        Glyph::Lb | Glyph::Rb => {
            p.rect(rect, CornerRadius::same((size / 2.0) as u8), fill, Stroke::new(1.5, ring), egui::StrokeKind::Inside);
        }
        _ => {
            p.circle(c, size / 2.0 - 0.5, fill, Stroke::new(1.5, ring));
        }
    }
    match g {
        Glyph::Menu => {
            for dy in [-3.5, 0.0, 3.5] {
                p.line_segment([pos2(c.x - 4.5, c.y + dy), pos2(c.x + 4.5, c.y + dy)], Stroke::new(1.6, Color32::WHITE));
            }
        }
        Glyph::Dpad => {
            let a = size * 0.16;
            let l = size * 0.34;
            p.rect_filled(egui::Rect::from_center_size(c, Vec2::new(a * 2.0, l * 2.0)), CornerRadius::same(1), Color32::WHITE);
            p.rect_filled(egui::Rect::from_center_size(c, Vec2::new(l * 2.0, a * 2.0)), CornerRadius::same(1), Color32::WHITE);
        }
        _ => {
            p.text(c, egui::Align2::CENTER_CENTER, letter, FontId::proportional(size * if letter.len() > 1 { 0.48 } else { 0.62 }), Color32::WHITE);
        }
    }
    resp
}

/// "(A) Open  (Y) Play ..." — a row of glyphs with their meaning.
pub fn hints(ui: &mut egui::Ui, items: &[(Glyph, &str)]) {
    ui.horizontal(|ui| {
        for (g, label) in items {
            ui.spacing_mut().item_spacing.x = 5.0;
            glyph(ui, *g, 22.0);
            // An empty label pairs a glyph with the next one ("LB RB Switch tab").
            if !label.is_empty() {
                ui.label(egui::RichText::new(*label).size(14.0).color(egui::Color32::from_rgb(214, 214, 236)));
                ui.add_space(14.0);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticks_need_a_clear_push() {
        assert_eq!(stick(0.7, Dir::Down, Dir::Up), Some(Dir::Up));
        assert_eq!(stick(-0.7, Dir::Down, Dir::Up), Some(Dir::Down));
        assert_eq!(stick(0.4, Dir::Down, Dir::Up), None);
    }

    #[test]
    fn directions_become_arrows_only_with_a_focus() {
        let (tx, rx) = channel();
        let mut p = Pad {
            rx: Some(rx),
            connected: 1,
            last_used: None,
            pressed: Vec::new(),
            wants_focus: false,
            focus_next: false,
            script: Vec::new(),
            script_at: None,
        };
        tx.send(Msg::Dir(Dir::Down)).unwrap();
        let mut raw = RawInput { focused: true, ..Default::default() };
        p.feed(&mut raw, false);
        assert!(p.wants_focus && raw.events.is_empty());
        tx.send(Msg::Dir(Dir::Down)).unwrap();
        tx.send(Msg::Btn(Btn::X)).unwrap();
        p.feed(&mut raw, true);
        assert!(matches!(raw.events[0], Event::Key { key: Key::ArrowDown, pressed: true, .. }));
        assert!(p.take(Btn::X) && !p.take(Btn::X));
    }
}
