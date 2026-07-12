use crate::{GameState, Rect, gfx, sprite_bank::ICONES};

const UI_HUD_HEAD_RECT: Rect = Rect {
    x0: 150,
    y0: 137,
    x1: 170,
    y1: 160,
};

const UI_HUD_HEAD_RECT_GAME_AREA_STRIP: Rect = Rect {
    x0: 150,
    y0: 137,
    x1: 170,
    y1: 147,
};

// The HUD head sits over the hud command menu, partially overlapping the game area.

impl GameState {
    // = seg000:1797 ui_hud_head_draw — open the ICONES bank, draw the shoulders
    // sprite (0x0f at 0x7e,0x94) and the head sprite (0x10 + ui_hud_head_index
    // at 0x96,0x89), then restore the previously-open bank.
    pub(crate) fn ui_hud_head_draw(&mut self) {
        // = seg000:17ad mov ax,10h; add al,[_byte_1F598_ui_hud_head_index].
        let head = 0x10 + self.ui_hud_head_index as u16;
        self.with_bank(ICONES, |s| {
            // = seg000:179e ax=0fh, dx=7eh, bx=94h, draw_sprite_clobbering_bx_dx.
            s.draw_active_bank_sprite(0x0f, 0x7e, 0x94);
            // = seg000:17b1 dx=96h, bx=89h, draw_sprite_clobbering_bx_dx.
            s.draw_active_bank_sprite(head, 0x96, 0x89);
        });
    }

    // = seg000:17be ui_hud_head_redraw — restore the background, redraw the HUD
    // head at the current ui_hud_head_index, then push ui_hud_head_rect to the
    // screen (skipped while rendering offscreen, where a transition presents the
    // final frame). Folding up restores the whole ui_hud_head_rect from fb2;
    // folding down restores only ui_hud_head_rect_game_area_strip from the saved
    // buffer, since only that strip overlaps the live game area — the rest sits
    // over the static command menu.
    fn ui_hud_head_redraw(&mut self) {
        // = seg000:17be set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        if self.ui_hud_head_animating_down {
            // = seg000:17d1 put_rect — restore the game-area strip from the saved
            //   buffer (seg001:cd9e).
            gfx::vga_put_rect(
                &mut self.framebuffer,
                &self.ui_hud_head_saved_strip,
                UI_HUD_HEAD_RECT_GAME_AREA_STRIP,
            );
        } else {
            // = seg000:17cc copy_rect_fb2_to_fb1 — restore the clean background
            //   from fb2.
            gfx::vga_copy_rect(
                &mut self.framebuffer,
                &self.framebuffer_saved,
                UI_HUD_HEAD_RECT,
            );
        }
        // = seg000:17df ui_hud_head_draw.
        self.ui_hud_head_draw();
        // = seg000:17e3 present_screen_rect.
        if !self.front_buffer_is_fb1() {
            gfx::vga_copy_rect(&mut self.screen, &self.framebuffer, UI_HUD_HEAD_RECT);
            self.send_frame_to_display();
        }
    }

    // = seg000:17e6 ui_hud_head_animate_up — raise the HUD head into view, one
    // fold frame per 8 ticks (ui_hud_head_index 0 = folded/hidden .. 0x0a = fully
    // raised).
    pub(crate) fn ui_hud_head_animate_up(&mut self) {
        // = seg000:17e6 cmp game_screen_mode_flags,0; jnz ret — room view only.
        if self.game_screen_mode_flags != 0 {
            return;
        }
        // = seg000:17ed..1801 raise ui_hud_head_index toward 0x0a, redrawing and
        //   waiting 8 ticks per step.
        while self.ui_hud_head_index != 0x0a {
            self.ui_hud_head_index += 1;
            self.ui_hud_head_redraw();
            self.wait_a_bit_for_head_fold();
        }
    }

    // = seg000:1803 ui_hud_head_animate_down — fold the HUD head out of view, one
    // frame per 8 ticks, before a full-screen overlay or view switch. Sets
    // ui_hud_head_animating_down so ui_hud_head_redraw restores the game-area strip
    // from the saved buffer rather than the whole head from fb2.
    pub(crate) fn ui_hud_head_animate_down(&mut self) {
        // = seg000:1803 cmp voice_subtitle_mode,0; jnz ret — keep it up while a
        //   voice/subtitle line is showing.
        if self.voice_subtitle_mode != 0 {
            return;
        }
        // = seg000:180a cmp ui_hud_head_index,0; jz ret — already hidden (leaves
        //   the flag untouched).
        if self.ui_hud_head_index == 0 {
            return;
        }
        // = seg000:1811 mov _byte_2C316_ui_hud_head_animating_down,1.
        self.ui_hud_head_animating_down = true;
        // = seg000:181e..1832 (inner ui_hud_head_animate_down loop) lower
        //   ui_hud_head_index toward 0, redrawing and waiting 8 ticks per step.
        while self.ui_hud_head_index != 0 {
            self.ui_hud_head_index -= 1;
            self.ui_hud_head_redraw();
            self.wait_a_bit_for_head_fold();
        }
        // = seg000:1819 dec _byte_2C316_ui_hud_head_animating_down.
        self.ui_hud_head_animating_down = false;
    }

    // = seg000:1834 ui_hud_head_save_rect — grab the ui_hud_head_rect_game_area_strip
    // out of fb1 into the saved buffer (seg001:cd9e), capturing the game-area
    // background before the head rises so the fold-down restore can put it back.
    pub(crate) fn ui_hud_head_save_rect(&mut self) {
        // = seg000:1834 si=0cd9eh, bp=ui_hud_head_rect_game_area_strip, es=fb1;
        //   vga_grab_rect.
        self.ui_hud_head_saved_strip =
            gfx::vga_grab_rect(&self.framebuffer, UI_HUD_HEAD_RECT_GAME_AREA_STRIP);
    }

    // = seg000:1843 ui_hud_head_animate_down_start — fold the HUD head down in two
    // quick steps before a view switch: frame 9, wait 8 ticks, frame 8; a no-op
    // when no head is shown. Unlike ui_hud_head_animate_down it skips the
    // intermediate frames, leaves ui_hud_head_index at 8, and does not set the
    // animating-down flag (so ui_hud_head_redraw restores from fb2).
    pub(crate) fn ui_hud_head_animate_down_start(&mut self) {
        // = seg000:1843 cmp ui_hud_head_index,0; jz ret.
        if self.ui_hud_head_index == 0 {
            return;
        }
        // = seg000:184a mov ui_hud_head_index,9; call ui_hud_head_redraw.
        self.ui_hud_head_index = 9;
        self.ui_hud_head_redraw();
        // = seg000:1852 mov ax,8; call wait_a_bit.
        self.wait_a_bit_for_head_fold();
        // = seg000:1858 mov ui_hud_head_index,8; jmp ui_hud_head_redraw.
        self.ui_hud_head_index = 8;
        self.ui_hud_head_redraw();
    }
}
