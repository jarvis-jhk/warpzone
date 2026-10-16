//! Background work. A job runs on a worker thread and returns a closure that
//! is applied to the app on the UI thread, so the UI never blocks and never
//! shares state across threads.

use crate::app::App;
use std::sync::mpsc::{Receiver, Sender, channel};

pub type Apply = Box<dyn FnOnce(&mut App) + Send>;

#[derive(Clone)]
pub struct Jobs {
    tx: Sender<Apply>,
    ctx: eframe::egui::Context,
}

impl Jobs {
    pub fn new(ctx: eframe::egui::Context) -> (Jobs, Receiver<Apply>) {
        let (tx, rx) = channel();
        (Jobs { tx, ctx }, rx)
    }

    /// Run `f` on a new thread; its result is applied on the UI thread.
    pub fn run(&self, f: impl FnOnce() -> Apply + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let apply = f();
            let _ = tx.send(apply);
            ctx.request_repaint();
        });
    }

    /// Send an update from inside a running job (progress).
    pub fn send(&self, apply: Apply) {
        let _ = self.tx.send(apply);
        self.ctx.request_repaint();
    }
}
