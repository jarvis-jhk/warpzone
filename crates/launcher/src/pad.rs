//! Controller navigation: gamepad buttons become the keys egui navigates
//! with, so every screen works from the couch and in Steam Deck Game Mode.
//!
//! | Pad | Key |
//! |---|---|
//! | D-pad / left stick down, right | Tab (next control) |
//! | D-pad / left stick up, left | Shift+Tab (previous control) |
//! | A (south) | Enter (activate) |
//! | B (east) | Escape (back) |
//! | LB / RB | previous / next tab (`Ctrl+PageUp/PageDown`) |
//! | Y (north) | Space |

use eframe::egui::{Event, Key, Modifiers, RawInput};
use gilrs::{Axis, Button, EventType, Gilrs};
use std::time::Instant;

pub struct Pad {
    gilrs: Option<Gilrs>,
    last_axis: Instant,
}

impl Pad {
    pub fn new() -> Pad {
        Pad { gilrs: Gilrs::new().ok(), last_axis: Instant::now() }
    }

    fn key(raw: &mut RawInput, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            raw.events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
    }

    pub fn feed(&mut self, raw: &mut RawInput) -> bool {
        let Some(g) = self.gilrs.as_mut() else { return false };
        let mut any = false;
        let shift = Modifiers { shift: true, ..Default::default() };
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        while let Some(ev) = g.next_event() {
            any = true;
            match ev.event {
                EventType::ButtonPressed(b, _) => match b {
                    Button::DPadDown | Button::DPadRight => Self::key(raw, Key::Tab, Modifiers::NONE),
                    Button::DPadUp | Button::DPadLeft => Self::key(raw, Key::Tab, shift),
                    Button::South => Self::key(raw, Key::Enter, Modifiers::NONE),
                    Button::East => Self::key(raw, Key::Escape, Modifiers::NONE),
                    Button::North => Self::key(raw, Key::Space, Modifiers::NONE),
                    Button::LeftTrigger => Self::key(raw, Key::PageUp, ctrl),
                    Button::RightTrigger => Self::key(raw, Key::PageDown, ctrl),
                    _ => {}
                },
                EventType::AxisChanged(axis, v, _) if self.last_axis.elapsed().as_millis() > 180 => {
                    let k = match axis {
                        Axis::LeftStickY if v < -0.6 => Some((Key::Tab, Modifiers::NONE)),
                        Axis::LeftStickY if v > 0.6 => Some((Key::Tab, shift)),
                        Axis::LeftStickX if v > 0.6 => Some((Key::Tab, Modifiers::NONE)),
                        Axis::LeftStickX if v < -0.6 => Some((Key::Tab, shift)),
                        _ => None,
                    };
                    if let Some((k, m)) = k {
                        self.last_axis = Instant::now();
                        Self::key(raw, k, m);
                    }
                }
                _ => {}
            }
        }
        any
    }
}
