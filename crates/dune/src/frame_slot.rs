//! Frame handoff from the game thread to whatever consumes presented frames.
//!
//! The DOS engine just wrote straight to A000. The port has to ferry the
//! 320×200 indexed framebuffer + its palette from the game thread to the
//! winit/wgpu present path (the interactive binary) or to an offline
//! collector (the headless examples that record sequences as PNGs).
//!
//! Two implementations of [`FrameSink`] are provided:
//!
//! * [`FrameSlot`] — latest-only, non-blocking. The previous undrained frame
//!   is dropped when a new one is published. The interactive binary uses
//!   this so a fast game thread never stalls on a slow present pipeline.
//! * `mpsc::SyncSender<(FrameBuffer, Palette)>` — preserves every frame and
//!   blocks the producer when the channel is full. The offline rendering
//!   examples use this so animation captures keep every frame.
//!
//! `GameState` stores a `Box<dyn FrameSink>` and calls `publish` once per
//! frame; the choice of sink is the caller's.

use std::sync::{Arc, Mutex, mpsc};

use crate::{FrameBuffer, Palette};

/// One presented frame: the 320×200 indexed framebuffer and the palette that
/// expands it. Cloned out of `GameState::screen` / `screen_pal` at publish
/// time so the game thread can keep mutating its own copies.
pub type Frame = (FrameBuffer, Palette);

/// Anything that accepts a `(framebuffer, palette)` pair from the game
/// thread. Implementations decide whether to queue, drop-old, or block.
pub trait FrameSink: Send {
    fn publish(&self, framebuffer: FrameBuffer, palette: Palette);
}

impl FrameSink for mpsc::SyncSender<Frame> {
    fn publish(&self, framebuffer: FrameBuffer, palette: Palette) {
        let _ = self.send((framebuffer, palette));
    }
}

/// Latest-only frame handoff. `publish` overwrites any previous undrained
/// frame; `take_latest` returns the most recent one (and clears the slot).
///
/// Cloning a `FrameSlot` produces another handle to the same underlying
/// slot — producer and consumer each hold one.
#[derive(Clone)]
pub struct FrameSlot {
    inner: Arc<Mutex<Option<Frame>>>,
}

impl FrameSlot {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Return the most recently published frame, if one has arrived since
    /// the last `take_latest`. Non-blocking.
    pub fn take_latest(&self) -> Option<Frame> {
        self.inner.lock().unwrap().take()
    }
}

impl Default for FrameSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameSink for FrameSlot {
    fn publish(&self, framebuffer: FrameBuffer, palette: Palette) {
        *self.inner.lock().unwrap() = Some((framebuffer, palette));
    }
}
