//! D-pad navigation within one part of the window. egui's own arrow-key
//! focus moves among every control on screen (into the tab bar, the bars at
//! the bottom) and stops dead in a text field, which eats the arrows. Here the
//! app notes which widgets a part of the window added this frame (the page in
//! the middle, or the dialog on top) and moves the focus among only those.

use crate::pad::Dir;
use eframe::egui::{self, Id, LayerId, Order, Rangef, Rect, Sense, Vec2};
use std::collections::HashSet;

/// A control the D-pad can go to.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub id: Id,
    pub layer: LayerId,
    pub rect: Rect,
}

/// The widgets registered so far this frame: call before drawing a part,
/// then [`since`] after it.
pub fn mark(ctx: &egui::Context) -> HashSet<Id> {
    ctx.viewport(|v| v.this_pass.widgets.layers().flat_map(|(_, ws)| ws.iter().map(|w| w.id)).collect())
}

/// The controls a part added since [`mark`]: enabled, clickable and focusable
/// (buttons, tiles, checkboxes, text fields — not labels, panels or a
/// dialog's backdrop).
pub fn since(ctx: &egui::Context, before: &HashSet<Id>) -> Vec<Target> {
    ctx.viewport(|v| {
        v.this_pass
            .widgets
            .layers()
            .flat_map(|(_, ws)| ws.iter())
            .filter(|w| !before.contains(&w.id) && w.enabled && w.sense.contains(Sense::CLICK | Sense::FOCUSABLE) && w.rect.is_positive())
            .map(|w| Target { id: w.id, layer: w.layer_id, rect: w.rect })
            .collect()
    })
}

/// Only the top-most layer counts: with a drop-down open, only its entries.
fn top_layer(cands: &[Target], focused: Option<Id>) -> Vec<Target> {
    let Some(top) = cands.iter().map(|t| t.layer.order).max_by_key(|o| order_rank(*o)) else { return Vec::new() };
    let mine = focused.and_then(|f| cands.iter().find(|t| t.id == f)).filter(|t| t.layer.order == top).map(|t| t.layer);
    let layer = mine.or_else(|| cands.iter().rev().find(|t| t.layer.order == top).map(|t| t.layer));
    cands.iter().filter(|t| Some(t.layer) == layer).copied().collect()
}

fn order_rank(o: Order) -> u8 {
    match o {
        Order::Background => 0,
        Order::Middle => 1,
        Order::Foreground => 2,
        Order::Tooltip => 3,
        Order::Debug => 4,
    }
}

/// The first control of a part: the top-most, then the left-most.
pub fn first(cands: &[Target]) -> Option<Id> {
    top_layer(cands, None).iter().min_by(|a, b| (a.rect.top(), a.rect.left()).partial_cmp(&(b.rect.top(), b.rect.left())).unwrap()).map(|t| t.id)
}

/// Is `focused` one of `cands` (in the top-most layer)?
pub fn inside(cands: &[Target], focused: Option<Id>) -> bool {
    focused.is_some_and(|f| top_layer(cands, Some(f)).iter().any(|t| t.id == f))
}

/// Where `d` goes from `focused` among `cands`: `None` when `focused` is not
/// one of them (the caller then starts at the page's first control) or
/// nothing lies that way (the focus stays).
pub fn step(cands: &[Target], focused: Option<Id>, d: Dir) -> Option<Id> {
    let cands = top_layer(cands, focused);
    let cur = cands.iter().find(|t| Some(t.id) == focused)?;
    // As egui does it: overlapping ranges count as aligned, and only what
    // lies within 45° of the direction is a candidate.
    fn range_diff(a: Rangef, b: Rangef) -> f32 {
        if a.intersection(b).span() >= 0.5 * b.span().min(a.span()) { 0.0 } else { a.center() - b.center() }
    }
    let dir = match d {
        Dir::Up => Vec2::UP,
        Dir::Down => Vec2::DOWN,
        Dir::Left => Vec2::LEFT,
        Dir::Right => Vec2::RIGHT,
    };
    let mut best: Option<(f32, Id)> = None;
    let mut wide: Option<(f32, Id)> = None;
    for c in cands.iter().filter(|c| c.id != cur.id) {
        let to = Vec2::new(range_diff(c.rect.x_range(), cur.rect.x_range()), range_diff(c.rect.y_range(), cur.rect.y_range()));
        if to == Vec2::ZERO {
            continue;
        }
        let cos = to.normalized().dot(dir);
        if cos >= 0.5_f32.sqrt() {
            let score = to.length() / (cos * cos);
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, c.id));
            }
        } else if cos > 0.0 {
            // Nothing in the cone (a lone button at the right edge above a
            // form at the left): the nearest one that way at all.
            let along = to.dot(dir);
            let score = along + 2.0 * (to - dir * along).length();
            if wide.is_none_or(|(s, _)| score < s) {
                wide = Some((score, c.id));
            }
        }
    }
    best.or(wide).map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn t(n: &str, x: f32, y: f32, w: f32, h: f32) -> Target {
        Target { id: Id::unique(n), layer: LayerId::background(), rect: Rect::from_min_size(pos2(x, y), Vec2::new(w, h)) }
    }

    #[test]
    fn moves_through_a_grid_of_tiles() {
        let mut c = vec![t("filter", 0.0, 0.0, 80.0, 24.0)];
        for r in 0..2 {
            for col in 0..3 {
                c.push(t(&format!("{r}{col}"), col as f32 * 200.0, 40.0 + r as f32 * 300.0, 180.0, 270.0));
            }
        }
        assert_eq!(first(&c), Some(Id::unique("filter")));
        assert_eq!(step(&c, Some(Id::unique("00")), Dir::Right), Some(Id::unique("01")));
        assert_eq!(step(&c, Some(Id::unique("01")), Dir::Down), Some(Id::unique("11")));
        assert_eq!(step(&c, Some(Id::unique("10")), Dir::Left), None);
        assert_eq!(step(&c, Some(Id::unique("00")), Dir::Up), Some(Id::unique("filter")));
        // Nothing straight below the filter button but the grid's first row:
        // the nearest of it, not nothing.
        assert_eq!(step(&c, Some(Id::unique("filter")), Dir::Down), Some(Id::unique("00")));
        let lone = [t("refresh", 1000.0, 0.0, 80.0, 24.0), t("field", 0.0, 50.0, 300.0, 24.0), t("join", 310.0, 50.0, 60.0, 24.0)];
        assert_eq!(step(&lone, Some(Id::unique("refresh")), Dir::Down), Some(Id::unique("join")));
        assert_eq!(step(&lone, Some(Id::unique("refresh")), Dir::Up), None);
        // The focus outside the part (in the tab bar): start over.
        assert_eq!(step(&c, Some(Id::unique("tab")), Dir::Down), None);
    }

    #[test]
    fn an_open_drop_down_keeps_the_focus() {
        let mut c = vec![t("combo", 0.0, 0.0, 100.0, 24.0), t("below", 0.0, 100.0, 100.0, 24.0)];
        let pop = LayerId::new(Order::Foreground, Id::unique("popup"));
        c.push(Target { id: Id::unique("a"), layer: pop, rect: Rect::from_min_size(pos2(0.0, 26.0), Vec2::new(100.0, 20.0)) });
        c.push(Target { id: Id::unique("b"), layer: pop, rect: Rect::from_min_size(pos2(0.0, 48.0), Vec2::new(100.0, 20.0)) });
        assert_eq!(first(&c), Some(Id::unique("a")));
        assert!(!inside(&c, Some(Id::unique("combo"))) && inside(&c, Some(Id::unique("a"))));
        assert_eq!(step(&c, Some(Id::unique("a")), Dir::Down), Some(Id::unique("b")));
        assert_eq!(step(&c, Some(Id::unique("b")), Dir::Down), None);
    }
}
