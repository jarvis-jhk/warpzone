//! Controller navigation, so the launcher feels native on the Steam Deck and
//! from the couch. A thread owns the gamepads and wakes the UI, so buttons
//! work even while nothing else is happening on screen.
//!
//! | Pad | Does |
//! |---|---|
//! | D-pad / left stick | move the focus to the next control in that direction, within the page (held: repeats) |
//! | A (south) | activate (Enter) |
//! | B (east) | back (Escape) |
//! | X (west) | page action: library filter *Installed / All games* |
//! | Y (north) | page action: play or install the focused game |
//! | LB / RB | previous / next tab |
//! | LT | refresh the page |
//! | RT | quit (asks first) |
//! | View (select) | player name |
//! | Start (menu) | options of the focused game (change artwork) |
//!
//! The D-pad never reaches the tab bar or the bars at the bottom: the app
//! moves the focus itself (`nav.rs`) among the controls of the page or the
//! open dialog. The hint bar at the bottom shows the buttons as glyphs
//! whenever a controller is connected. Any button hides the mouse pointer and
//! its hover highlight until the mouse moves again.

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
    Lt,
    Rt,
    View,
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
    /// Directions since the last frame, for the app to move the focus with.
    moves: Vec<Dir>,
    /// A direction came in while nothing on the page had the focus: the
    /// page should focus its natural first control.
    pub wants_focus: bool,
    /// Set by a page change made with the controller: focus the new page's
    /// first control in the next frame.
    pub focus_next: bool,
    /// Scripted directions (screenshots of controller navigation), one
    /// every 400 ms.
    pub script: Vec<Dir>,
    script_at: Option<Instant>,
    /// The controller was used after the mouse: the pointer is gone (no
    /// hover highlights) and hidden until the mouse or touch moves again.
    pub pointer_hidden: bool,
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
                    Button::LeftTrigger2 => out.push(Msg::Btn(Btn::Lt)),
                    Button::RightTrigger2 => out.push(Msg::Btn(Btn::Rt)),
                    Button::Select => out.push(Msg::Btn(Btn::View)),
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
        Pad {
            rx: Some(rx),
            connected: 0,
            last_used: None,
            pressed: Vec::new(),
            moves: Vec::new(),
            wants_focus: false,
            focus_next: false,
            script: Vec::new(),
            script_at: None,
            pointer_hidden: false,
        }
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

    /// The directions pressed since the last frame (consumes them).
    pub fn take_moves(&mut self) -> Vec<Dir> {
        std::mem::take(&mut self.moves)
    }

    fn key(raw: &mut RawInput, key: Key) {
        for pressed in [true, false] {
            raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }

    /// Turn this frame's controller input into keys, page buttons and moves.
    pub fn feed(&mut self, raw: &mut RawInput) {
        self.pressed.clear();
        self.moves.clear();
        self.wants_focus = std::mem::take(&mut self.focus_next);
        if raw.events.iter().any(|e| matches!(e, Event::PointerMoved(_) | Event::PointerButton { .. } | Event::MouseWheel { .. } | Event::Touch { .. })) {
            self.pointer_hidden = false;
        }
        let used = self.last_used;
        if !self.script.is_empty() && self.script_at.is_none_or(|t| t.elapsed() > Duration::from_millis(400)) {
            self.script_at = Some(Instant::now());
            let d = self.script.remove(0);
            self.last_used = Some(Instant::now());
            self.moves.push(d);
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
                Msg::Dir(d) => self.moves.push(d),
            }
        }
        self.rx = Some(rx);
        // A button was pressed: egui forgets where the pointer is, so what
        // it rests on no longer lights up next to the focus.
        if self.last_used != used && !self.pointer_hidden {
            self.pointer_hidden = true;
            raw.events.push(Event::PointerGone);
        }
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
    Lt,
    Rt,
    Menu,
    View,
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
        Glyph::Lt => (size * 1.7, "LT", Color32::from_rgb(170, 170, 200)),
        Glyph::Rt => (size * 1.7, "RT", Color32::from_rgb(170, 170, 200)),
        Glyph::View => (size, "", Color32::from_rgb(170, 170, 200)),
        Glyph::Menu => (size, "", Color32::from_rgb(170, 170, 200)),
        Glyph::Dpad => (size, "", Color32::from_rgb(170, 170, 200)),
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, size), egui::Sense::hover());
    let p = ui.painter();
    let fill = Color32::from_rgb(38, 38, 58);
    let c = rect.center();
    match g {
        Glyph::Lb | Glyph::Rb | Glyph::Lt | Glyph::Rt => {
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
        Glyph::View => {
            // Two overlapping windows, as on the Deck's and Xbox pads' View button.
            let s = size * 0.2;
            p.rect_stroke(
                egui::Rect::from_center_size(c + Vec2::new(-s * 0.4, -s * 0.4), Vec2::splat(s * 1.9)),
                CornerRadius::same(1),
                Stroke::new(1.4, Color32::WHITE),
                egui::StrokeKind::Middle,
            );
            p.rect_filled(egui::Rect::from_center_size(c + Vec2::new(s * 0.5, s * 0.5), Vec2::splat(s * 1.9)), CornerRadius::same(1), Color32::WHITE);
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

/// "(A) Open  (Y) Play ..." — a row of glyphs with their meaning (also
/// right-aligned, in a right-to-left layout, reading the same way).
pub fn hints(ui: &mut egui::Ui, items: &[(Glyph, &str)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let rtl = ui.layout().prefer_right_to_left();
        let label = |ui: &mut egui::Ui, l: &str| {
            ui.label(egui::RichText::new(l).size(14.0).color(egui::Color32::from_rgb(214, 214, 236)));
        };
        let mut order: Vec<&(Glyph, &str)> = items.iter().collect();
        if rtl {
            order.reverse();
        }
        for (g, l) in order {
            // An empty label pairs a glyph with the next one ("LB RB Switch tab").
            if rtl {
                if !l.is_empty() {
                    ui.add_space(14.0);
                    label(ui, l);
                }
                glyph(ui, *g, 22.0);
            } else {
                glyph(ui, *g, 22.0);
                if !l.is_empty() {
                    label(ui, l);
                    ui.add_space(14.0);
                }
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
    fn directions_become_moves_not_keys() {
        let (tx, rx) = channel();
        let mut p = Pad {
            rx: Some(rx),
            connected: 1,
            last_used: None,
            pressed: Vec::new(),
            moves: Vec::new(),
            wants_focus: false,
            focus_next: false,
            script: Vec::new(),
            script_at: None,
            pointer_hidden: false,
        };
        // Arrow keys would let egui move the focus anywhere, even into the
        // tab bar; the app moves it itself, within the page.
        tx.send(Msg::Dir(Dir::Down)).unwrap();
        tx.send(Msg::Btn(Btn::X)).unwrap();
        tx.send(Msg::Key(Key::Enter)).unwrap();
        let mut raw = RawInput { focused: true, ..Default::default() };
        p.feed(&mut raw);
        assert!(raw.events.iter().all(|e| matches!(e, Event::Key { key: Key::Enter, .. } | Event::PointerGone)));
        assert_eq!(p.take_moves(), vec![Dir::Down]);
        assert!(p.take_moves().is_empty());
        assert!(p.take(Btn::X) && !p.take(Btn::X));
        // The pad was used: the pointer goes, until the mouse moves.
        assert!(p.pointer_hidden && raw.events.iter().any(|e| matches!(e, Event::PointerGone)));
        let mut raw = RawInput { focused: true, events: vec![Event::PointerMoved(egui::pos2(1.0, 1.0))], ..Default::default() };
        p.feed(&mut raw);
        assert!(!p.pointer_hidden);
        // A game window has the focus: the pad is not for us.
        tx.send(Msg::Dir(Dir::Up)).unwrap();
        p.feed(&mut RawInput { focused: false, ..Default::default() });
        assert!(p.take_moves().is_empty());
    }
}
