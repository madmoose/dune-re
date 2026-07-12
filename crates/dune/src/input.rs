//! Host keyboard + mouse input, mirroring the DOS keyboard ISR (= seg000:efe7)
//! and the INT 33h mouse driver.
//!
//! The DOS engine fills shared globals from an asynchronous keyboard interrupt
//! and polls them (plus the mouse via INT 33h) from `any_key_pressed`
//! (= seg000:dd63). The port keeps the same split: the host event loop (winit)
//! writes [`InputState`] as the "ISR", and the game thread polls it. The
//! `Arc<Mutex<…>>` wrapper matches that one-writer / one-reader model; the lock
//! is held only for the brief field updates here.

use std::sync::{Arc, Mutex};

use winit::keyboard::KeyCode;

use crate::GameState;

/// Shared input state: written by the host event loop, polled by the game.
pub type SharedInput = Arc<Mutex<InputState>>;

/// Number of scancode slots in the DOS keyboard array. The ISR ignores
/// scancodes >= 0x5a (= seg000:effc `cmp di,5ah; jnb`).
pub const KB_KEYS_LEN: usize = 0x5a;

// Scan Code Set 1 make codes the game reads by name.
//
/// ESC — `kb_check_for_esc_key_hit` compares the buffer against this
/// (= seg000:de59 `cmp …,1`).
pub const SCANCODE_ESC: u8 = 0x01;
/// P — the pause key; its array slot is `_byte_2C34A_kb_keys_p` (0xce81 + 0x19).
pub const SCANCODE_P: u8 = 0x19;
/// Enter — its array slot is `_byte_2C34D_kb_keys_enter` (0xce81 + 0x1c).
pub const SCANCODE_ENTER: u8 = 0x1c;

/// Keyboard + mouse state shared across the host/game thread boundary.
#[derive(Clone)]
pub struct InputState {
    // = seg001:cee8 _byte_2C398_key_hit_scancode — the most recent key-PRESS
    // scancode (make code). The ISR records it on press only; releases carry the
    // high bit, which zeroes the value before the store (= seg000:f00c
    // `or al,al; jz`). `get_and_reset_key_scancode` reads then clears it.
    pub key_hit_scancode: u8,

    // = seg001:cee9 _byte_2C399_kb_esc_was_hit — set by kb_check_for_esc_key_hit
    // when the buffered scancode is ESC.
    pub kb_esc_was_hit: u8,

    // = the seg001:ce81.. keyboard key-down array, indexed by Set-1 scancode:
    // 0xff while a key is held, 0x00 when up (= seg000:f00a store). The game
    // reads individual entries (P at +0x19, Enter at +0x1c, …).
    pub kb_keys: [u8; KB_KEYS_LEN],

    // Mouse position in game-framebuffer coordinates (x 0..319, y 0..199). The
    // host maps window-cursor coordinates into this 320×200 space; it is the
    // post-scale equivalent of _word_2D0E8_mouse_pos_x / _word_2D0E6_mouse_pos_y
    // (the INT 33,3 reading after the >> _word_21A30_mouse_pos_scaler shift).
    pub mouse_x: u16,
    pub mouse_y: u16,

    // = the INT 33,3 button bitmask masked to the low 3 bits (bl & 7):
    // bit0 = left, bit1 = right, bit2 = middle.
    pub mouse_buttons: u8,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            key_hit_scancode: 0,
            kb_esc_was_hit: 0,
            kb_keys: [0; KB_KEYS_LEN],
            mouse_x: 0,
            mouse_y: 0,
            mouse_buttons: 0,
        }
    }
}

impl InputState {
    /// A fresh shared input, ready to hand to both the host event loop and
    /// `GameState::new_with_input`.
    pub fn shared() -> SharedInput {
        Arc::new(Mutex::new(Self::default()))
    }

    // = seg000:efe7 the keyboard ISR core. A make code (press) sets the key's
    // array entry to 0xff and records it as the latest hit; a break code
    // (release) only clears the array entry — DOS skips the scancode store for
    // releases (= seg000:f00c). Scancodes >= 0x5a are ignored (= seg000:effc).
    pub fn on_key(&mut self, scancode: u8, pressed: bool) {
        let idx = (scancode & 0x7f) as usize;
        if idx >= KB_KEYS_LEN {
            return;
        }
        self.kb_keys[idx] = if pressed { 0xff } else { 0x00 };
        if pressed {
            self.key_hit_scancode = scancode & 0x7f;
        }
    }

    /// Update the mouse position (already mapped into 320×200 game coordinates).
    pub fn on_mouse_move(&mut self, x: u16, y: u16) {
        self.mouse_x = x;
        self.mouse_y = y;
    }

    /// Update the mouse button bitmask (= the INT 33,3 `bl & 7`).
    pub fn on_mouse_button(&mut self, buttons: u8) {
        self.mouse_buttons = buttons & 7;
    }
}

// = the host keyboard map: winit physical KeyCode -> DOS Scan Code Set 1 make
// code. The DOS ISR reads raw set-1 codes straight from port 0x60
// (= seg000:eff1); winit abstracts the hardware, so the host translates back to
// the codes the game's scancode array and checks expect. Extended (E0-prefixed)
// navigation keys map to their keypad equivalents, matching the DOS ISR which
// discards the E0 prefix byte (0x60 >= 0x5a, ignored at seg000:effc) and
// registers only the trailing keypad code.
pub fn keycode_to_scancode(key: KeyCode) -> Option<u8> {
    let sc = match key {
        KeyCode::Escape => 0x01,
        KeyCode::Digit1 => 0x02,
        KeyCode::Digit2 => 0x03,
        KeyCode::Digit3 => 0x04,
        KeyCode::Digit4 => 0x05,
        KeyCode::Digit5 => 0x06,
        KeyCode::Digit6 => 0x07,
        KeyCode::Digit7 => 0x08,
        KeyCode::Digit8 => 0x09,
        KeyCode::Digit9 => 0x0a,
        KeyCode::Digit0 => 0x0b,
        KeyCode::Minus => 0x0c,
        KeyCode::Equal => 0x0d,
        KeyCode::Backspace => 0x0e,
        KeyCode::Tab => 0x0f,
        KeyCode::KeyQ => 0x10,
        KeyCode::KeyW => 0x11,
        KeyCode::KeyE => 0x12,
        KeyCode::KeyR => 0x13,
        KeyCode::KeyT => 0x14,
        KeyCode::KeyY => 0x15,
        KeyCode::KeyU => 0x16,
        KeyCode::KeyI => 0x17,
        KeyCode::KeyO => 0x18,
        KeyCode::KeyP => 0x19,
        KeyCode::BracketLeft => 0x1a,
        KeyCode::BracketRight => 0x1b,
        KeyCode::Enter => 0x1c,
        KeyCode::ControlLeft => 0x1d,
        KeyCode::KeyA => 0x1e,
        KeyCode::KeyS => 0x1f,
        KeyCode::KeyD => 0x20,
        KeyCode::KeyF => 0x21,
        KeyCode::KeyG => 0x22,
        KeyCode::KeyH => 0x23,
        KeyCode::KeyJ => 0x24,
        KeyCode::KeyK => 0x25,
        KeyCode::KeyL => 0x26,
        KeyCode::Semicolon => 0x27,
        KeyCode::Quote => 0x28,
        KeyCode::Backquote => 0x29,
        KeyCode::ShiftLeft => 0x2a,
        KeyCode::Backslash => 0x2b,
        KeyCode::KeyZ => 0x2c,
        KeyCode::KeyX => 0x2d,
        KeyCode::KeyC => 0x2e,
        KeyCode::KeyV => 0x2f,
        KeyCode::KeyB => 0x30,
        KeyCode::KeyN => 0x31,
        KeyCode::KeyM => 0x32,
        KeyCode::Comma => 0x33,
        KeyCode::Period => 0x34,
        KeyCode::Slash => 0x35,
        KeyCode::ShiftRight => 0x36,
        KeyCode::NumpadMultiply => 0x37,
        KeyCode::AltLeft => 0x38,
        KeyCode::Space => 0x39,
        KeyCode::CapsLock => 0x3a,
        KeyCode::F1 => 0x3b,
        KeyCode::F2 => 0x3c,
        KeyCode::F3 => 0x3d,
        KeyCode::F4 => 0x3e,
        KeyCode::F5 => 0x3f,
        KeyCode::F6 => 0x40,
        KeyCode::F7 => 0x41,
        KeyCode::F8 => 0x42,
        KeyCode::F9 => 0x43,
        KeyCode::F10 => 0x44,
        KeyCode::NumLock => 0x45,
        KeyCode::ScrollLock => 0x46,
        KeyCode::Numpad7 => 0x47,
        KeyCode::Numpad8 => 0x48,
        KeyCode::Numpad9 => 0x49,
        KeyCode::NumpadSubtract => 0x4a,
        KeyCode::Numpad4 => 0x4b,
        KeyCode::Numpad5 => 0x4c,
        KeyCode::Numpad6 => 0x4d,
        KeyCode::NumpadAdd => 0x4e,
        KeyCode::Numpad1 => 0x4f,
        KeyCode::Numpad2 => 0x50,
        KeyCode::Numpad3 => 0x51,
        KeyCode::Numpad0 => 0x52,
        KeyCode::NumpadDecimal => 0x53,
        KeyCode::F11 => 0x57,
        KeyCode::F12 => 0x58,
        // Extended navigation keys -> their keypad equivalents (see note above).
        KeyCode::ArrowUp => 0x48,
        KeyCode::ArrowLeft => 0x4b,
        KeyCode::ArrowRight => 0x4d,
        KeyCode::ArrowDown => 0x50,
        KeyCode::Home => 0x47,
        KeyCode::PageUp => 0x49,
        KeyCode::End => 0x4f,
        KeyCode::PageDown => 0x51,
        KeyCode::Insert => 0x52,
        KeyCode::Delete => 0x53,
        KeyCode::NumpadEnter => 0x1c,
        _ => return None,
    };
    Some(sc)
}

impl GameState {
    // = seg000:dd5a get_and_reset_key_scancode — return the buffered key-press
    // scancode and clear it (`xchg al,[…]`). Returns 0 when nothing is buffered.
    pub fn get_and_reset_key_scancode(&mut self) -> u8 {
        std::mem::take(&mut self.input.lock().unwrap().key_hit_scancode)
    }

    // = seg000:dd63 any_key_pressed — poll for user input. Runs the P-key
    // pause check, then reports whether the user pressed a key (ESC or any
    // buffered scancode) or newly pressed a mouse button. Returns true on input
    // (= DOS `stc` / carry set at loc_0ddae), false otherwise (= `or al,1` /
    // carry clear at seg000:ddab).
    //
    // DOS additionally calls process_frame_tasks on the no-input path
    // (= seg000:dda6); the port drives frame tasks from the wait loops
    // (tick_one_frame) instead, so this stays a pure poll. Mouse-button
    // detection is edge-triggered against prev_mouse_buttons (= DOS's `si`), so
    // a held button counts only once.
    pub fn any_key_pressed(&mut self) -> bool {
        // = seg000:dd63 call pause_if_p_key_pressed.
        self.pause_if_p_key_pressed();
        // = seg000:dd66 call kb_check_for_esc_key_hit; jz -> input.
        self.kb_check_for_esc_key_hit();
        let input = self.input.lock().unwrap();

        if input.kb_esc_was_hit != 0 {
            return true;
        }
        // = seg000:dd6b cmp key_hit_scancode,0; jnz -> input.
        if input.key_hit_scancode != 0 {
            return true;
        }
        // = seg000:dd7c INT 33,3 buttons; newly-pressed = buttons & ~prev.
        let buttons = input.mouse_buttons & 7;
        let newly_pressed = buttons & !self.prev_mouse_buttons;
        self.prev_mouse_buttons = buttons;
        newly_pressed != 0
    }

    // = seg000:de4e kb_clear_scancode — drop the buffered scancode.
    pub fn kb_clear_scancode(&mut self) {
        self.input.lock().unwrap().key_hit_scancode = 0;
    }

    // = seg000:de54 kb_check_for_esc_key_hit — set kb_esc_was_hit and clear the
    // buffer when the buffered scancode is ESC; otherwise clear the flag.
    pub fn kb_check_for_esc_key_hit(&mut self) {
        let mut input = self.input.lock().unwrap();
        input.kb_esc_was_hit = 0;
        if input.key_hit_scancode == SCANCODE_ESC {
            input.kb_esc_was_hit = 1;
            input.key_hit_scancode = 0;
        }
    }

    // = seg000:de68 kb_drain_and_clear — discard every buffered scancode, then
    // clear the whole key-down array (= the get_and_reset loop + clear_keyboard_array).
    pub fn kb_drain_and_clear(&mut self) {
        let mut input = self.input.lock().unwrap();
        input.key_hit_scancode = 0;
        input.kb_esc_was_hit = 0;
        input.kb_keys = [0; KB_KEYS_LEN];
    }

    // = seg000:de7b pause_if_p_key_pressed — when the P key is held and the
    // pause window is enabled, suspend the game until the player presses a
    // (non-ESC) key. The DOS routine also draws the "GAME PAUSED" window
    // (font + framebuffer swap, seg000:dea1..ded3); that rendering is not ported
    // yet, so the port pauses headlessly.
    pub fn pause_if_p_key_pressed(&mut self) {
        // = seg000:de7b cmp kb_keys_p,0; jz ret.
        if self.input.lock().unwrap().kb_keys[SCANCODE_P as usize] == 0 {
            return;
        }
        // = seg000:de82 cmp pause_enabled,0; jz ret.
        if self.pause_enabled == 0 {
            return;
        }
        // = seg000:de8c mov al,1; xchg al,[game_suspend_count] — suspend, saving
        // the previous nesting count to restore on resume.
        let saved = self.game_suspend_count;
        self.game_suspend_count = 1;
        // TODO: draw the "GAME PAUSED" window (seg000:dea1..ded3 font_draw…).
        // = seg000:ded6 wait for P to be released.
        while self.input.lock().unwrap().kb_keys[SCANCODE_P as usize] != 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // = seg000:dedd kb_drain_and_clear.
        self.kb_drain_and_clear();
        // = seg000:dee0 wait for a key; ESC (scancode 1) loops back rather than
        // resuming (it "removes the window"), any other key resumes.
        loop {
            let sc = self.get_and_reset_key_scancode();
            if sc == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            // = seg000:dee6 kb_drain_and_clear.
            self.kb_drain_and_clear();
            // = seg000:deed dec al; jz loc_0dee0 — scancode 1 (ESC) loops.
            if sc == SCANCODE_ESC {
                continue;
            }
            break;
        }
        // = seg000:defe mov [game_suspend_count],al — restore the suspend count.
        self.game_suspend_count = saved;
    }

    // = seg000:f08e clear_keyboard_array — zero the buffered scancode and the
    // whole key-down array.
    pub fn clear_keyboard_array(&mut self) {
        let mut input = self.input.lock().unwrap();
        input.key_hit_scancode = 0;
        input.kb_keys = [0; KB_KEYS_LEN];
    }
}
