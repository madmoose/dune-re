//! The SEE DUNE MAP full-planet map view — the room verb (seg001:220c, handler
//! seg000:186b ui_toggle_room_view) that leaves the room screen for the whole-
//! planet map: the interpolated flat map (map_renderer.rs, segvga:1f4c
//! vga_draw_map_zoomed) in the full-map window (4,4)-(316,148), the vegetation
//! marks and location markers over it, the map main verb menu (EXIT MAPS /
//! CONTACT FREMEN TROOPS / SEE SPICE DENSITY / TAKE AN ORNITHOPTER / FIND
//! PROSPECTORS), and the first-visit "Map to command rallied troops" popup.
//!
//! This is the troop-command screen. The troop icons render through the
//! layered-icon system in troop_icons.rs (the "particle system" the night
//! attack scene reuses; see attack/mod.rs); clicking an icon selects the
//! troop (the rotating highlight ring), right-clicking toggles its info
//! panel, and clicking a location marker opens the location info popup with
//! its GO THERE command menu. The info/location panels scale in and out with
//! an XOR outline animation (xor_rect_outline_advance / _reverse).
//!
//! The CONTACT FREMEN TROOPS / GIVE ORDERS TO TROOP verb
//! (menu_callback_choice_map_main_contact_fremen_troops) selects a troop, opens
//! its contact verb menu (map_open_troop_contact_menu) and puts the contact
//! dialogue popup up over the map (map_open_troop_contact_dialogue): a panel
//! opposite the troop's icon, the speaker's portrait re-anchored into the head
//! box on the left (talking_head.rs draw_talking_head_in_box) and the troop's
//! dialogue line, spoken and subtitled, on the right (the seg001:2244 subtitle
//! layout in subtitle.rs).
//!
//! ASK FOR MORE INFORMATION asks the troop for its next line, and CHANGE /
//! SELECT TROOP OCCUPATION opens the occupation submenu its current
//! occupation class calls for.
//!
//! The SPECIALIZE IN … and ASSEMBLY WIND-TRAP verbs reassign the troop for
//! real (troops.rs troop_set_occupation), ask it, and take the change back if
//! it refuses.
//!
//! Still stubbed here: ESPIONAGE / ATTACK / GO & SEARCH FOR EQUIPMENT, which
//! also MOVE the troop through the unported troop-command core
//! (troop_location_082da / 084a6), MODIFY EQUIPMENT and MOVE TROOP with their
//! equipment spinners, the
//! GO THERE launch's CALL A WORM branch, the water/spice popup extra
//! (loc_0605c), popup dragging, and the spice-density overlay (data_046eb bit
//! 0x40).

use crate::{
    GameState, Rect, cmd,
    game_ui::{MouseHandlers, NAV_PANEL_ALT},
    gfx,
    locations::location_index_from_ptr,
    menu_defs::{
        self, CMD_GREY, MenuRef,
        MenuRef::{
            MenuOccupationForArmyTroop, MenuOccupationForEcologyTroop,
            MenuOccupationForEspionageTroop, MenuOccupationForSpiceTroop,
            MenuSelectTroopOccupation,
        },
    },
    rect::rect,
};

// = seg001:1482 full_map_view_rect — the SEE DUNE MAP window (4,4)-(316,148),
// copied into data_046e3_rect (map_view_rect) by the open transition
// callback. vga_draw_map_zoomed fills exactly this window: 36 bands of 4
// rows, 312 px wide.
const FULL_MAP_VIEW_RECT: Rect = rect(4, 4, 316, 148);

/// = seg001:194a data_0194a — the rallied-troops title popup's panel record:
/// rect (10,10)-(190,64), frame colour 0xf5 (+8), fill colour 0xfb (+9). The
/// record's seg001 offset doubles as the popup identity in map_popup_ptr.
pub(crate) const MAP_POPUP_RALLIED: u16 = 0x194a;
pub(crate) const RALLIED_POPUP_RECT: Rect = rect(10, 10, 190, 64);

/// = seg001:18df data_018df — the troop info panel record: the rect is
/// written at open time (loc_05f25), frame colour 0xfb (+8), fill colour
/// 0xf0 (+9). The record's seg001 offset is the popup identity.
pub(crate) const MAP_POPUP_TROOP_INFO: u16 = 0x18df;

/// = seg001:1668 data_01668 — the location info panel record (frame 0xf8,
/// fill 0x10). The record's seg001 offset is the popup identity.
pub(crate) const MAP_POPUP_LOCATION: u16 = 0x1668;

/// = seg001:11c1/11c3 data_011c1/data_011c3 — the spice-density overlay
/// panel's screen origin (75, 15).
const SPICE_OVERLAY_PANEL_POS: (i16, i16) = (75, 15);

/// = seg001:4710 data_04710 — the overlay panel's rect doubles as its popup
/// identity in map_popup_ptr (seg000:5535).
pub(crate) const MAP_POPUP_SPICE_OVERLAY: u16 = 0x4710;

/// = seg001:18e9 troop_contact_text_panel_record — the contact dialogue
/// popup's panel record, frame colour 0xf5 (+8), fill 0xfb (+9). The record's
/// seg001 offset is the popup identity in map_popup_ptr.
pub(crate) const MAP_POPUP_TROOP_CONTACT: u16 = 0x18e9;

/// = the compiled-in rect of that record: (5,5)-(232,72). Only the y pair is
/// rewritten per open (map_draw_troop_contact_popup picks the half of the
/// screen the troop's icon is not in).
pub(crate) const TROOP_CONTACT_POPUP_RECT: Rect = rect(5, 5, 232, 72);

/// = seg001:2244 — the contact subtitle's layout descriptor: 153x63, its
/// origin written per open. subtitle_setup_layout picks it for every line
/// presented while the full-map view owns the screen (seg000:8cd8).
pub(crate) const TROOP_CONTACT_SUBTITLE_SIZE: (i16, i16) = (153, 63);

/// The GO THERE menu variant map_click_location_marker folds in.
#[derive(Clone, Copy)]
enum MoveMenu {
    /// = seg001:20da menu_multiple_move_to_location_flying_an_orni.
    Orni,
    /// = seg001:20e6 menu_multiple_move_to_location_riding_a_worm.
    Worm,
}

/// = seg001:1a9e mouse_handlers_01a9e — the full-map view's MouseHandlers
/// record, installed by ui_main_view_map_interface. The release/drag pair
/// moves a dragged popup panel (not ported); rmb_release/rmb_drag are the
/// no-op loc_00f66.
pub(crate) static DUNE_MAP_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::dune_map_mouse_idle,
    lmb: GameState::dune_map_mouse_lmb,
    rmb: GameState::dune_map_mouse_rmb,
    release: GameState::dune_map_mouse_release,
    rmb_release: GameState::dune_map_mouse_noop,
    drag: GameState::dune_map_mouse_drag,
    rmb_drag: GameState::dune_map_mouse_drag_noop,
};

// = seg001:1aac mouse_handlers_01aac — the MOVE TROOP destination-pick mode:
// the map hover keeps working, the LMB picks a destination, everything else
// is a no-op.
pub(crate) static MOVE_TROOP_MOUSE_HANDLERS: MouseHandlers = MouseHandlers {
    idle: GameState::dune_map_mouse_idle,
    lmb: GameState::move_troop_pick_lmb,
    rmb: GameState::dune_map_mouse_noop,
    release: GameState::dune_map_mouse_noop,
    rmb_release: GameState::dune_map_mouse_noop,
    drag: GameState::dune_map_mouse_drag_noop,
    rmb_drag: GameState::dune_map_mouse_drag_noop,
};

impl GameState {
    // = seg000:5a1a ui_show_globe_map_view — leave the room view and bring up
    // the full DUNE MAP view (the else-branch of ui_toggle_room_view).
    pub(crate) fn ui_show_globe_map_view(&mut self) {
        // = seg000:5a1a voice_subtitle_mode = 1.
        self.voice_subtitle_mode = 1;
        // = seg000:5a1f call ui_teardown_room_view.
        self.ui_teardown_room_view();
        // = seg000:5a22 call set_zoomed_globe_pos_from_map_position — centre
        //   the map on the player.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:5a25 bp=callback_transition_05a56; al=34h; dx=0ffffh; call
        //   transition — build the map view offscreen and dissolve it in. (The
        //   dx direction byte is only read by the effect-4 curtain; 0x34
        //   ignores it, so the port's fixed dl=0 matches.)
        self.transition(0x34, |s| s.callback_transition_dune_map_view());
        // = seg000:5a30 cmp map_view_reentry_count,0; jnz — the rallied-troops
        //   popup only on the first visit.
        if self.map_view_reentry_count == 0 {
            self.map_show_rallied_troops_popup();
        }
        // = seg000:5a3a jmp ui_hud_head_animate_up.
        self.ui_hud_head_animate_up();
    }

    // = seg000:5a56 callback_transition_05a56 — the SEE DUNE MAP view builder,
    // run inside the transition (front buffer redirected to fb1).
    fn callback_transition_dune_map_view(&mut self) {
        // = seg000:5a56 cmp data_046eb,0; js — already in the map view: only
        //   redraw.
        if self.data_046eb & 0x80 != 0 {
            self.ui_main_view_map_interface();
            return;
        }
        // = seg000:5a5d call dismiss_stacked_overlays.
        self.dismiss_stacked_menus();
        // = seg000:5a60 call loc_04aca — data_011ca = 1 (suspend the pending
        //   room-swap machinery while the map owns the screen).
        self.data_011ca = 1;
        // = seg000:5a63 call remove_globe_frame_tasks.
        self.remove_globe_frame_tasks();
        // = seg000:5a66..5a6c add troop_icon_anim_task (interval 15).
        self.arm_troop_icon_anim_task();
        // = seg000:5a6f..5a75 data_046e3_rect = full_map_view_rect.
        self.map_view_rect = FULL_MAP_VIEW_RECT;
        // = seg000:5a78 call draw_map_view_border.
        self.draw_map_view_border();
        // = seg000:5a7b call ui_hud_head_draw.
        self.ui_hud_head_draw();
        // = seg000:5a7e data_046eb = 0x80 — the full-map view owns the screen.
        self.data_046eb = 0x80;
        // = seg000:5a83 call loc_0ad5e — re-pick the background music for the
        //   new screen mode.
        self.update_room_music();
        // = seg000:5a86 troop_icon_draw_order_func = troop_icons_pick_next_
        //   by_depth — back-to-front icon layering down the map.
        self.troop_icon_draw_by_depth = true;
        // = seg000:5a8c..5a92 install ui_main_view_map_interface as the
        //   main-view drawing function and run it.
        self.current_main_view_drawing_function = Some(GameState::ui_main_view_map_interface);
        self.ui_main_view_map_interface();
        // = seg000:5a94 call ui_set_and_draw_frieze_sides_map.
        self.ui_set_and_draw_frieze_sides_map();
        // = seg000:5a97 jmp loc_0d712 — install and draw the alternate
        //   (map-scroll) nav panel.
        self.ui_install_nav_panel(&NAV_PANEL_ALT);
    }

    // = seg000:5a9a ui_main_view_map_interface — compose the full DUNE MAP
    // view into fb1. Installed as the main-view drawing function while the
    // view is up (map_refresh_main_view re-runs it after a scroll/recentre).
    pub(crate) fn ui_main_view_map_interface(&mut self) {
        // = seg000:5a9a call set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // = seg000:5a9d call loc_05b8d — restore rect (data_d83c_rect) and
        //   sprite clip rect = the map window. The port passes clip rects per
        //   draw call (see map_view_clip_rect).
        // = seg000:5aa0..5aa6 force data_046eb = 0x80 around the draws,
        //   keeping the entry value (the 0x40 spice sub-mode restores below).
        let saved_046eb = std::mem::replace(&mut self.data_046eb, 0x80);
        // = seg000:5aa7 call map_draw_zoomed_globe — the interpolated full-
        //   planet map (MapRenderer).
        self.map_draw_zoomed_globe();
        // = seg000:5aaa call open_onmap_resource — the on-map marker sprites.
        self.open_onmap_spritesheet();
        // = seg000:5aad call map_build_and_draw_location_markers — the
        //   vegetation marks, then the location markers (ONMAP base 0x7a).
        self.map_build_and_draw_location_markers();
        // = seg000:5ab0 call map_draw_player_position_sprite.
        self.map_draw_player_position_sprite();
        // = seg000:5ab3 call copy_active_framebuffer_to_framebuffer_2 — the
        //   clean map snapshot the popup dismissals restore from.
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:5ab6 troop_icon_count = 0 (the focused slots are cleared
        //   with the list, port-side, so they cannot dangle); 5abc call
        //   map_spawn_troop_icons.
        self.troop_icons.clear();
        self.troop_icon_focused = [None; 2];
        self.map_spawn_troop_icons();
        // = seg000:5abf/5ac2 si = data_046e3_rect; call troop_icons_update_
        //   dirty_rect — draw the icons over the snapshot and present the
        //   map window.
        let r = self.map_view_rect;
        self.troop_icons_update_dirty_rect(r);
        // = seg000:5ac5 call map_setup_main_menu.
        self.map_setup_main_menu();
        // = seg000:5ac8/5ac9 restore the entry data_046eb.
        self.data_046eb = saved_046eb;
        // = seg000:5acc..5ad0 bit 0x40 re-enters the spice-density overlay.
        if self.data_046eb & 0x40 != 0 {
            self.map_enter_spice_density_overlay();
        }
        // = seg000:5ad3 install mouse_handlers_01a9e.
        self.active_mouse_handlers = &DUNE_MAP_MOUSE_HANDLERS;
        // = seg000:5ad9 nav rect = the map window.
        self.set_mouse_nav_rect(self.map_view_rect);
    }

    // = seg000:53f1 menu_callback_choice_map_main_see_spice_density — the map
    // main menu's SEE SPICE DENSITY verb: a toggle. With the overlay up it
    // leaves it; otherwise it raises it with the map main menu as the panel's
    // menu (data_04720 = 0x1e6e).
    pub(crate) fn menu_callback_choice_map_main_see_spice_density(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:53f1 data_04722 = 0 — the spice-density mode (not the
        //   ecology one).
        self.map_overlay_mode = 0;
        // = seg000:53f6..53fd test data_046eb,40h; jnz loc_058fa.
        if self.data_046eb & 0x40 != 0 {
            self.map_leave_spice_density_overlay();
            return;
        }
        // = seg000:5400 data_04720 = menu_map_main; fall into
        //   map_enter_spice_density_overlay.
        self.map_enter_spice_density_overlay();
    }

    // = seg000:5406 map_enter_spice_density_overlay — raise the spice-density
    // overlay at its home position: a small map window inside a panel showing
    // each location's spice field shaded by its density. The panel origin
    // reloads from the data_011c1/011c3 home words and the open popups close.
    // The SEE SPICE DENSITY verb falls in here (loc_05400) and the map
    // recompose re-enters via the bit-6 check at seg000:5ad0.
    //
    // The panel origin is data_04710/04712; the map window sits at +(5, 7)
    // and is 0xa0 x 0x59. The window content is the MAP2.HSQ spice layer run
    // through the same windowed row fill as the terrain map (DOS swaps
    // res_map_seg around the call, seg000:5487), then rendered through the
    // per-field colour table loc_057e5 builds (vga_draw_landscape).
    pub(crate) fn map_enter_spice_density_overlay(&mut self) {
        // = seg000:5406..5412 the panel origin from data_011c1/011c3. DOS
        //   persists panel drags back into the home words (seg000:59d1); the
        //   drag is not ported, so the home stays the static (75, 15).
        self.map_overlay_panel_pos = SPICE_OVERLAY_PANEL_POS;
        // = seg000:5416..541c close the popups the overlay replaces.
        self.map_close_rallied_troops_popup();
        self.map_close_location_troop_popup();
        self.map_close_troop_info_popup();
        self.map_enter_spice_density_overlay_in_place();
    }

    // = seg000:541f map_enter_spice_density_overlay_in_place — raise the
    // overlay at the current panel origin, left wherever the last popup
    // staged it: the contact popup parks it at (92, 30) — or (92, 14) with
    // the popup in the lower half (seg000:7a15) — which is where the
    // prospector scene's overlay appears. Reached from the move-order
    // caption tail (seg000:8134); the nav-panel centre button also jumps
    // here in the overlay sub-mode (seg000:5b0a, unported).
    pub(crate) fn map_enter_spice_density_overlay_in_place(&mut self) {
        // = seg000:541f..542b the overlay opens on the map position the
        //   view is centred at (globe_param_3/4 = the zoomed globe position).
        self.globe_param_3 = self.zoomed_globe_longitude;
        self.globe_param_4 = self.zoomed_globe_latitude;
        self.map_draw_spice_density_overlay();
    }

    // = seg000:542f loc_0542f — draw (or redraw) the overlay.
    pub(crate) fn map_draw_spice_density_overlay(&mut self) {
        let (px, py) = self.map_overlay_panel_pos;
        // = seg000:542f/5435 data_046fc = 0; current_bubble_layout_ptr = 0.
        self.subtitle_bubble = None;
        // = seg000:543e..5456 the map window: the panel origin + (5, 7),
        //   0xa0 x 0x59.
        self.map_view_rect = rect(px + 5, py + 7, px + 5 + 160, py + 7 + 89);
        // = seg000:5459..5465 the panel rect's far corner: the window plus a
        //   (5, 0xc) border.
        self.map_overlay_panel_rect = rect(px, py, px + 5 + 160 + 5, py + 7 + 89 + 0xc);
        // = seg000:5468..5471 draw the panel frame sprite 0x8d from ONMAP at
        //   the panel origin.
        self.open_onmap_spritesheet();
        let (sx, sy) = (px, py);
        let full = rect(0, 0, 320, 200);
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_sprite_from_sheet_clipped(sheet, 0x8d, sx, sy, full);
        });
        // = seg000:5474/5477 fb1 active + the sprite clip rect.
        self.set_fb1_as_active_framebuffer();
        // = seg000:547a..5497 the window content: MAP2's rows (res_map_seg
        //   swapped) with data_046eb = 0x40 so map_draw_zoomed_globe fills the
        //   row buffer without blitting, around the loc_0b69a position swap.
        let saved_046eb = std::mem::replace(&mut self.data_046eb, 0x40);
        self.map_overlay_swap_position();
        let map2 = self.map2.clone();
        let (rows, width, height, top_lat) = self.map_fill_window_rows_from(Some(&map2));
        // = seg000:549e call loc_058e4 — the landscape render through the
        //   per-location colour table.
        let xlat = self.build_spice_density_xlat();
        let r = self.map_view_rect;
        crate::gfx::vga_draw_landscape(self, &rows, width, height, r.x0, r.y0, top_lat, &xlat);
        // = seg000:54a1 call load_icones_sprites — the windowed-view marker
        //   sprites (base 0x3a) live in ICONES; the legend below reopens
        //   ONMAP for its ramp bars.
        self.open_icones_spritesheet();
        // = seg000:54a4..54aa the location markers and the overlay's legend
        //   strip and content (loc_05605/loc_0563e).
        self.map_build_and_draw_location_markers();
        self.map_overlay_draw_legend_strip();
        self.map_overlay_draw_legend();
        // = seg000:54ad data_02772 = 0x5555 — the dotted line pattern for the
        //   box below.
        self.line_pattern = 0x5555;
        // = seg000:54b3..551a the "you are here" box: the player position
        //   projected into the window, a 0x50 x 0x28 box clamped to the
        //   window, outlined in 0xfb.
        let (cx, cy) = self.map_position_to_screen(self.globe_param_3, self.globe_param_4);
        let (mut x0, mut x1) = (cx - 0x28, cx + 0x27);
        let (mut y0, mut y1) = (cy - 0x14, cy + 0x13);
        let (wx0, wx1) = (px + 5, px + 5 + 0x9f);
        let (wy0, wy1) = (py + 7, py + 7 + 0x58);
        x0 = x0.clamp(wx0, wx1);
        x1 = x1.clamp(wx0, wx1);
        y0 = y0.clamp(wy0, wy1);
        y1 = y1.clamp(wy0, wy1);
        if y0 != y1 && x0 != x1 {
            self.draw_rect_outline(x0, y0, x1, y1, 0xfb);
        }
        // = seg000:551d data_02772 = 0xffff — the solid pattern back.
        self.line_pattern = 0xffff;
        // = seg000:551d..5535 the panel becomes an open popup so a click
        //   inside it routes to the overlay, not the map: the primary slot
        //   when it is free (or already the overlay's), else the secondary
        //   one — that is how the overlay coexists with the troop-contact
        //   popup during the prospector scene.
        let secondary = self.map_popup_ptr != 0 && self.map_popup_ptr != MAP_POPUP_SPICE_OVERLAY;
        if secondary {
            self.map_popup2_ptr = MAP_POPUP_SPICE_OVERLAY;
        } else {
            self.map_popup_ptr = MAP_POPUP_SPICE_OVERLAY;
        }
        // = seg000:553c..554a the pending menu (data_04720) folds in with
        //   effect 6. The port's callers push their own menus.
        // = seg000:554e..5558 the primary-slot case draws the overlay's own
        //   decorations (loc_062f2 + loc_0813e: the spice-field legend and
        //   the equipment row — unported); the secondary-slot case draws the
        //   player-position sprite.
        if secondary {
            self.map_draw_player_position_sprite();
        }
        // = seg000:555b/555e present the panel rect.
        let panel = self.map_overlay_panel_rect;
        self.present_screen_rect(panel);
        // = seg000:5561 call loc_0b69a — swap the position back.
        self.map_overlay_swap_position();
        // = seg000:5564..5572 data_046eb = 0xc0 (the full map plus the
        //   overlay bit) and the map window returns to the full-map rect.
        self.data_046eb = saved_046eb | 0xc0;
        self.map_view_rect = FULL_MAP_VIEW_RECT;
        // = seg000:5575/5578 the mouse nav rect is the panel.
        self.set_mouse_nav_rect(panel);
    }

    // = seg000:5605 loc_05605 — the legend strip background: fill the panel
    // band below the map window ((px+6, py+0x62)-(x1-6, y1-2)) with 0xf5 and
    // select the small font for the label. DOS also resets the legend hover
    // cache here (data_04724 = 0xff); the hover highlighter it serves
    // (seg000:5744..57e0, the density-tick XOR box and label recolour under
    // the mouse) is not ported.
    fn map_overlay_draw_legend_strip(&mut self) {
        let (px, py) = self.map_overlay_panel_pos;
        let r = self.map_overlay_panel_rect;
        // = seg000:5605..562c the 0xf5 fill of the strip rect, into the
        //   active framebuffer.
        let dest = self.active_fb();
        crate::gfx::vga_fill_rect(
            self,
            dest,
            (px + 6) as u16,
            (py + 0x62) as u16,
            (r.x1 - 6) as u16,
            (r.y1 - 2) as u16,
            0xf5,
        );
        // = seg000:5635 call font_select_small_font.
        self.font_select_small_font();
    }

    // = seg000:563e loc_0563e — the legend content on the strip: the
    // "  SPICE DENSITY  " label (phrase 0x65, colour word 0xf5fe) at
    // (px+6, py+0x62), the '-' glyph at x = px+6+0x53 with the '+' 0x41
    // past it, then the two ONMAP density-ramp bar sprites 0x80/0x81 at
    // (px+0x5f, py+0x63) and 0x3c further right. The data_04722 != 0
    // ecology variant ("  TROOP OCCUPATION  ", seg000:568c) is not
    // reachable from the ported callers. TODO.
    fn map_overlay_draw_legend(&mut self) {
        let (px, py) = self.map_overlay_panel_pos;
        // = seg000:5652..5656 the label.
        self.font_draw_phrase_or_command_string_with_color_at_pos(
            0x65,
            0xf5fe,
            (px + 6) as u16,
            (py + 0x62) as u16,
        );
        // = seg000:5659..5662 the '-' end cap: only the pen x moves (the y
        //   stays on the label row).
        self.font_state.x = (px + 6 + 0x53) as u16;
        self.font_draw_glyph(b'-');
        // = seg000:5666..566d the '+' end cap, 0x41 past the advanced pen.
        self.font_state.x += 0x41;
        self.font_draw_glyph(b'+');
        // = seg000:5671..5689 the density-ramp bars: ONMAP sprites 0x80 and
        //   0x81 (draw_sprite, the unclipped variant).
        self.open_onmap_spritesheet();
        let full = rect(0, 0, 320, 200);
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_sprite_from_sheet_clipped(sheet, 0x80, px + 0x5f, py + 0x63, full);
            s.draw_sprite_from_sheet_clipped(sheet, 0x81, px + 0x5f + 0x3c, py + 0x63, full);
        });
    }

    // = seg000:b69a loc_0b69a — exchange the live map position (zoomed_globe_
    // longitude/latitude) with the overlay's (globe_param_3/4). Called before
    // and after the overlay draw, so the overlay renders its own position and
    // the map view keeps its own.
    fn map_overlay_swap_position(&mut self) {
        std::mem::swap(&mut self.zoomed_globe_longitude, &mut self.globe_param_3);
        std::mem::swap(&mut self.zoomed_globe_latitude, &mut self.globe_param_4);
    }

    // = seg000:57e5 build_spice_density_xlat — build the overlay's 256-entry
    // palette-remap table: every MAP2 spice-field id maps to a colour,
    // defaulting to the backdrop 0x70. With data_04722 == 0 (the spice-
    // density mode) each visible location paints its own field: status bit 6
    // clear takes the flat marker colour (0x75, or 0x78 in monotone mode),
    // bit 6 set the density-ramp shade 0x50 + (spice_density >> 4).
    fn build_spice_density_xlat(&mut self) -> [u8; 256] {
        // = seg000:57f1..57f8 fill with 0x70.
        let mut xlat = [0x70u8; 256];
        // = seg000:57fa cmp data_04722,0; jnz loc_0583f — the other mode (the
        //   ecology/vegetation view) is not reachable from the ported
        //   callers. TODO.
        for loc in self.locations.iter() {
            // = seg000:5808..580d test status,80h — a hidden location paints
            //   nothing.
            if loc.status & 0x80 != 0 {
                continue;
            }
            // = seg000:580f bl = spice_field_id — the MAP2 byte this
            //   location owns.
            let field = loc.spice_field_id as usize;
            // = seg000:5812..582e status bit 6 clear takes the flat colour
            //   (the jz at 5821 stores the 0x75 set up before the popf);
            //   bit 6 set the density shade.
            let colour = if loc.status & 0x40 == 0 {
                // = seg000:5815..581e 0x75, or 0x78 with the MON cmd arg.
                if self.cmd_args_memory & 2 != 0 {
                    0x78
                } else {
                    0x75
                }
            } else {
                // = seg000:5823..582e al = 0x50 + (spice_density >> 4).
                0x50 + (loc.spice_density >> 4)
            };
            xlat[field] = colour;
        }
        xlat
    }

    // = seg000:58fa loc_058fa — leave the spice-density overlay: drop the
    // sub-mode bit, take the panel back out of the popup slot, repaint the
    // map under it and restore the map view's mouse rect.
    pub(crate) fn map_leave_spice_density_overlay(&mut self) {
        // = seg000:58fa/58ff test data_046eb,40h; jz ret.
        if self.data_046eb & 0x40 == 0 {
            return;
        }
        // = seg000:5904 and data_046eb,0bfh.
        self.data_046eb &= 0xbf;
        // = seg000:5909..5917 clear whichever popup slot holds the panel.
        if self.map_popup_ptr == MAP_POPUP_SPICE_OVERLAY {
            self.map_popup_ptr = 0;
        } else if self.map_popup2_ptr == MAP_POPUP_SPICE_OVERLAY {
            self.map_popup2_ptr = 0;
        }
        // = seg000:5919 call troop_icons_update_dirty_rect — repaint the map
        //   under the panel.
        let r = self.map_overlay_panel_rect;
        self.troop_icons_update_dirty_rect(r);
        // = seg000:591c call loc_05ad9 — the map view's nav rect back.
        self.set_mouse_nav_rect(self.map_view_rect);
    }

    // = seg000:5d50 location_mark_map_view_dirty — a location changed in a
    // way the map shows (revealed, battle over, ...): while a map view is up
    // and the location is visible and not hidden, bump the data_046ec dirty
    // counter so the next event-run refresh repaints the view. (DOS tests
    // visibility with location_visible_on_map; the port tests the visible-
    // marker list that predicate builds.)
    pub(crate) fn location_mark_map_view_dirty(&mut self, li: usize) {
        // = seg000:5d52..5d5d the hidden / no-map-view gates.
        if self.locations[li].status & 0x80 != 0 || self.data_046eb == 0 {
            return;
        }
        // = seg000:5d5f..5d66 call location_visible_on_map; jb skip.
        if self
            .visible_location_markers
            .iter()
            .any(|m| m.location_index as usize == li)
        {
            self.spice_density_overlay_dirty = self.spice_density_overlay_dirty.wrapping_add(1);
        }
    }

    // = seg000:5d44 location_mark_map_and_minimap_dirty — the flight
    // variant: while a travel mode is active (game_screen_mode_flags bits
    // 0-1) also flag the minimap redraw.
    pub(crate) fn location_mark_map_and_minimap_dirty(&mut self, li: usize) {
        if self.game_screen_mode_flags & 3 != 0 {
            self.travel_minimap_state |= 1;
        }
        self.location_mark_map_view_dirty(li);
    }

    // = seg000:5d6d map_view_refresh_after_events — refresh the map/globe
    // main view after a time-period event run (the scheduler's tail, gated
    // on the data_046ec dirty counter, which this resets). data_046eb > 0:
    // the installed main-view redraw; bit 0x80: the full DUNE MAP recompose
    // (loc_05d82); 0: no map view up, nothing to do.
    pub(crate) fn map_view_refresh_after_events(&mut self) {
        // = seg000:5d6d data_046ec = 0.
        self.spice_density_overlay_dirty = 0;
        // = seg000:5d72..5d79 the data_046eb routing.
        if self.data_046eb & 0x80 == 0 {
            if self.data_046eb == 0 {
                return;
            }
            // = seg000:5d7b/5d7e restore the cursor and jump through the
            //   installed drawing function (map_view_redraw / travel_
            //   minimap_redraw).
            self.call_restore_cursor();
            let redraw = self
                .current_main_view_drawing_function
                .expect("map_view_refresh_after_events with no drawing function installed");
            redraw(self);
            return;
        }
        // = seg000:5d82 loc_05d82 — the full-map recompose, the same draw
        //   sequence as ui_main_view_map_interface without the menu setup.
        self.set_fb1_as_active_framebuffer();
        self.call_restore_cursor();
        // = seg000:5d88 call loc_05b8d — restore rect + sprite clip rect =
        //   the map window; the port passes clip rects per draw call.
        // = seg000:5d8b..5d92 force data_046eb = 0x80 around the draws and
        //   save the contact troop (data_046ef) for the focus restore.
        let saved_046eb = std::mem::replace(&mut self.data_046eb, 0x80);
        let saved_contact = self.map_contact_troop;
        // = seg000:5d96..5da2 the map, the markers, the player sprite, and
        //   the fresh fb2 snapshot.
        self.map_draw_zoomed_globe();
        self.open_onmap_spritesheet();
        self.map_build_and_draw_location_markers();
        self.map_draw_player_position_sprite();
        self.copy_active_framebuffer_to_framebuffer_2();
        // = seg000:5da5..5db1 rebuild the troop icons and present the window.
        self.troop_icons.clear();
        self.troop_icon_focused = [None; 2];
        self.map_spawn_troop_icons();
        let r = self.map_view_rect;
        self.troop_icons_update_dirty_rect(r);
        // = seg000:5db4..5db9 re-focus the contacted troop's highlight ring.
        if let Some(ti) = saved_contact {
            self.map_focus_troop_icon(ti);
        }
        // = seg000:5dbc call loc_01c18 — the open popups' content.
        self.redraw_period_sensitive_view_content();
        // = seg000:5dbf/5dc0 restore the entry data_046eb.
        self.data_046eb = saved_046eb;
        // = seg000:5dc3..5dc7 bit 0x40 redraws the spice-density overlay.
        if self.data_046eb & 0x40 != 0 {
            self.map_draw_spice_density_overlay();
        }
        // = seg000:5dca jmp open_onmap_resource.
        self.open_onmap_spritesheet();
    }

    // = seg000:8064 menu_callback_choice_multiple_move_troop — the MOVE
    // TROOP / CHANGE DESTINATION verb: switch the mouse into the
    // destination-pick mode, show the instruction caption and push the pick
    // menu (the plain Cancel menu, or the prospector's multi-destination
    // menu for troops[2]).
    pub(crate) fn menu_callback_choice_multiple_move_troop(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:8064/8067 install mouse_handlers_01aac.
        self.active_mouse_handlers = &MOVE_TROOP_MOUSE_HANDLERS;
        // = seg000:806a call contact_verb_troop.
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        // = seg000:806d cmp si,troops[2] — the prospector picks up to four
        //   destinations; everyone else exactly one.
        if ti != 2 {
            // = seg000:8073..8076 the COMMAND 0x54 caption.
            self.move_troop_show_instruction_caption(cmd::SHOW_ME_WHERE_YOU_WANT_ME_TO_GO);
            // = seg000:8079..807f bp = menu_multiple_cancel, bx = loc_0824d
            //   (move_troop_cleanup); jmp loc_0d323.
            self.menu_cancel.records = menu_defs::MENU_CANCEL.records.to_vec();
            self.stage_command_submenu(MenuRef::MenuCancel, GameState::move_troop_cleanup);
            return;
        }
        // = seg000:8082..808c the working copy of the prospector queue
        //   (three words; the count scan below covers four — the DOS
        //   asymmetry leaves the fourth working word stale).
        self.prospector_pick_queue[..3].copy_from_slice(&self.prospector_destinations[..3]);
        // = seg000:808d..809c the entry count: the first zero word of the
        //   four, 3 when none is zero.
        self.prospector_pick_count = self
            .prospector_pick_queue
            .iter()
            .position(|&w| w == 0)
            .unwrap_or(3) as u8;
        // = seg000:80a0..80a6 the COMMAND 0x55 caption + the menu greys.
        self.move_troop_show_instruction_caption(cmd::SHOW_ME_3_SIETCHS_WHERE_YOU_WANT_ME_TO_G);
        self.move_prospectors_configure_menu();
        // = seg000:80a9 jmp loc_0d32f — transition + insert + fold (without
        //   the d323 hover highlight). bx = loc_0824d (move_troop_cleanup).
        self.screen_overlay_request_transition();
        self.menu_stack_push(
            MenuRef::MenuMoveProspectors,
            Some(GameState::move_troop_cleanup),
        );
        self.play_pending_panel_fold();
    }

    // = seg000:80df move_troop_show_instruction_caption — show the
    // move-order instruction (a COMMAND id) as a voiced subtitle in the
    // contact popup's text box, then flip the map into the spice-density
    // overlay sub-mode and drop the talking-head HUD element. Besides the
    // move-troop verbs, show_voice_subtitle jumps here through its
    // data_046eb bit-6 short-circuit (seg000:88c7).
    pub(crate) fn move_troop_show_instruction_caption(&mut self, id: u16) {
        // = seg000:80e0 call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:80e3 call subtitle_draw_troop_popup_background — the text
        //   box wipe; the port's popup subtitle path repaints the box inside
        //   the bubble draw.
        // = seg000:80e7..80fd the pen shifts right 0x26 (when x >= 0x32) and
        //   the box narrows to 0x19 rows; the box height is a port const, so
        //   only the pen shift is modelled.
        let saved_pos = self.map_contact_subtitle_pos;
        if saved_pos.0 >= 0x32 {
            self.map_contact_subtitle_pos.0 += 0x26;
        }
        // = seg000:8102 call loc_09f82 — the subtitle font.
        self.font_state.color = 0x00f0;
        self.font_select_tall_font();
        // = seg000:8105/8106 call show_voice_subtitle.
        self.show_voice_subtitle(id);
        // = seg000:8109..811b the caption's voice (unless a dialogue is
        //   active, data_04774): the id rebased onto the shared instruction
        //   bank — play_dialogue_voc subtracts the Fremen voc base right
        //   back.
        if !self.is_dialogue_active {
            self.current_subtitle_id = self
                .current_subtitle_id
                .wrapping_add(0x10a)
                .wrapping_add(self.voc_base(0x0e));
            self.play_dialogue_voc();
        }
        // = seg000:811e..8126 restore the pen, back to fb1.
        self.map_contact_subtitle_pos = saved_pos;
        self.set_fb1_as_active_framebuffer();
        // = seg000:8129..812f data_04720 = data_018f3 (the pending panel
        //   menu; the port's callers push their own) and data_04722 = 0 (the
        //   spice-density mode).
        self.map_overlay_mode = 0;
        // = seg000:8134 call map_enter_spice_density_overlay_in_place — the
        //   overlay comes up at the panel origin the contact popup staged,
        //   not the home position.
        self.map_enter_spice_density_overlay_in_place();
        // = seg000:8137 drop the talking-head HUD element.
        self.ui_elements[18].flags = 0;
    }

    // = seg000:80ac move_prospectors_configure_menu — grey ADD A DESTINATION
    // once the working queue holds three destinations.
    fn move_prospectors_configure_menu(&mut self) {
        let full = self.prospector_pick_count.wrapping_sub(1) >= 2;
        let id = cmd::ADD_A_DESTINATION;
        self.menu_move_prospectors.records[0].text_id = if full { CMD_GREY | id } else { id };
    }

    // = seg000:81ec mouse_handler_move_troop_pick — the destination-pick
    // LMB. DOS gates the click through rect_contains on [map_popup2_ptr] —
    // during the move mode that word is 0 unless the modify-equipment panel
    // record was staged, leaving the test on the pseudo-rect at ds:0; the
    // port reads the evident intent and cancels on a click outside the map
    // window. The caption-panel hit test (loc_05944) belongs to the
    // unported overlay sub-mode.
    pub(crate) fn move_troop_pick_lmb(&mut self) {
        // = seg000:81ec call open_onmap_resource.
        self.open_onmap_spritesheet();
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:81ef..81f6 the gate; a miss exits the menu (loc_08246).
        if !self.map_view_clip_rect().in_rect(x, y) {
            self.menu_callback_choice_exit_menu(0, 0);
            self.open_onmap_spritesheet();
            return;
        }
        // = seg000:81fb..8209 the marker pick, with data_046eb forced to the
        //   overlay value 0x40 across the search and back to 0xc0 after: the
        //   visible-marker entries were built while the spice-density overlay
        //   was drawing, so they carry mode 0x40 and only match the search
        //   under that value. Any appearance (al = 0xff) within distance 9.
        let saved_046eb = std::mem::replace(&mut self.data_046eb, 0x40);
        let (marker_ptr, dist) = self.find_nearest_location_marker(0xff, x, y);
        self.data_046eb = saved_046eb | 0xc0;
        if dist >= 9 || marker_ptr == 0 {
            return;
        }
        let li = location_index_from_ptr(marker_ptr);
        // = seg000:820f call move_troop_validate_pick; jb ret (keep picking).
        if self.move_troop_validate_pick(li) {
            // = seg000:8212 fall through into the done path.
            self.move_troop_finalize_order(Some(li));
        }
    }

    // = seg000:8256 move_troop_validate_pick — validate/queue the picked
    // destination; returns whether the order proceeds (the DOS carry-clear
    // fall-through into the done path).
    fn move_troop_validate_pick(&mut self, li: usize) -> bool {
        // = seg000:8256..825d a non-prospector troop goes straight through
        //   (loc_0829e).
        let Some(ti) = self.contact_verb_troop() else {
            return false;
        };
        if ti != 2 {
            return true;
        }
        // = seg000:825f..826a the prospector needs a sietch (appearance
        //   < 0x20) or an Atreides-held area (status bit 3).
        let loc = &self.locations[li];
        if loc.appearance >= 0x20 && loc.status & 8 == 0 {
            return false;
        }
        // = seg000:826c..8273 a full working queue starts over.
        if self.prospector_pick_count >= 3 {
            self.give_new_destinations_for_prospectors();
        }
        // = seg000:8276..8282 append.
        let slot = self.prospector_pick_count as usize;
        self.prospector_pick_queue[slot] = crate::locations::location_ptr_from_index(li);
        self.prospector_pick_count += 1;
        // = seg000:8286..828c redraw the overlay (loc_0542f, unported) and
        //   the menu (its ADD slot may grey), re-inserted in place.
        self.move_prospectors_configure_menu();
        self.menu_stack_push(
            MenuRef::MenuMoveProspectors,
            Some(GameState::move_troop_cleanup),
        );
        // = seg000:828f..829b three destinations proceed to the done path
        //   after a 0x32-tick beat (a busy-wait; no deterministic state
        //   effect headless).
        self.prospector_pick_count >= 3
    }

    // = seg000:8214 menu_callback_choice_map_move_prospectors_done — the
    // shared done path: also entered by a valid pick's fall-through with the
    // picked destination.
    pub(crate) fn menu_callback_choice_map_move_prospectors_done(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        self.move_troop_finalize_order(None);
    }

    // = seg000:8214..8243 the move-order finalization.
    fn move_troop_finalize_order(&mut self, picked: Option<usize>) {
        // = seg000:8214 call move_troop_teardown (loc_082b7).
        self.move_troop_teardown();
        // = seg000:8217 call contact_verb_troop.
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        // = seg000:821a..8233 the prospector copies the working queue back
        //   (three words) and orders toward its head; an empty head cancels.
        let dest_li = if ti == 2 {
            let queue = self.prospector_pick_queue;
            self.prospector_destinations[..3].copy_from_slice(&queue[..3]);
            let head = self.prospector_destinations[0];
            if head == 0 {
                // = seg000:8233 jz loc_08246.
                self.menu_callback_choice_exit_menu(0, 0);
                self.open_onmap_spritesheet();
                return;
            }
            location_index_from_ptr(head)
        } else {
            let Some(li) = picked else {
                return;
            };
            li
        };
        // = seg000:8235/8238 the acknowledgement line; a spoken-line event
        //   that drops the gate refuses the order (loc_08246).
        if !self.troop_present_move_acknowledgement(ti, dest_li) {
            self.menu_callback_choice_exit_menu(0, 0);
            self.open_onmap_spritesheet();
            return;
        }
        // = seg000:823a call troop_issue_move_order.
        self.troop_issue_move_order(ti, dest_li);
        // = seg000:823d call screen_element_stack_pop_and_redraw — the pick
        //   menu pops WITHOUT its cleanup (the success path restores by
        //   hand).
        if self.menu_stack.len() > 1 {
            self.menu_stack.pop();
            self.redraw_active_command_menu();
        }
        // = seg000:8240 call loc_08250 — the map mouse handlers return.
        self.move_troop_restore_map_handlers();
        // = seg000:8243 jmp map_setup_main_menu.
        self.map_setup_main_menu();
    }

    // = seg000:80c8 give_new_destinations_for_prospectors — reset the
    // working queue (three words; the DOS clear matches the three-word
    // copies).
    fn give_new_destinations_for_prospectors(&mut self) {
        self.prospector_pick_count = 0;
        self.prospector_pick_queue[..3].fill(0);
    }

    // = seg000:80d9 menu_callback_choice_map_move_prospectors_give_new_
    // destinations — the GIVE NEW DESTINATIONS verb: reset the queue and
    // redraw (loc_08286: the overlay redraw is unported, the menu ungreys
    // and re-inserts).
    pub(crate) fn menu_callback_choice_map_move_prospectors_give_new_destinations(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        self.give_new_destinations_for_prospectors();
        self.move_prospectors_configure_menu();
        self.menu_stack_push(
            MenuRef::MenuMoveProspectors,
            Some(GameState::move_troop_cleanup),
        );
    }

    // = seg000:82da troop_present_move_acknowledgement — the troop speaks
    // its answer to a move order toward `dest_li`: action code 0x0b with the
    // destination staged for CONDIT (or 0x10 without restaging when it
    // already stands there). Returns whether the order was accepted (the
    // interrupt gate survived the line's events).
    fn troop_present_move_acknowledgement(&mut self, ti: usize, dest_li: usize) -> bool {
        // = seg000:82dc bp = [si+4].
        let old_ptr = self.troops[ti].offset_of_location;
        let dest_ptr = crate::locations::location_ptr_from_index(dest_li);
        // = seg000:82df..82f8 the action code, with the destination staged
        //   for the line's conditions and name placeholders.
        let action = if ti != 2 && dest_ptr == old_ptr {
            0x10
        } else {
            self.troops[ti].offset_of_location = dest_ptr;
            self.troop_prepare_troop_data_for_condit(ti);
            self.set_command_menu_origin();
            self.troops[ti].offset_of_location = old_ptr;
            0x0b
        };
        // = seg000:82fa call loc_09f82 — the subtitle font.
        self.font_state.color = 0x00f0;
        self.font_select_tall_font();
        // = seg000:82fd call arm_dialogue_interrupt_gate.
        self.dialogue_interrupt_gate = 0xff;
        // = seg000:8300 call troop_present_dialogue_line_with_action
        //   (loc_07bbe): pending_room_action = the code, a fresh resume
        //   cursor, the line through the Fremen voice bank (0x0f), the
        //   voice replay when no line matched.
        self.map_contact_troop_pending = Some(ti);
        self.pending_room_action = action;
        self.dialogue_resume_entry_ptr = 0;
        self.set_screen_as_active_framebuffer();
        if !self.present_room_person_line(0x0f) {
            self.play_dialogue_voc();
        }
        // = seg000:7c56..7c5d with the full map up, drop the bubble pointer.
        if self.data_046eb & 0x80 != 0 {
            self.subtitle_bubble = None;
        }
        self.set_fb1_as_active_framebuffer();
        // = seg000:8305 jmp test_dialogue_interrupt_gate — ZF = accepted.
        self.dialogue_interrupt_gate == 0xff
    }

    // = seg000:824d loc_0824d — the move mode's Cancel cleanup: the
    // teardown, then the map mouse handlers.
    pub(crate) fn move_troop_cleanup(&mut self) {
        self.move_troop_teardown();
        self.move_troop_restore_map_handlers();
    }

    // = seg000:82b7 move_troop_teardown — leave the destination-pick
    // overlay: while the overlay sub-mode (data_046eb bit 6) is up, exit it
    // (map_leave_spice_density_overlay), repaint the contact popup panel and
    // re-present the contact dialogue (loc_07be0).
    fn move_troop_teardown(&mut self) {
        // = seg000:82b7/82bc test data_046eb,40h; jz ret.
        if self.data_046eb & 0x40 == 0 {
            return;
        }
        // = seg000:82be call map_leave_spice_density_overlay.
        self.map_leave_spice_density_overlay();
        // = seg000:82c1..82ca si = troop_contact_text_panel_record; call
        //   loc_0c551 — repaint the contact popup's panel outline on screen.
        self.set_screen_as_active_framebuffer();
        let r = self.map_contact_popup_rect;
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 0xf5);
        // = seg000:82cd/82d0 call contact_verb_troop; call loc_07be0 — for
        //   the live contact troop: a fresh resume cursor and the
        //   ask-for-more re-present (which rebuilds the popup and speaks the
        //   next line).
        if let Some(ti) = self.contact_verb_troop() {
            if self.map_contact_troop == Some(ti) {
                self.dialogue_resume_entry_ptr = 0;
                self.menu_callback_choice_map_troop_dialogue_ask_for_more_information(0, 0);
            }
        }
        // = seg000:82d3 back to fb1.
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:8250 loc_08250 (-> loc_05ad3) — restore the map view's mouse
    // handlers and nav rect, and reopen ONMAP.
    fn move_troop_restore_map_handlers(&mut self) {
        self.active_mouse_handlers = &DUNE_MAP_MOUSE_HANDLERS;
        self.set_mouse_nav_rect(self.map_view_rect);
        self.open_onmap_spritesheet();
    }

    // = seg000:878c map_setup_main_menu — configure the map main verb menu's
    // ids and grey bits from the game state, push it onto the menu stack
    // (bx = nullsub_00f66, a no-op cleanup) and reopen ONMAP.
    fn map_setup_main_menu(&mut self) {
        // = seg000:878c dialogue_resume_entry_ptr = 0.
        self.dialogue_resume_entry_ptr = 0;
        // = seg000:8792..87bd the TAKE AN ORNITHOPTER id: greyed (CMD_GREY)
        //   unless a location scene is up (current_scene != 0xff and not a
        //   deep sietch room) and its available equipment has an orni.
        let mut orni_id = CMD_GREY | cmd::TAKE_AN_ORNITHOPTER;
        if self.data_00008 != 0xff && (self.data_00008 < 0x20 || self.current_room < 3) {
            // = seg000:87aa..87bb di = [current_location_ptr]; call
            //   compute_location_available_equipment; ax = 0xa7, greyed when
            //   orni_count is 0.
            self.compute_location_available_equipment(self.current_location_index as usize);
            orni_id = if self.available_equipment.ornithopters != 0 {
                cmd::TAKE_AN_ORNITHOPTER
            } else {
                CMD_GREY | cmd::TAKE_AN_ORNITHOPTER
            };
        }
        // = seg000:87c0 bp = menu_map_main.
        // = seg000:87c3 [bp+0eh] = ax — the ornithopter entry's id.
        self.menu_map_troops.records[3].text_id = orni_id;
        // = seg000:87c6 [bp+0bh] |= 0x40 — SEE SPICE DENSITY greyed;
        // = seg000:87ca [bp+12h] = 0 — FIND PROSPECTORS hidden.
        self.menu_map_troops.records[2].text_id = CMD_GREY | cmd::SEE_SPICE_DENSITY;
        self.menu_map_troops.records[4].text_id = 0;
        // = seg000:87cf..87da game_phase >= 5 ungreys SEE SPICE DENSITY and
        //   shows FIND PROSPECTORS.
        if self.game_phase >= 5 {
            self.menu_map_troops.records[2].text_id = cmd::SEE_SPICE_DENSITY;
            self.menu_map_troops.records[4].text_id = cmd::FIND_PROSPECTORS;
        }
        // = seg000:87df..8813 the contact slot ([bp+6]).
        if self.location_visibility_distance < 2 {
            // = seg000:87e6 GIVE ORDERS TO TROOP, greyed unless the current
            //   location has a troop (the head of its troop chain resolves).
            let mut id = CMD_GREY | cmd::GIVE_ORDERS_TO_TROOP;
            if let Some(location) = self.locations.get(self.current_location_index as usize) {
                // = seg000:87f3..87fd [di+9] != 0 and get_address_of_troop_by_ID.
                let troop_id = location.troop_id;
                if troop_id != 0 && self.troops.get((troop_id - 1) as usize).is_some() {
                    // = seg000:87ff and word ptr [bp+6],0bfffh.
                    id = cmd::GIVE_ORDERS_TO_TROOP;
                }
            }
            self.menu_map_troops.records[1].text_id = id;
        } else {
            // = seg000:8806..8813 CONTACT FREMEN TROOPS, greyed while no troop
            //   icons are on the map (troop_icon_count 0).
            self.menu_map_troops.records[1].text_id = if self.troop_icons.is_empty() {
                CMD_GREY | cmd::CONTACT_FREMEN_TROOPS
            } else {
                cmd::CONTACT_FREMEN_TROOPS
            };
        }
        // = seg000:8816/8819 bx = nullsub_00f66; call screen_element_stack_push.
        self.menu_stack_push(MenuRef::MenuMapTroops, None);
        // = seg000:881c jmp open_onmap_resource.
        self.open_onmap_spritesheet();
    }

    // = seg000:5bb0 map_show_rallied_troops_popup — draw the first-visit DUNE
    // MAP title popup straight to the visible screen: the panel record
    // data_0194a, then DUNE_MAP_RALLIED_TROOPS ("DUNE MAP / * Map to command
    // rallied troops * / Number of rallied troops = N") with the live count
    // patched over the trailing digits.
    fn map_show_rallied_troops_popup(&mut self) {
        // = seg000:5bb0 call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:5bb3/5bb6 map_popup_ptr = data_0194a.
        self.map_popup_ptr = MAP_POPUP_RALLIED;
        // = seg000:5bba call loc_07b1b — fill the panel rect with its fill
        //   colour ([rec+9] = 0xfb), then outline it in the frame colour
        //   ([rec+8] = 0xf5) inset one pixel (loc_0c551 -> draw_rect_outline).
        let r = RALLIED_POPUP_RECT;
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            0xfb,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 0xf5);
        // = seg000:5bbd call font_select_tall_font.
        self.font_select_tall_font();
        // = seg000:5bc0/5bc3 the title text.
        let mut text = self
            .get_phrase_or_command_string(cmd::DUNE_MAP_HEADER)
            .to_vec();
        // = seg000:5bc6..5bce find_last_numeric_digit_in_str_at_es_si +
        //   string_replace_number_ending_at_es_si — write the decimal
        //   number_of_rallied_troops backwards over the digit tail (the
        //   template's leading spaces stay for short counts).
        if let Some(last) = text.iter().rposition(|c| c.is_ascii_digit()) {
            let mut n = self.number_of_rallied_troops as u16;
            let mut i = last;
            loop {
                text[i] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 || i == 0 {
                    break;
                }
                i -= 1;
            }
        }
        // = seg000:5bd1..5be5 the pen: the panel origin + (10, 8); colour
        //   word 0x00f0 (fg 0xf0 on transparent bg).
        self.font_state.color = 0x00f0;
        self.font_set_draw_position(r.x0 as u16 + 10, r.y0 as u16 + 8);
        self.font_draw_string(&text);
        // = seg000:5be8 jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
        // DOS drew straight to the visible A000 buffer; the port publishes the
        // touched screen.
        self.send_frame_to_display();
    }

    // = seg000:5beb map_dismiss_rallied_troops_popup — when the open popup is
    // the rallied-troops title panel, close it and repaint the map beneath.
    pub(crate) fn map_close_rallied_troops_popup(&mut self) {
        // = seg000:5beb cmp map_popup_ptr,194ah; jnz ret.
        if self.map_popup_ptr != MAP_POPUP_RALLIED {
            return;
        }
        // = seg000:5bf6/5bf8 xor si,si; xchg si,[map_popup_ptr].
        self.map_popup_ptr = 0;
        // = seg000:5bfc call troop_icons_update_dirty_rect — repaint the map
        //   beneath the panel rect from the fb2 snapshot.
        self.troop_icons_update_dirty_rect(RALLIED_POPUP_RECT);
    }

    // = seg000:6314 map_draw_player_position_sprite — draw the "you are here"
    // ICONES sprite 0x4c at the player's projected map position, anchored
    // 13 px left and one sprite height up from the point.
    pub(crate) fn map_draw_player_position_sprite(&mut self) {
        // = seg000:6314/6317 get_map_position; map_position_to_screen_if_
        //   visible; jb ret.
        let (x, lat) = self.get_map_position();
        let Some((sx, sy)) = self.map_position_to_screen_if_visible(x, lat) else {
            return;
        };
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        // = seg000:6324..632c push the active bank and open ICONES.
        let prev = self.open_icones_spritesheet();
        // = seg000:631a ax = 0x4c; 631e dx -= 13; 6330 bl -= the sprite
        //   height; 6334 call draw_sprite_clipped_clobbering_bx_dx.
        self.with_active_bank_sheet(|s, sheet| {
            if let Some(sprite) = sheet.get_sprite(0x4c) {
                let h = sprite.height() as i16;
                s.draw_sprite_from_sheet_clipped(sheet, 0x4c, sx - 13, sy - h + yoff, clip);
            }
        });
        // = seg000:6337/6338 restore the previous bank.
        self.open_sprite_bank(prev as i16);
    }

    // = seg000:633b map_draw_vegetation_marks — draw the vegetation tufts over
    // the full map view, one band at a time from the top of the window until a
    // band projects outside it. Run by map_build_and_draw_location_markers in
    // full-map mode before the location markers; the active bank is ONMAP.
    pub(crate) fn map_draw_vegetation_marks(&mut self) {
        // = seg000:633b dx = longitude; bx = latitude - 0x12 (the top band).
        let lng = self.zoomed_globe_longitude;
        let mut lat = self.zoomed_globe_latitude - 0x12;
        // = seg000:6346 loop while map_position_to_screen_if_visible clears CF.
        while let Some((sx, sy)) = self.map_position_to_screen_if_visible(lng, lat) {
            self.map_draw_vegetation_marks_row(lng, lat, sx, sy);
            lat += 1;
        }
    }

    // = seg000:634d map_draw_vegetation_marks_row — one band's marks: walk the
    // map row east (loc_0636a) then west (loc_0639a) from the projected centre
    // column, 4 screen px per map cell, wrapping around the row ends.
    fn map_draw_vegetation_marks_row(&mut self, lng: u16, lat: i16, sx: i16, sy: i16) {
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let y = (lat + 98) as u16;
        let row_off = tablat.offset(y) as usize;
        let row_len = tablat.len(y) as usize;
        if row_len == 0 {
            return;
        }
        // = seg000:636e/639e map_func — the centre cell for (lng, lat).
        let cell = ((row_len as u32 * lng as u32) >> 16) as usize;
        let (wx0, wx1) = (self.map_view_rect.x0, self.map_view_rect.x1);

        // = seg000:636a map_draw_vegetation_marks_east — from the centre
        //   column to the window's right edge.
        let mut x = sx;
        let mut i = cell;
        loop {
            self.map_draw_vegetation_mark(row_off, row_len, i, x, sy);
            // = seg000:637f add dx,4; cmp dx,[data_046e3_rect.x1]; jnb ret.
            x += 4;
            if x >= wx1 {
                break;
            }
            // = seg000:6388..6390 the cell step, wrapping at the row end.
            i = (i + 1) % row_len;
        }

        // = seg000:639a map_draw_vegetation_marks_west — from the centre
        //   column to the window's left edge.
        let mut x = sx;
        let mut i = cell;
        loop {
            self.map_draw_vegetation_mark(row_off, row_len, i, x, sy);
            // = seg000:63af sub dx,4; cmp dx,[data_046e3_rect.x0]; jb ret.
            x -= 4;
            if x < wx0 {
                break;
            }
            // = seg000:63b8..63be the cell step, wrapping at the row start.
            i = (i + row_len - 1) % row_len;
        }
    }

    // = seg000:6375/63a5 the per-cell test + seg000:63c7 map_draw_vegetation_
    // mark_sprite — on a vegetation cell (map byte & 0x30 == 0x10) stamp a
    // tuft: ONMAP sprite 0x79 when the next cell east is also vegetation,
    // else 0x78, jittered 0..3 px in x and y from the cell's map offset so
    // the tufts do not grid-align.
    fn map_draw_vegetation_mark(
        &mut self,
        row_off: usize,
        row_len: usize,
        i: usize,
        x: i16,
        y: i16,
    ) {
        // = seg000:6375 ax = the cell pair; and ax,3030h; cmp al,10h; jnz.
        let cell = self.map[row_off + i];
        if cell & 0x30 != 0x10 {
            return;
        }
        let next = self.map[row_off + (i + 1) % row_len];
        // = seg000:63cd..63d5 sprite 0x79 when the neighbour matches, else 0x78.
        let sprite = if next & 0x30 == 0x10 { 0x79 } else { 0x78 };
        // = seg000:63d6..63e4 the jitter: di = the map byte offset, bp = the
        //   row length; y += di & 3, x += ((bp + di) >> 2) & 3.
        let di = row_off + i;
        let jy = (di & 3) as i16;
        let jx = (((row_len + di) >> 2) & 3) as i16;
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        // = seg000:63e6 call loc_0c343 — centred + clipped.
        self.with_active_bank_sheet(|s, sheet| {
            s.draw_sprite_centered_clipped(sheet, sprite, x + jx, y + jy + yoff, clip);
        });
    }

    // = seg000:5c03 map_main_mouse_idle — the full-map view's idle/hover
    // handler (mouse_handlers_01a9e).
    pub(crate) fn dune_map_mouse_idle(&mut self) {
        // = seg000:5c03..5c17 auto-dismiss the rallied-troops popup 1000 PIT
        //   ticks (~5 s) after game_clock_tick_base — the game loop re-stamps
        //   that on every mouse-button edge (seg000:d893), so the budget runs
        //   from the click that opened the view.
        if self.map_popup_ptr == MAP_POPUP_RALLIED
            && (self.game_ticks() as u16).wrapping_sub(self.game_clock_tick_base) >= 1000
        {
            // = seg000:5c19 call_restore_cursor; 5c1c the dismissal; 5c1f
            //   jmp draw_mouse.
            self.call_restore_cursor();
            self.map_close_rallied_troops_popup();
            self.draw_mouse();
        }
        // = seg000:5c22..5c75 the marker hover label + the occupation-panel
        //   readout — both gated on the troop occupation panel (data_04710)
        //   being open, which is not ported yet. TODO.
    }

    // = seg000:5c76 map_main_mouse_lmb — the full-map view's LMB handler.
    pub(crate) fn dune_map_mouse_lmb(&mut self) {
        // = seg000:5c76 call map_dismiss_rallied_troops_popup.
        self.map_close_rallied_troops_popup();
        // = seg000:5c79 call open_onmap_resource.
        self.open_onmap_spritesheet();
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:5c7c..5ca2 a click inside the open popup panel routes to the
        //   panel, not a dismiss: the troop occupation panel (data_04710 ->
        //   loc_05923) or the equipment spinners (loc_07e97/loc_07eb8), both
        //   stubbed, so an inside-click is a no-op here.
        if let Some(r) = self.map_open_popup_rect() {
            if r.in_rect(x, y) {
                return;
            }
        }
        // = seg000:5ca5 cmp data_046f5,0 — with the spice-density overlay up
        //   any other click exits the menu. Not ported (no overlay yet).
        // = seg000:5caf call loc_06946 (the icon hit-test); jb troop_0872c.
        if let Some((_, ti)) = self.troop_icon_hit_test(x, y) {
            self.map_click_troop_icon(ti);
            return;
        }
        // = seg000:5cb7 al=31h; call find_nearest_location_marker — a click
        //   within 20 px of a location marker (appearance < 0x31) opens the
        //   location troop popup (loc_05fb0).
        let (marker, dist) = self.find_nearest_location_marker(0x31, x, y);
        if dist < 0x14 && marker != 0 {
            let li = location_index_from_ptr(marker);
            // = seg000:5cc1 cmp di,[data_046f8]; jz — re-clicking the open
            //   location's marker is a no-op (its popup is already up).
            if self.map_location_popup_loc != Some(li) {
                self.map_click_location_marker(li);
            }
            return;
        }
        // = seg000:5cca..5cd0 a click on empty map space closes the open
        //   popups: the location popup menu (loc_05f79), the troop info panel
        //   (loc_079de) and the spice sub-mode (loc_058fa, stubbed).
        self.map_close_location_troop_popup();
        self.map_close_troop_info_popup();
        // = seg000:5cd3..5ce0 a live troop contact (data_01954) tears down
        //   through the no-more-orders path, folded back in.
        if self.map_selected_troop_id != 0 {
            self.screen_overlay_request_transition();
            self.menu_callback_choice_multiple_no_more_orders(0, 0);
            self.play_pending_panel_fold();
        }
    }

    // = the open popup panel's rect (the record rect DOS reads through
    // map_popup_ptr, e.g. seg000:5c7c rect_contains, loc_0c7d4): the
    // rallied-troops title panel, the troop info panel, the location info
    // panel, the troop contact dialogue panel or the spice-density overlay
    // panel. None when no popup is up.
    pub(crate) fn map_open_popup_rect(&self) -> Option<Rect> {
        self.map_popup_record_rect(self.map_popup_ptr)
    }

    // = the popup record's rect field (the +0..+7 words at the pointer DOS
    // keeps in map_popup_ptr / map_popup2_ptr).
    pub(crate) fn map_popup_record_rect(&self, ptr: u16) -> Option<Rect> {
        match ptr {
            MAP_POPUP_RALLIED => Some(RALLIED_POPUP_RECT),
            MAP_POPUP_TROOP_INFO => Some(self.map_info_panel_rect),
            MAP_POPUP_LOCATION => Some(self.map_location_popup_rect),
            MAP_POPUP_TROOP_CONTACT => Some(self.map_contact_popup_rect),
            MAP_POPUP_SPICE_OVERLAY => Some(self.map_overlay_panel_rect),
            _ => None,
        }
    }

    // = seg000:5ce4 map_main_mouse_rmb — the full-map view's RMB handler:
    // toggle the troop info panel.
    pub(crate) fn dune_map_mouse_rmb(&mut self) {
        // = seg000:5ce4 call map_dismiss_rallied_troops_popup.
        self.map_close_rallied_troops_popup();
        // = seg000:5ce7 call open_onmap_resource.
        self.open_onmap_spritesheet();
        // = seg000:5cea..5cef a secondary popup open blocks the toggle.
        if self.map_popup2_ptr != 0 {
            return;
        }
        let x = self.mouse_pos_x as i16;
        let y = self.mouse_pos_y as i16;
        // = seg000:5cf1..5d02 with a popup open: only the info panel is
        //   toggleable — a click inside it closes it; any other open popup
        //   blocks; a click outside falls through to the icon hit-test.
        if self.map_popup_ptr != 0 {
            if self.map_popup_ptr != MAP_POPUP_TROOP_INFO {
                return;
            }
            if self.map_info_panel_rect.in_rect(x, y) {
                // = seg000:5d02 jb loc_05d1a -> loc_079de.
                self.map_close_troop_info_popup();
                return;
            }
        }
        // = seg000:5d04 call loc_06946; jnb ret.
        let Some((icon, ti)) = self.troop_icon_hit_test(x, y) else {
            return;
        };
        // = seg000:5d0c cmp si,[data_046fa]; jz loc_05d1a — the same troop's
        //   panel toggles closed.
        if self.map_info_popup_troop == Some(ti) {
            self.map_close_troop_info_popup();
            return;
        }
        // = seg000:5d12/5d17 close the other troop popups, then open
        //   (troop_078bc).
        self.map_dismiss_troop_popups();
        self.map_open_troop_info_popup(icon, ti);
    }

    // = seg000:872c troop_0872c — an LMB click on a troop icon: select the
    // troop (data_01954) and open the contact UI over it; a click on the
    // already-selected troop goes straight to the troop dialogue.
    fn map_click_troop_icon(&mut self, ti: usize) {
        // = seg000:872c si = [si+0ah]; 872f al = [si] — the troop id.
        let id = self.troops[ti].troop_id;
        // = seg000:8731..873f while location_visibility_distance < 2, a troop
        //   at the current location is handled in the room, not here.
        if self.location_visibility_distance < 2 {
            let li = location_index_from_ptr(self.troops[ti].offset_of_location);
            if li == self.current_location_index as usize {
                return;
            }
        }
        // = seg000:8741..874d the already-selected troop goes straight to the
        //   next dialogue line, without rebuilding the contact UI.
        if id == self.map_selected_troop_id {
            self.map_open_troop_contact_dialogue(ti);
            return;
        }
        // = seg000:8747 data_01954 = al; 874a jmp loc_08685.
        self.map_selected_troop_id = id;
        self.map_select_troop();
    }

    // = seg000:8685 loc_08685 — (re)build the selected-troop UI: tear the old
    // contact UI down, spawn the highlight ring over the selection, then open
    // the contact verb menu and the troop dialogue.
    pub(crate) fn map_select_troop(&mut self) {
        // = seg000:8685 data_046d8 = 1 — suppress the info panel's outline
        //   scale-out below (the selection replaces it immediately).
        self.map_popup_anim_suppress = true;
        // = seg000:868a call loc_069a3 — remove the old highlight ring.
        self.map_remove_focused_troop_icon();
        // = seg000:868d call map_close_troop_contact_popup — tear down the
        //   previous troop's contact dialogue popup.
        self.map_close_troop_contact_popup();
        // = seg000:8690/8693/8696 close the location popup menu (loc_05f79,
        //   not ported), the info panel (loc_079de) and the spice sub-mode
        //   (loc_058fa, not ported).
        self.map_close_troop_info_popup();
        // = seg000:8699..86a3 a valid selected id resolves its troop; the
        //   carry get_address_of_troop_by_ID returns is "the troop is rallied"
        //   (occupation < 0x80), and jnb bails without one.
        let id = self.map_selected_troop_id;
        if id == 0 || id > 0x43 {
            return;
        }
        let ti = (id - 1) as usize;
        if self.troops.get(ti).is_none_or(|t| t.occupation >= 0x80) {
            return;
        }
        // = seg000:86a5 data_01955 = al — the id the contact verb resumes from
        //   once the selection is dropped.
        self.map_last_selected_troop_id = id;
        // = seg000:86a9 call troop_0697c — the highlight ring.
        self.map_focus_troop_icon(ti);
        // = seg000:86ae call map_open_troop_contact_menu — the contact verb menu.
        self.map_open_troop_contact_menu(ti);
        // = seg000:86b2/86b5 di = the troop's location; call
        //   map_open_troop_contact_dialogue — the popup and its first line.
        self.map_open_troop_contact_dialogue(ti);
    }

    // = seg000:7c02 map_open_troop_contact_dialogue — the contacted troop's
    // dialogue: (re)build the popup over the map, stage its CONDIT block, then present one
    // line into it (subtitle + voice). Re-entered for every further line the
    // contact menu's verbs ask for.
    pub(crate) fn map_open_troop_contact_dialogue(&mut self, ti: usize) {
        // = seg000:7c02 call map_setup_troop_contact_popup — the popup,
        //   unless it is already up for this troop.
        self.map_setup_troop_contact_popup(ti);
        // = seg000:7c05 call troop_prepare_troop_data_for_condit — stage the
        //   troop's block so the record's conditions can read it.
        self.troop_prepare_troop_data_for_condit(ti);
        // = seg000:7c08/7c0b di = [si+4]; call set_command_menu_origin — the
        //   menu origin from the troop's location (a stub in the port).
        self.set_command_menu_origin();
        // = seg000:7c0e..7c2a out of visibility range the troop answers from
        //   afar: ds:4c = 0xff picks the record's out-of-contact lines, and the
        //   highlight ring's icon script swaps to seg001:1916 (the "no
        //   contact" ring).
        if self.troop_distance_from_player(ti) > self.location_visibility_distance {
            self.contacting_troops_ds_4c = 0xff;
            // = seg000:7c1c..7c2a di = troop_icon_focused_ptr; [di+0dh] and
            //   [di+0fh] = 1916h — the cursor and the base, so the anim task
            //   restarts on the new script.
            if let Some(i) = self.troop_icon_focused[0] {
                self.troop_icons[i].script_cursor = 0x1916;
                self.troop_icons[i].script_base = 0x1916;
            }
        }
        // = seg000:7c2d..7c34 call map_present_troop_contact_line; jb
        //   loc_07c2d — present a line.
        //   A walk that matched nothing reset the resume cursor, so the retry
        //   restarts at the record head. DOS spins here until a line matches;
        //   the port stops after the restart, since a second failure means no
        //   line can ever match and the loop would take the game with it.
        if !self.map_present_troop_contact_line() {
            self.map_present_troop_contact_line();
        }
        // = seg000:7c36 call loc_09efd — load and play the line's voice.
        self.play_dialogue_voc();
        // = seg000:7c3b data_046f4 = 0; 7c40..7c53 with the interrupt gate at
        //   0x80 (a line whose event armed the equipment hand-over) the popup
        //   also shows the equipment spinners: data_046f4 = 1,
        //   troop_unpack_equipment_flags, loc_07e1e. Not ported — the spinner
        //   panel and its two mouse handlers (loc_07e97/loc_07eb8) are the
        //   MODIFY EQUIPMENT verb's UI. TODO.
        // = seg000:7c56..7c5d on the map view (data_046eb bit 7) drop the
        //   bubble layout pointer without restoring under it (loc_09901): the
        //   popup owns those pixels and takes them down itself.
        if self.data_046eb & 0x80 != 0 {
            self.subtitle_bubble = None;
        }
        // = seg000:7c60 jmp set_fb1_as_active_framebuffer.
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:7bed menu_callback_choice_map_troop_dialogue_ask_for_more_
    // information — the order menu's first verb: ask the contacted troop for
    // its next line.
    pub(crate) fn menu_callback_choice_map_troop_dialogue_ask_for_more_information(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:7bed/7bf4 with the equipment spinners up (data_046f4) AND
        //   the spinner sub-mode armed (data_046f5) the verb is a spinner
        //   click instead (loc_07e97). Neither is ported, so the verb always
        //   takes the dialogue path.
        // = seg000:7bfe si = [data_046ef]; falls into troop_07c02.
        let Some(ti) = self.map_contact_troop else {
            return;
        };
        self.map_open_troop_contact_dialogue(ti);
    }

    // = seg000:69b3 menu_callback_choice_map_troop_dialogue_change_troop_
    // occupation — the CHANGE / SELECT TROOP OCCUPATION verb: pick the
    // occupation submenu the troop's current occupation calls for, apply its
    // grey rules, and stage it over the order menu.
    pub(crate) fn menu_callback_choice_map_troop_dialogue_change_troop_occupation(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:69b3 call contact_verb_troop — the troop the contact verbs act on.
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        let occupation = self.troops[ti].occupation;
        // = seg000:69b9..69c0 al = occupation & 0xf; nibble 2 (awaiting
        //   orders) has no occupation to change: the plain SELECT TROOP
        //   OCCUPATION menu, and none of the grey rules below apply to it.
        let menu_ref = if occupation & 0x0f == 2 {
            // = seg001:215a menu_map_select_troop_occupation.
            MenuSelectTroopOccupation
        } else {
            // = seg000:69c2 call troop_get_occupation_bits_2_and_3_0693b — the
            //   occupation class (nibble >> 2): 0 spice, 1 army, >= 2 ecology.
            //   Each class's menu offers the OTHER two specialisations plus
            //   its own extra verb.
            match (occupation & 0x0f) >> 2 {
                // = seg000:69c5 seg001:216e for a spice troop.
                0 => MenuOccupationForSpiceTroop,
                // = seg000:69d2 seg001:2182 for an army troop.
                1 => {
                    // = seg000:69d5..69e2 ESPIONAGE is offered only with a
                    //   Harkonnen holding within 0x1e of the troop's location.
                    let espionage_greyed = self.distance_to_closest_harkonnen_area >= 0x1e;
                    self.menu_occupation_for_army_troop.records[1].set_grayed_if(espionage_greyed);

                    // = seg000:69e8..69f1 an army troop already on espionage
                    //   duty (nibble 5) gets the fortress menu instead
                    //   (seg001:219a) and skips every grey rule below.
                    if occupation & 0x0f == 5 {
                        MenuOccupationForEspionageTroop
                    } else {
                        MenuOccupationForArmyTroop
                    }
                }
                // = seg000:69cd seg001:21a6 for an ecology troop.
                _ => MenuOccupationForEcologyTroop,
            }
        };

        let equipment_greyed = self.game_phase < 0x10;
        let ecology_greyed = self.bitfield_paul_events & 0x20 == 0;
        let menu = self.menu_buffer_mut(menu_ref);

        // = seg000:69f6..6a02 the three class menus lead with GO & SEARCH FOR
        //   EQUIPMENT, greyed until game_phase 0x10. (The SELECT menu skips
        //   this: it jumps straight to the ecology scan below.)
        if occupation & 0x0f != 2 {
            menu.records[0].set_grayed_if(equipment_greyed);
        }

        // = seg000:6a07..6a23 walk the entries to the first SPECIALIZE IN
        //   ECOLOGY and grey it unless Paul has learnt the ecology (the
        //   bitfield_Paul_events 0x20 bit). The walk rewrites the id from its
        //   masked value, so any other high bit on that entry is dropped.
        if let Some(ecology_menu_item) = menu
            .records
            .iter_mut()
            .find(|r| r.text_id & 0x0fff == cmd::SPECIALIZE_IN_ECOLOGY)
        {
            ecology_menu_item.set_grayed_if(ecology_greyed);
        }

        // = seg000:6a25 bx = nullsub_00f66; jmp loc_0d323 — stage the picked
        //   occupation submenu with a no-op cleanup.
        self.stage_command_submenu(menu_ref, |_| {});
    }

    // = seg000:6a71 menu_callback_choice_troop_occupation_specialize_in_spice
    // — SPECIALIZE IN SPICE: occupation 0 (spice mining), except the
    // Prospector troop, which prospects (1) instead.
    pub(crate) fn menu_callback_choice_troop_occupation_specialize_in_spice(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:6a71/6a74 call contact_verb_troop; cl = 0.
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        // = seg000:6a76..6a80 cmp si,troops[2]; jnz troop_apply_occupation_
        //   choice — the Prospector troop (index 2) takes spice prospecting
        //   and then re-installs
        //   the dialogue panel element (loc_02ebf).
        if ti == 2 {
            self.troop_occupation_verb_apply(ti, 1);
            // = seg000:8080 jmp loc_02ebf — put the scene's " Continue…"
            //   panel up (the line just presented installed the script).
            self.sequence_push_continue_menu();
            return;
        }
        self.troop_occupation_verb_apply(ti, 0);
    }

    // = seg000:6a83 menu_callback_choice_troop_occupation_specialize_in_army —
    // SPECIALIZE IN ARMY: occupation 4 (military training).
    pub(crate) fn menu_callback_choice_troop_occupation_specialize_in_army(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        self.troop_occupation_verb_apply(ti, 4);
    }

    // = seg000:6a87 menu_callback_choice_troop_occupation_specialize_in_ecology
    // — SPECIALIZE IN ECOLOGY: occupation 8 (irrigation and tree care).
    pub(crate) fn menu_callback_choice_troop_occupation_specialize_in_ecology(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        self.troop_occupation_verb_apply(ti, 8);
    }

    // = seg000:6a2b menu_callback_choice_troop_occupation_ecology_troop_
    // assembly_wind_trap — ASSEMBLY WIND-TRAP, and seg000:6a35
    // choice_troop_occupation_common_code: the verb keeps the troop's
    // occupation CLASS (bits 2-3) and replaces the job within it (al = 1 here,
    // so an ecology troop goes from irrigation 8 to wind-trap assembly 9).
    pub(crate) fn menu_callback_choice_troop_occupation_assembly_wind_trap(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        self.troop_occupation_within_class(1);
    }

    // = seg000:6a35 choice_troop_occupation_common_code — cl = (occupation &
    // 0x0c) | al, i.e. `job` inside the troop's current occupation class.
    fn troop_occupation_within_class(&mut self, job: u8) {
        // = seg000:6a36 call contact_verb_troop.
        let Some(ti) = self.contact_verb_troop() else {
            return;
        };
        let new = (self.troops[ti].occupation & 0x0c) | job;
        self.troop_occupation_verb_apply(ti, new);
    }

    // = seg000:6a89 troop_apply_occupation_choice — the shared occupation-verb
    // tail: apply the new occupation, let the troop react, and take the change back if it
    // refuses. The reaction is a dialogue line presented with
    // pending_room_action 0x0a (troop_present_reaction_line); a line whose event clears the
    // interrupt gate is the refusal.
    fn troop_occupation_verb_apply(&mut self, ti: usize, new: u8) {
        // = seg000:6a8c..6a93 an unchanged occupation only closes the menu.
        if self.troops[ti].occupation & 0x0f == new {
            self.menu_callback_choice_exit_menu(0, 0);
            return;
        }
        // = seg000:6a95..6a99 push the old occupation byte and the speech word
        //   — what the refusal below puts back.
        let old_occupation = self.troops[ti].occupation;
        let old_speech = self.troops[ti].dissatisfaction_and_speech;
        // = seg000:6a9c call troop_set_occupation — apply it.
        self.troop_set_occupation(ti, new);
        // = seg000:6a9f call arm_dialogue_interrupt_gate.
        self.dialogue_interrupt_gate = 0xff;
        // = seg000:6aa2/6aa4 al = 0x0a; call troop_present_reaction_line —
        //   the troop's reaction.
        self.map_present_troop_reaction_line(ti, 0x0a);
        // = seg000:6aa7..6aad call test_dialogue_interrupt_gate; jz loc_06ab8 —
        //   the gate still armed means no line objected.
        if self.dialogue_interrupt_gate == 0xff {
            // = seg000:6ab8..6ac3 accepted: a troop that left the spice class
            //   (class != 0) gives up its harvester (equipment bit 7).
            if self.troop_get_occupation_bits_2_and_3(ti) != 0 {
                self.troops[ti].equipment &= 0x7f;
            }
        } else {
            // = seg000:6aaf..6ab2 refused: put the speech word back and
            //   re-apply the old occupation. DOS pushed the WORD at si+3, so
            //   `pop cx` hands troop_set_occupation the whole old occupation byte —
            //   bits 0x10..0x80 come back with the nibble.
            self.troops[ti].dissatisfaction_and_speech = old_speech;
            self.troop_set_occupation(ti, old_occupation);
        }
        // = seg000:6ab5 jmp menu_callback_choice_exit_menu — close the
        //   occupation submenu, revealing the order menu under it.
        self.menu_callback_choice_exit_menu(0, 0);
    }

    // = seg000:7bb9 troop_present_reaction_line (+ loc_07bbe) — present one
    // dialogue line for the troop with `action` in pending_room_action, the code the line's
    // conditions read to pick the reaction. The line is spoken by the Fremen-2
    // room person (0x0f), so on the map view it lands in the contact popup.
    fn map_present_troop_reaction_line(&mut self, ti: usize, action: u8) {
        // = seg000:7bb9 call troop_prepare_troop_data_for_condit.
        self.troop_prepare_troop_data_for_condit(ti);
        // = seg000:7bbe/7bc2 data_046f1 = si; pending_room_action = al —
        //   data_046f1 is what subtitle_setup_layout rebuilds the popup from.
        self.map_contact_troop_pending = Some(ti);
        self.pending_room_action = action;
        // = seg000:7bc5 dialogue_resume_entry_ptr = 0 — the reaction starts at
        //   the head of the person's record, not where the contact left off.
        self.dialogue_resume_entry_ptr = 0;
        // = seg000:7bcb call set_screen_as_active_framebuffer.
        self.set_screen_as_active_framebuffer();
        // = seg000:7bce/7bd1 ax = 0x0f; call present_room_person_dialogue.
        if self.present_room_person_line(0x0f) {
            // = seg000:7bd6 call loc_09efd — the line's voice.
            self.play_dialogue_voc();
        }
        // = seg000:7bd9/7bdd si = data_046f1; jmp loc_07c56 — the shared tail:
        //   drop the bubble pointer on the map view, then fb1 active again.
        if self.data_046eb & 0x80 != 0 {
            self.subtitle_bubble = None;
        }
        self.set_fb1_as_active_framebuffer();
    }

    // = seg000:68eb contact_verb_troop — the troop the contact verbs act on: the map's
    // selected troop (data_01954) while the full-map view owns the screen,
    // else the room's Fremen-2 slot (fremen2_troop_ptrs[selected_fremen2_
    // index]). DOS also returns carry = "the troop is rallied" (occupation <
    // 0x80); this caller does not read it.
    pub(crate) fn contact_verb_troop(&self) -> Option<usize> {
        // = seg000:68ee cmp data_046eb,80h; jnb get_address_of_troop_by_ID.
        if self.data_046eb < 0x80 {
            // = seg000:68f5..6904 in the room the troop is whichever Fremen-2
            //   slot the conversation picked (the round-robin the room-entry
            //   classification fills, troops.rs).
            return self.fremen2_troops[(self.selected_fremen2 & 7) as usize];
        }
        // = seg000:6906 get_address_of_troop_by_ID: troops + (id - 1) * 0x1b.
        let id = self.map_selected_troop_id;
        if id == 0 {
            return None;
        }
        self.troops
            .get((id - 1) as usize)
            .map(|_| (id - 1) as usize)
    }

    // = seg000:7ba3 map_setup_troop_contact_popup — put the contact popup up
    // for `ti`, unless it is already this troop's: a further line only
    // repaints the text.
    fn map_setup_troop_contact_popup(&mut self, ti: usize) {
        // = seg000:7ba3 call set_screen_as_active_framebuffer — the popup
        //   draws straight to the visible screen, over the map.
        self.set_screen_as_active_framebuffer();
        // = seg000:7ba6 cmp si,[data_046ef]; jz ret.
        if self.map_contact_troop == Some(ti) {
            return;
        }
        // = seg000:7bad data_046f1 = si — the troop the popup is being built
        //   for, which subtitle_setup_layout rebuilds from.
        self.map_contact_troop_pending = Some(ti);
        // = seg000:7bb1 call map_draw_troop_contact_popup.
        self.map_draw_troop_contact_popup(ti);
        // = seg000:7bb4 call loc_09f40 — the per-presentation setup. On the
        //   map view its only effect is the subtitle font (loc_09f82 below);
        //   the fb1 redirect and the in-room pads are gated on data_046eb == 0
        //   and map_draw_troop_contact_popup set the popup's own pads.
        self.font_state.color = 0x00f0;
        self.font_select_tall_font();
    }

    // = seg000:79ee map_draw_troop_contact_popup — draw the contact dialogue
    // popup: a panel in the half of the screen the troop's icon is not in, with the head box on
    // the left and the subtitle box on the right. Also reached from
    // subtitle_setup_layout (seg000:8cee) when a line is presented with no
    // popup up.
    pub(crate) fn map_draw_troop_contact_popup(&mut self, ti: usize) {
        // = seg000:79ee data_046ef = si — the contact is live from here.
        self.map_contact_troop = Some(ti);
        // = seg000:79f2/79f8 call troop_find_icon; jnz loc_07a1e — without an
        //   icon on the map the record keeps whatever rect it last had.
        let mut r = self.map_contact_popup_rect;
        let icon_pos = self
            .troop_find_icon(ti)
            .map(|i| (self.troop_icons[i].rect.x0, self.troop_icons[i].rect.y0));
        if let Some((_, icon_y)) = icon_pos {
            // = seg000:79fa..7a09 the popup goes opposite the icon: with the
            //   icon in the lower half (y0 >= 76) at the top (y0 = 5), else at
            //   the bottom (y0 = 80). ax rides along as the occupation panel's
            //   y (data_04712).
            let y0 = if icon_y >= 0x4c { 5 } else { 0x50 };
            // = seg000:7a0c/7a12 [si+2] = bx; [si+6] = bx + 0x43.
            r.y0 = y0;
            r.y1 = y0 + 0x43;
            // = seg000:7a15/7a1b data_04710 = 0x5c, data_04712 = 0x1e / 0x0e —
            //   park the shared popup-panel origin in the half the popup is
            //   not in; the spice-density overlay's in-place entry draws its
            //   panel there. The map hover readout also reads it
            //   (seg000:5c22, unported).
            self.map_overlay_panel_pos = (0x5c, if icon_y >= 0x4c { 0x1e } else { 0x0e });
        }
        self.map_contact_popup_rect = r;
        // = seg000:7a1e map_popup_ptr = si — this popup becomes the open one,
        //   so a click inside it routes here, not to the map.
        self.map_popup_ptr = MAP_POPUP_TROOP_CONTACT;
        // = seg000:7a22/7a24 al = 2; call loc_07b0f — data_046d8 = 0, then the
        //   popup's open effect (run_vga_effect al=2 = xor_bracket_zoom_to_panel,
        //   ds:si = the icon rect after the xchg): the XOR box trail from the
        //   icon to the panel centre and the expanding corner brackets, before
        //   the panel is drawn (loc_07b1b). DOS runs the effect even with no
        //   icon on the map (si is then stale); the port skips it.
        self.map_popup_anim_suppress = false;
        if let Some(src) = icon_pos {
            self.xor_bracket_zoom_to_panel(r, src);
        }
        // = the panel fill (0xfb) + frame (0xf5) from the record.
        self.map_draw_panel_record(r, 0xfb, 0xf5);
        // = seg000:7a32..7a50 the subtitle descriptor's origin (the panel
        //   origin + (0x49, 3)) and the popup's own text insets.
        self.map_contact_subtitle_pos = (r.x0 + 0x49, r.y0 + 3);
        self.subtitle_pad_left = 0;
        self.subtitle_pad_right = 5;
        self.subtitle_pad_top = 0;
        self.subtitle_pad_bottom = 1;
        // = seg000:7a53..7a67 data_018f3 = the head box, the panel origin +
        //   (4, 3) and 0x3d square, filled 0xe4 and framed 0xf5.
        let head = rect(r.x0 + 4, r.y0 + 3, r.x0 + 4 + 0x3d, r.y0 + 3 + 0x3d);
        self.map_contact_head_rect = head;
        self.map_draw_panel_record(head, 0xe4, 0xf5);
        // = seg000:7a6a..7b0c the head itself.
        self.map_draw_troop_contact_head(ti, head);
        // = seg000:7b0c jmp open_onmap_resource.
        self.open_onmap_spritesheet();
    }

    // = seg000:7a6a..7b0c — the popup's talking head: pick the head the troop
    // speaks through, load its portrait sheet, and draw one frame into the
    // popup's head box (inset one pixel, 0x3b square).
    fn map_draw_troop_contact_head(&mut self, ti: usize, head_box: Rect) {
        let troop = self.troops[ti];
        // = seg000:7a6e..7a80 a captured troop (occupation bit 5) at a
        //   battle-flagged (location status bit 1) or Atreides location does
        //   not speak for itself — the Harkonnen head 0x0c does, on its
        //   animation 0 and the seg001:22b9 entry the 0x0c byte offset picks.
        let li = location_index_from_ptr(troop.offset_of_location);
        let captor = troop.occupation & 0x20 != 0
            && (self.locations[li].status & 2 != 0 || self.location_is_atreides(li));
        let (anim, anchor) = if captor {
            // = seg000:7a82..7a94 ax = 0x0c both as the resource id and as the
            //   anchor table's byte offset (entry 3); bp = 0 (animation 0).
            self.current_lip_sync_resource_id = 0x0c;
            self.open_talking_head_resource(0x0c, 0);
            self.update_screen_palette();
            (0usize, 3usize)
        } else {
            // = seg000:7a96..7abf the generic Fremen head. The troop is staged
            //   as the room's Fremen-2 so character_id_to_sprite derives the
            //   head sprite and idle expression from it (walk_facing_sprite).
            self.current_lip_sync_resource_id = 0x0f;
            self.fremen2_troops[0] = Some(ti);
            self.selected_fremen2 = 0;
            self.open_talking_head_resource(0x0f, 0);
            self.update_screen_palette();
            // = seg000:7aab..7abf ax = (talking_head_id - 0x0e) * 4 (the anchor
            //   entry); bp = (talking_head_idle_expr - 1) * 2 (the animation).
            let Some(head) = self.talking_head.as_ref() else {
                return;
            };
            let anchor = (head.talking_head_id as usize).saturating_sub(0x0e);
            let anim = head.facing.saturating_sub(1) as usize;
            (anim, anchor)
        };
        // = seg000:7ac1..7acd si = data_022b9 + ax; the two words into
        //   data_046d2/046d4 — the anchor draw_head_image_group_in_box subtracts.
        self.head_popup_anchor = crate::talking_head::popup_anchor(anchor);
        // = seg000:7adc..7afd data_047d4 = the head box inset one pixel, 0x3b
        //   square — the draw origin and the clip.
        self.head_popup_box = rect(
            head_box.x0 + 1,
            head_box.y0 + 1,
            head_box.x0 + 1 + 0x3b,
            head_box.y0 + 1 + 0x3b,
        );
        // = seg000:7ad9/7b02 si = the animation's first frame; call draw_talking_head_in_box.
        self.draw_talking_head_in_box(anim, 0);
        // = seg000:7b06/7b09 si = data_047d4; call gfx_copy_rect_to_screen —
        //   the popup drew into the screen buffer, so publish it.
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:7b1b loc_07b1b — paint a panel record: fill its rect with the
    // record's fill colour ([rec+9]) and outline it one pixel in from the edge
    // in its frame colour ([rec+8], loc_0c551).
    pub(crate) fn map_draw_panel_record(&mut self, r: Rect, fill: u8, frame: u8) {
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            fill,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, frame);
    }

    // = seg000:9719 map_present_troop_contact_line — pick and present one
    // line of the troop-contact dialogue: resume where the last line left off, or
    // start at DIALOGUE[244], the block every contacted troop speaks from.
    // Returns whether a line was presented (DOS's carry-clear exit).
    fn map_present_troop_contact_line(&mut self) -> bool {
        // = seg000:9719 cmp related_to_contacting_troops_ds_4c,0; js — a troop
        //   answering from afar queues no message; one in range queues the
        //   "<troop> is here" message for its location (al = 0x0f, di =
        //   [si+4], messages_02a51). The message queue is not ported.
        // = seg000:972c call loc_09f82 — the subtitle font setup.
        self.font_state.color = 0x00f0;
        self.font_select_tall_font();
        // = seg000:972f current_lip_sync_resource_id = 0x0f — every troop
        //   speaks through the generic Fremen head, so the voice bank and the
        //   lip-sync resource are the Fremen ones.
        self.current_lip_sync_resource_id = 0x0f;
        // = seg000:9735 data_047a2 = &room_persons[15] — the Fremen-2 slot as
        //   the active speaker; only the unported loc_094f3 reads it.
        // = seg000:973b call arm_dialogue_interrupt_gate.
        self.dialogue_interrupt_gate = 0xff;
        // = seg000:973e..9748 si = dialogue_resume_entry_ptr; an exhausted (0)
        //   or reset (0xffff) cursor starts over at si = [DIALOGUE + 122*2],
        //   the record every contacted troop speaks from.
        let start = match self.dialogue_resume_entry_ptr {
            0 | 0xffff => crate::container::entry_offset(&self.dialogue, 122),
            resume => resume,
        };
        // = seg000:974c data_047c2 = 0x20 — the auto/no-verb sentence mask.
        self.data_047c2 = 0x20;
        // = seg000:9751 call present_first_matching_dialogue_line.
        let (next, presented) = self.present_first_matching_dialogue_line(start as usize);
        // = seg000:9754/975a store the resume cursor; a walk that matched
        //   nothing resets it so the next call restarts at the record head.
        self.dialogue_resume_entry_ptr = if presented { next } else { 0 };
        presented
    }

    // = seg000:5a03 loc_05a03 — GIVE ORDERS TO TROOP from the dialogue panel:
    // the Fremen leader you are talking to is contacted on the full-planet map
    // instead, with the map opened on him and his order menu already up.
    pub(crate) fn menu_callback_choice_give_orders_to_troop(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:5a03 call subtitle_restore_prior — take the speech balloon
        //   down before the room goes away.
        self.subtitle_restore_prior();
        // = seg000:5a06 inc map_view_reentry_count — marks this map visit as
        //   the room's troop detour: the order menu's last slot then reads NO
        //   MORE ORDERS rather than CUT CONTACT, and choosing it returns here
        //   (menu_callback_choice_multiple_no_more_orders).
        self.map_view_reentry_count = self.map_view_reentry_count.wrapping_add(1);
        // = seg000:5a0a/5a0d call contact_verb_troop; data_01954 = al — the troop
        //   behind the room's Fremen-2 person becomes the map's selection.
        //   (DOS stores al either way; an empty slot leaves id 0, which
        //   map_select_troop then rejects.)
        let id = self
            .contact_verb_troop()
            .map_or(0, |ti| self.troops[ti].troop_id);
        self.map_selected_troop_id = id;
        // = seg000:5a10 not room_view_toggle — the room/map toggle flips
        //   without going through ui_toggle_room_view.
        self.room_view_toggle = !self.room_view_toggle;
        // = seg000:5a14 call ui_show_globe_map_view.
        self.ui_show_globe_map_view();
        // = seg000:5a17 jmp map_select_troop — the ring, the contact menu and
        //   the troop's dialogue popup, exactly as a click on its icon.
        self.map_select_troop();
    }

    // = seg000:86cc menu_callback_choice_map_main_contact_fremen_troops — the
    // map main menu's contact slot: "CONTACT FREMEN TROOPS" over the whole
    // planet, or "GIVE ORDERS TO TROOP" while location_visibility_distance is
    // short enough that only the player's own location is reachable
    // (map_setup_main_menu picks the wording). Resumes the last contacted
    // troop when nothing is selected, else cycles to the next one.
    pub(crate) fn menu_callback_choice_map_main_contact_fremen_troops(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:86cc call map_dismiss_rallied_troops_popup.
        self.map_close_rallied_troops_popup();
        // = seg000:86cf cmp number_of_rallied_troops,0; jz ret — no Fremen
        //   have joined yet, so there is nobody to contact.
        if self.number_of_rallied_troops == 0 {
            return;
        }
        // = seg000:86d6 cmp location_visibility_distance,2; jb
        //   map_contact_troop_at_current_location.
        if self.location_visibility_distance < 2 {
            self.map_contact_troop_at_current_location();
            return;
        }
        // = seg000:86dd ax = the word at data_01954: al the selected id, ah
        //   the last contacted one (data_01955).
        // = seg000:86e0 or al,al; jnz — a live selection cycles on.
        if self.map_selected_troop_id != 0 {
            self.menu_callback_choice_map_troop_contact_next_troop(0, 0);
            return;
        }
        // = seg000:86e4/86e8 al = ah; or al,al; jz — nothing contacted yet
        //   either, so start the cycle from troop id 0.
        let last = self.map_last_selected_troop_id;
        if last == 0 {
            self.menu_callback_choice_map_troop_contact_next_troop(0, 0);
            return;
        }
        // = seg000:86ea data_01954 = ah; 86ed call get_address_of_troop_by_ID;
        //   86f0 call troop_find_icon; 86f3 jz loc_08685 — resume the last
        //   contacted troop, but only while it still has an icon on the map.
        self.map_selected_troop_id = last;
        let ti = (last - 1) as usize;
        if self.troops.get(ti).is_some() && self.troop_find_icon(ti).is_some() {
            self.map_select_troop();
            return;
        }
        // = seg000:86f5 data_01954 = 0; falls into the next-troop scan.
        self.map_selected_troop_id = 0;
        self.menu_callback_choice_map_troop_contact_next_troop(0, 0);
    }

    // = seg000:86b9 map_contact_troop_at_current_location — the GIVE ORDERS TO
    // TROOP path: contact the troop stationed at the player's own location,
    // recentring the map on it first (at this visibility distance it is the
    // only troop reachable).
    fn map_contact_troop_at_current_location(&mut self) {
        // = seg000:86b9/86bd di = [current_location_ptr]; al = [di+9] — the
        //   head of the location's troop chain.
        let Some(location) = self.locations.get(self.current_location_index as usize) else {
            return;
        };
        // = seg000:86c0 data_01954 = al.
        self.map_selected_troop_id = location.troop_id;
        // = seg000:86c3 call set_zoomed_globe_pos_from_map_position — recentre
        //   the map on the player.
        self.set_zoomed_globe_pos_from_map_position();
        // = seg000:86c6 call [_word_23B9D_current_main_view_drawing_function]
        //   — the installed main-view redraw (ui_main_view_map_interface here;
        //   the verb is only reachable with the map view up).
        let redraw = self
            .current_main_view_drawing_function
            .expect("the contact verb with no main-view drawing function installed");
        redraw(self);
        // = seg000:86ca jmp loc_08685.
        self.map_select_troop();
    }

    // = seg000:86fa menu_callback_choice_map_troop_contact_next_troop — the
    // NEXT TROOP verb: of the troops with an icon on the map, contact the one
    // whose id follows the selected one cyclically.
    pub(crate) fn menu_callback_choice_map_troop_contact_next_troop(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:86fa si = troop_icon_count; lodsw; cx = ax; jcxz ret.
        if self.troop_icons.is_empty() {
            return;
        }
        // = seg000:8702..8707 al = the selected id; bh = 0xff (the best id
        //   delta so far); di = 0 (the best icon).
        let selected = self.map_selected_troop_id;
        let mut best: Option<usize> = None;
        let mut best_delta = 0xffu8;
        for i in 0..self.troop_icons.len() {
            // = seg000:8709 bp = [si+0ah] — the icon's troop.
            let t = &self.troops[self.troop_icons[i].troop_index];
            // = seg000:870c cmp byte ptr [bp+3],80h; jnb — an unrallied troop
            //   (occupation bit 7) is not contactable.
            if t.occupation >= 0x80 {
                continue;
            }
            // = seg000:8712..871b ah = troop->troop_id - al; the smallest
            //   non-zero unsigned delta wins, i.e. the next id above the
            //   selection, wrapping past the end of the troop table.
            let delta = t.troop_id.wrapping_sub(selected);
            if delta == 0 || delta > best_delta {
                continue;
            }
            best_delta = delta;
            best = Some(i);
        }
        // = seg000:8726 or di,di; jz ret; 872a si = di, falling into
        //   map_click_troop_icon with the winning icon.
        let Some(i) = best else {
            return;
        };
        let ti = self.troop_icons[i].troop_index;
        self.map_click_troop_icon(ti);
    }

    // = seg000:780a map_open_troop_contact_menu — open the contact verb menu
    // over the selected troop. Which of the three menus applies is decided
    // here: the full order menu for a troop in range and standing still, the
    // change-destination menu for one on the move, and the cycle menu for one
    // out of range or held prisoner. Also entered from the room view
    // (seg000:1768).
    pub(crate) fn map_open_troop_contact_menu(&mut self, ti: usize) {
        // = seg000:780a call troop_07c63; 780d bp = menu_map_troop_contact_
        //   cycle_troops; 7810 cmp ax,[location_visibility_distance]; ja — out
        //   of visibility range the troop can only be cycled past.
        let mut element = MenuRef::MenuNextTroop;
        let in_range = self.troop_distance_from_player(ti) <= self.location_visibility_distance;
        let occupation = self.troops[ti].occupation;
        // = seg000:7816..781f occupation bit 5 (captured) keeps the cycle menu
        //   too, unless the troop is a freed prisoner (occupation 0x22).
        if in_range && (occupation & 0x20 == 0 || occupation == 0x22) {
            if occupation & 0x40 != 0 {
                // = seg000:7821/7824 test occupation,40h; jnz — a troop on the
                //   move only takes a new destination.
                element = MenuRef::MenuChangeTroopDestination;
            } else {
                // = seg000:782a bp = menu_map_troop_dialog.
                element = MenuRef::MenuTroopDialog;
                // = seg000:782d..7838 ax = 0x52; cmp map_view_reentry_count,1;
                //   adc ax,0; [bp+12h] = ax — the last slot reads CUT CONTACT
                //   on a map opened from the map itself, and NO MORE ORDERS
                //   once the room's troop path re-entered the view (the count
                //   that also sends the verb back to the room, seg000:8763).
                self.menu_troop_dialog.records[4].text_id = if self.map_view_reentry_count == 0 {
                    cmd::CUT_CONTACT
                } else {
                    cmd::NO_MORE_ORDERS
                };
                // = seg000:783b call map_setup_troop_dialog_menu — the order
                //   menu's grey bits.
                self.map_setup_troop_dialog_menu(ti);
            }
        }
        // = seg000:783e bx = map_troop_contact_cleanup; 7841 call loc_0d323 —
        //   stage the menu over the map main menu and fold it in.
        self.stage_command_submenu(element, GameState::map_troop_contact_cleanup);
        // = seg000:7844 jmp open_onmap_resource.
        self.open_onmap_spritesheet();
    }

    // = seg000:7847 map_setup_troop_dialog_menu — the order menu's grey bits.
    // CHANGE TROOP OCCUPATION, MODIFY EQUIPMENT and MOVE TROOP all start
    // greyed and are ungreyed one by one from the troop's state.
    pub(crate) fn map_setup_troop_dialog_menu(&mut self, ti: usize) {
        // = seg000:7847 [data_02110] = 0x404f; 784d/7852 [data_02115] |= 0x40,
        //   [data_02119] |= 0x40 — the three orders greyed.
        let records = &mut self.menu_troop_dialog.records;
        records[1].text_id = CMD_GREY | cmd::CHANGE_TROOP_OCCUPATION;
        records[2].text_id |= CMD_GREY;
        records[3].text_id |= CMD_GREY;
        let troop = self.troops[ti];
        // = seg000:7857 test word ptr [si+12h],400h; jnz ret — the troop is
        //   not taking orders at all.
        if troop.dissatisfaction_and_speech & 0x400 != 0 {
            return;
        }
        // = seg000:785e..7867 al = occupation & 0xf; anything but 1 (spice
        //   prospecting) can be reassigned.
        let occupation = troop.occupation & 0x0f;
        if occupation != 1 {
            self.menu_troop_dialog.records[1].text_id &= !CMD_GREY;
        }
        // = seg000:786c..7875 occupation 2 (awaiting orders) relabels the slot
        //   SELECT TROOP OCCUPATION — the troop has no occupation to change
        //   yet — and no other order applies.
        if occupation == 2 {
            self.menu_troop_dialog.records[1].text_id = cmd::SELECT_TROOP_OCCUPATION;
            return;
        }
        // = seg000:7876..787d game_phase >= 5 ungreys MOVE TROOP.
        if self.game_phase >= 5 {
            self.menu_troop_dialog.records[3].text_id &= !CMD_GREY;
        }
        // = seg000:7882 cmp game_phase,4; jb ret — equipment handover only
        //   opens up at game_phase 4.
        if self.game_phase < 4 {
            return;
        }
        // = seg000:7889 test word ptr [si+10h],200h; jnz ret — a troop busy
        //   repairing keeps what it holds.
        if troop.bitfield_10 & 0x200 != 0 {
            return;
        }
        // = seg000:7890..789d di = [si+4]; the troop's location must be one
        //   equipment can be handed over at: status bit 3, or an appearance
        //   below 0x28.
        let li = location_index_from_ptr(troop.offset_of_location);
        let Some(location) = self.locations.get(li).copied() else {
            return;
        };
        if location.status & 8 == 0 && location.appearance >= 0x28 {
            return;
        }
        // = seg000:78a0..78b4 call compute_location_available_equipment (di =
        //   the troop's location); or the 7 buffer counts together, then or in
        //   the troop's own equipment mask ([si+19h]) — with nothing on either
        //   side there is nothing to modify.
        self.compute_location_available_equipment(li);
        let available = self.available_equipment;
        let any = available.harvesters
            | available.ornithopters
            | available.krys_knives
            | available.laser_guns
            | available.weirding_modules
            | available.atomics
            | available.bulbs
            | troop.equipment;
        if any == 0 {
            return;
        }
        // = seg000:78b6 and byte ptr [data_02115],0bfh — ungrey MODIFY
        //   EQUIPMENT.
        self.menu_troop_dialog.records[2].text_id &= !CMD_GREY;
    }

    // = seg000:8751 map_troop_contact_cleanup — the cleanup func every
    // troop-contact menu is staged with: drop the selection, its highlight
    // ring and the contact dialogue popup. Run when the menu leaves the
    // menu stack — through Cancel (menu_callback_choice_exit_menu)
    // or through the map main menu's 0xff push popping it
    // (map_setup_main_menu).
    pub(crate) fn map_troop_contact_cleanup(&mut self) {
        // = seg000:8751 cmp data_01954,0; jz ret.
        if self.map_selected_troop_id == 0 {
            return;
        }
        // = seg000:8758 call loc_069a3 — the highlight ring.
        self.map_remove_focused_troop_icon();
        // = seg000:875b data_01954 = 0 (data_01955 keeps the id, so the
        //   contact verb can resume this troop); 8760 jmp
        //   map_close_troop_contact_popup.
        self.map_selected_troop_id = 0;
        self.map_close_troop_contact_popup();
    }

    // = seg000:7b58 map_close_troop_contact_popup — tear down the contacted
    // troop's dialogue popup: drop its talking-head HUD element, then (for a
    // live contact, data_046ef) mark the contact on the troop record and
    // repaint the popup's panel rect.
    pub(crate) fn map_close_troop_contact_popup(&mut self) {
        // = seg000:7b58 ui_hud_elements[18].flags = 0 — drop the contacted
        //   troop's talking head.
        self.ui_elements[18].flags = 0;
        // = seg000:7b5e data_046f4 = 0 — the equipment spinners go with the
        //   popup (not ported, see map_open_troop_contact_dialogue).
        // = seg000:7b63..7b70 xor si,si; xchg si,[data_046ef]; jz ret — no
        //   live contact, nothing to take down. The ds:4c reset at 7b65 runs
        //   either way, BEFORE the exchange.
        let was_out_of_contact = self.contacting_troops_ds_4c != 0;
        self.contacting_troops_ds_4c = 0;
        let Some(ti) = self.map_contact_troop.take() else {
            return;
        };
        // = seg000:7b72 cmp related_to_contacting_troops_ds_4c,0; jnz — a
        //   troop that answered from outside the visibility range was never
        //   really reached, so the contact is not marked on it. (The compare
        //   reads the byte 7b65 just cleared, so it tests the value the
        //   contact ran with.)
        if !was_out_of_contact {
            // = seg000:7b79 call game_phase_set_to_64_if_conditions_met.
            self.game_phase_set_to_64_if_conditions_met(ti);
            // = seg000:7b7c/7b81 the spoken-to masks: bitfield_10 &= 0x3f0,
            //   dissatisfaction_and_speech &= 0xe5ff — the per-contact speech
            //   flags the dialogue conditions set are dropped again.
            self.troops[ti].bitfield_10 &= 0x3f0;
            self.troops[ti].dissatisfaction_and_speech &= 0xe5ff;
            // = seg000:7b86/7b89 [si+14h] = the in-game day of this contact.
            self.troops[ti].game_day_of_ralliement = self.get_ingame_day_in_ax() as u8;
        }
        // = seg000:7b8c call lip_sync_stop — stop the troop's voice.
        self.lip_sync_stop();
        // = seg000:7b8f..7b97 si = troop_contact_text_panel_record; clear
        //   map_popup_ptr and the dialogue resume cursor: the next contact
        //   starts its record over.
        self.map_popup_ptr = 0;
        self.dialogue_resume_entry_ptr = 0;
        // = seg000:7b9a call troop_icons_update_dirty_rect — repaint the map
        //   and its icons over the popup's rect.
        let r = self.map_contact_popup_rect;
        self.troop_icons_update_dirty_rect(r);
        // = seg000:7b9d/7b9f al = 4; call loc_07b2b — the popup's close effect
        //   (run_vga_effect al=4 = xor_bracket_zoom_from_panel) unless data_046d8
        //   suppresses it: the brackets shrink back and the box trail returns
        //   to the icon, over the freshly repainted map.
        if !self.map_popup_anim_suppress {
            self.xor_bracket_zoom_from_panel();
        }
    }

    // = seg000:8763 menu_callback_choice_multiple_no_more_orders — the order
    // menu's last slot. Cuts the contact, and — when the map view was opened
    // from the room's troop path (loc_05a03, which bumped
    // map_view_reentry_count) — returns to the room the orders were given
    // from. Also called by the map's LMB miss (seg000:5cdd).
    pub(crate) fn menu_callback_choice_multiple_no_more_orders(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:8763 cmp map_view_reentry_count,0; jz — the count is zeroed
        //   by the call below, so the room toggle is decided up front.
        let reentered = self.map_view_reentry_count != 0;
        // = seg000:8768/876a either way the contact is cut.
        self.menu_callback_choice_map_troop_contact_no_more_orders(0, 0);
        // = seg000:876d jmp ui_toggle_room_view.
        if reentered {
            self.ui_toggle_room_view();
        }
    }

    // = seg000:8770 menu_callback_choice_map_troop_contact_no_more_orders — the
    // cycle menu's NO MORE ORDERS slot: put the map main menu back, which pops
    // the contact menu — and with it the teardown — on the way in.
    pub(crate) fn menu_callback_choice_map_troop_contact_no_more_orders(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:8770 cmp data_01954,0; jz ret — no troop is selected, so
        //   there is no contact to end.
        if self.map_selected_troop_id == 0 {
            return;
        }
        // = seg000:877a map_view_reentry_count = 0 — the map view is the
        //   player's own again, not the room's troop detour.
        self.map_view_reentry_count = 0;
        // = seg000:877f call map_setup_main_menu — rebuild and push the map
        //   main menu. Its 0xff priority pops the 0xfc contact menu beneath
        //   it, running its map_troop_contact_cleanup.
        self.map_setup_main_menu();
        // = seg000:8782 ui_hud_elements[18].flags = 0.
        self.ui_elements[18].flags = 0;
    }

    // = seg000:69a3 loc_069a3 — remove the selected-troop highlight ring
    // (troop_icon_focused_ptr slot 0).
    pub(crate) fn map_remove_focused_troop_icon(&mut self) {
        // = seg000:69a4..69ae xor di,di; xchg di,[troop_icon_focused_ptr];
        //   call troop_icon_remove.
        if let Some(i) = self.troop_icon_focused[0].take() {
            self.troop_icon_remove(i);
        }
    }

    // = seg000:697c troop_0697c — spawn the highlight ring (the rotating
    // FOCUS_RING_SCRIPT icon, flag 0x40 so it draws last and never hit-tests)
    // over the selected troop's icon and store it in the focused slot.
    fn map_focus_troop_icon(&mut self, ti: usize) {
        // = seg000:697c call troop_leaf_fn_06917; jnz ret — the troop must
        //   have a plain icon on the map.
        if self.troop_find_icon(ti).is_none() {
            return;
        }
        // = seg000:6981..698d a stationed troop's location must have a
        //   visible marker (location_leaf_fn_05ed0).
        let t = &self.troops[ti];
        let li = location_index_from_ptr(t.offset_of_location);
        let marker = self
            .visible_location_markers
            .iter()
            .find(|m| m.location_index as usize == li)
            .copied();
        if t.occupation & 0x40 == 0 && marker.is_none() {
            return;
        }
        // = seg000:698f call troop_icon_screen_pos; jb ret.
        let Some((x, y)) = self.troop_icon_screen_pos(ti, marker.as_ref()) else {
            return;
        };
        // = seg000:6994/6997 bp = data_018fd; call troop_icon_spawn_with_anim.
        let Some(i) =
            self.troop_icon_spawn_with_anim(crate::troop_icons::FOCUS_RING_SCRIPT, x, y, ti)
        else {
            return;
        };
        // = seg000:699a flag 0x40; 699e troop_icon_focused_ptr = the icon.
        self.troop_icons[i].flags |= 0x40;
        self.troop_icon_focused[0] = Some(i);
    }

    // = seg000:7c63 troop_07c63 — the Chebyshev distance in map cells between
    // the player and the troop's gps position (the troop twin of
    // location_distance_from_player).
    fn troop_distance_from_player(&self, ti: usize) -> u16 {
        // = seg000:7c64 call get_map_position; 7c68..7c70 bp = the
        //   longitude-units-per-cell for the player's |latitude|.
        let (px, plat) = self.get_map_position();
        let tablat = self.tablat.as_ref().expect("TABLAT.BIN not loaded");
        let per_cell = tablat.lng_units_per_cell((plat + 98) as u16);
        let t = &self.troops[ti];
        // = seg000:7c74..7c8e |gps_1 - x| / bp vs |gps_2 - lat|, the larger.
        let dx = t.gps_coordinates_1.abs_diff(px) / per_cell;
        let dy = (t.gps_coordinates_2 as i16).abs_diff(plat);
        dx.max(dy)
    }

    // = seg000:78bc troop_078bc — open the troop info panel next to the
    // troop's icon.
    fn map_open_troop_info_popup(&mut self, icon_index: usize, ti: usize) {
        // = seg000:78bf..78c6 out of visibility range shows no panel.
        if self.troop_distance_from_player(ti) >= self.location_visibility_distance {
            return;
        }
        // = seg000:78c8 troop_leaf_fn_06917 — the hit-test already resolved
        //   the icon. = seg000:78cd draw to the front buffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:78d0 data_046fa = the troop.
        self.map_info_popup_troop = Some(ti);
        // = seg000:78d5..78e0 si = data_018df; the icon's top-left in dx/bx;
        //   cx = 0x64; call loc_05f25 — place + draw the empty panel.
        let (ix, iy) = {
            let r = self.troop_icons[icon_index].rect;
            (r.x0, r.y0)
        };
        // = data_018df: fill 0xf0 (+9), frame 0xfb (+8).
        self.map_info_panel_rect =
            self.map_place_popup_panel(MAP_POPUP_TROOP_INFO, ix, iy, 0x64, 0xf0, 0xfb);
        // = seg000:78e4/78e6 data_01955 = the id (not modelled); falls into
        //   loc_078e9 — the panel content.
        self.map_draw_troop_info_panel_content(ti);
        // = seg000:79db jmp set_fb1_as_active_framebuffer; the port restores
        //   the bracket and publishes the touched screen.
        self.active_fb = saved;
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:5f25 loc_05f25 — place a popup panel record next to an icon /
    // marker at (x, y): 106 px wide, `h` tall, vertically centred and clamped
    // into the map window; on the right at x+15 unless that crosses x 210,
    // then 130 px to the left. Registers the panel as map_popup_ptr, draws
    // the comm-glow pointer arrow (not modelled) and the panel fill + frame,
    // and returns the rect. `popup_id`/`fill`/`frame` come from the record
    // (data_018df for the troop info panel, data_01668 for the location one).
    fn map_place_popup_panel(
        &mut self,
        popup_id: u16,
        x: i16,
        y: i16,
        h: i16,
        fill: u8,
        frame: u8,
    ) -> Rect {
        // = seg000:5f29..5f42 the vertical clamp [4, 0x94 - h].
        let py = (y - h / 2).clamp(4, 0x94 - h);
        // = seg000:5f42..5f4f the side pick.
        let mut px = x + 0x0f;
        if px >= 0xd2 {
            px -= 0x82;
        }
        // = seg000:5f4f..5f5f the record rect (width 0x6a) + map_popup_ptr.
        let r = rect(px, py, px + 0x6a, py + h);
        self.map_popup_ptr = popup_id;
        // = seg000:5f65..5f76 store the source point (the icon / marker
        //   position, less 10) and the panel rect for the outline animation,
        //   clear the suppress flag (data_046d8 = 0), then run the outline
        //   scale-in (effect al=6, xor_rect_outline_advance).
        self.map_popup_anim_src = (x, y);
        self.map_popup_anim_rect = r;
        self.map_popup_anim_suppress = false;
        self.animate_popup_outline(false);
        // = seg000:7b1b loc_07b1b — the panel fill + frame (fill [rec+9],
        //   frame [rec+8]).
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            fill,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, frame);
        r
    }

    // = segvga:38d8 xor_rect_outline_advance / segvga:39bb _reverse (effects
    // al=6 / al=8) — the panel's XOR outline scale animation: a rectangle
    // outline that grows from the source point (map_popup_anim_src, less 2) to
    // the panel rect on open, or shrinks back to it on close. 15 frames, each
    // drawing the outline, pacing one frame, then XOR-erasing it. The close
    // repaints the map under the panel first (the caller), so the shrinking
    // outline plays over the clean map.
    fn animate_popup_outline(&mut self, reverse: bool) {
        // The animation is a foreground timing effect; headless runs skip it.
        if self.is_headless() {
            return;
        }
        // = segvga:38d8/38e0 the start corner: the source point + 8 - 10 = -2.
        let (sx, sy) = self.map_popup_anim_src;
        let start_x = sx - 2;
        let start_y = sy - 2;
        let r = self.map_popup_anim_rect;
        let pw = r.x1 - r.x0;
        let ph = r.y1 - r.y0;
        // = segvga:3920/3925 the size steps (panel extent / 16).
        let wstep = pw >> 4;
        let hstep = ph >> 4;
        // = segvga:392a..395d the top-left steps: |panel - start| / 16, signed.
        let dxstep = ((r.x0 - start_x).abs() >> 4) * (r.x0 - start_x).signum();
        let dystep = ((r.y0 - start_y).abs() >> 4) * (r.y0 - start_y).signum();
        // = segvga:3962/396c the advance starts at the source, size 0; the
        //   reverse (segvga:39bb) starts at the panel rect, size = its extent,
        //   with every step negated.
        let (mut cx, mut cy, mut cw, mut ch, dx, dy, dw, dh) = if reverse {
            (r.x0, r.y0, pw, ph, -dxstep, -dystep, -wstep, -hstep)
        } else {
            (start_x, start_y, 0, 0, dxstep, dystep, wstep, hstep)
        };
        // = segvga:397a..39b7 the 15-frame draw / pace / erase loop. DOS XORs
        //   straight onto the visible screen, so the erase is seen at once; the
        //   port presents after the draw, so publish once more after the final
        //   erase to clear the last outline (otherwise it lingers — the open
        //   path's panel fill re-presents over it, but the close leaves it).
        for _ in 0..15 {
            cx += dx;
            cy += dy;
            cw += dw;
            ch += dh;
            self.xor_rect_outline(cx, cy, cw, ch);
            self.present_transition_frame();
            self.xor_rect_outline(cx, cy, cw, ch);
        }
        if reverse {
            self.send_frame_to_display();
        }
    }

    // = segvga:3733 vga_xor_rect_outline_inner — XOR the four edges of the
    // rect at (x, y) size (w, h) into the visible screen with colour 0x0f,
    // clamped to the map area [4, 0x13c] x [4, 0x94]. XOR-drawing the same
    // rect twice erases it.
    fn xor_rect_outline(&mut self, x: i16, y: i16, w: i16, h: i16) {
        let x0 = x.clamp(4, 0x13c);
        let x1 = (x + w).clamp(4, 0x13c);
        let y0 = y.clamp(4, 0x94);
        let y1 = (y + h).clamp(4, 0x94);
        if x1 < x0 || y1 < y0 {
            return;
        }
        let yoff = self.y_offset;
        let scr = &mut self.screen;
        let mut toggle = |px: i16, py: i16| {
            let px = px as u16;
            let py = (py + yoff as i16) as u16;
            scr.set(px, py, scr.get(px, py) ^ 0x0f);
        };
        // = segvga:3784/378f/379d/37a8 the top, right, bottom, left edges.
        for px in x0..=x1 {
            toggle(px, y0);
            toggle(px, y1);
        }
        for py in y0 + 1..y1 {
            toggle(x0, py);
            toggle(x1, py);
        }
    }

    // = segvga:36b0 xor_bracket_anim_setup — stage the bracket-zoom animation
    // from the panel rect (es:di, the record's +0..+7) and the target point
    // (data_035f2/035f4): the origin of a 20x20 box centred on the panel
    // (data_035f6/035f8), the per-frame bracket expand step (half the panel
    // extent less 20, / 8 — data_035ee/035f0) and the per-frame trail step
    // from the target to that centre (/ 8, signed — data_035ea/035ec).
    fn xor_bracket_anim_setup(&mut self, panel: Rect, target: (i16, i16)) {
        // = segvga:36b0..36e7 per axis: ax = (extent - 20) / 2; centre origin
        //   = panel origin + ax; expand step = ax / 8.
        let half_w = (panel.x1 - panel.x0 - 0x14) >> 1;
        let half_h = (panel.y1 - panel.y0 - 0x14) >> 1;
        let center = (panel.x0 + half_w, panel.y0 + half_h);
        self.xor_bracket_anim_center = center;
        self.xor_bracket_anim_expand_step = (half_w >> 3, half_h >> 3);
        // = segvga:36eb..371e the trail step: (centre - target) / 8, the shift
        //   run on |value| with the sign restored (truncation toward zero).
        let step = |d: i16| (d.abs() >> 3) * d.signum();
        self.xor_bracket_anim_move_step = (step(center.0 - target.0), step(center.1 - target.1));
    }

    // = segvga:3602 xor_bracket_zoom_to_panel — the troop-contact popup's open
    // effect (vga_effect_dispatch al=2): a 20x20 XOR box stepping from the
    // troop icon to the panel centre (8 frames), then the corner brackets
    // expanding from a centred 20x20 out to the panel rect (8 frames). Each
    // phase runs twice with identical frames — the XOR draws accumulate over
    // the first pass and the second pass erases them — pacing one present
    // interval (loc_segvga_02572) per frame.
    fn xor_bracket_zoom_to_panel(&mut self, panel: Rect, icon_pos: (i16, i16)) {
        // The animation is a foreground timing effect; headless runs skip it.
        if self.is_headless() {
            return;
        }
        // = segvga:3603..361e the target: the icon point, x clamped to
        //   [0, 300], y to >= 0.
        let target = (icon_pos.0.clamp(0, 0x12c), icon_pos.1.max(0));
        self.xor_bracket_anim_setup(panel, target);
        // = segvga:362a..3652 the box trail: 2 passes x 8 frames from the
        //   target, advancing by the trail step after each frame's box
        //   (vga_xor_box_20 = the outline at fixed size 20).
        let (dx, dy) = self.xor_bracket_anim_move_step;
        for _ in 0..2 {
            let (mut x, mut y) = target;
            for _ in 0..8 {
                self.xor_rect_outline(x, y, 0x14, 0x14);
                self.present_transition_frame();
                x += dx;
                y += dy;
            }
        }
        // = segvga:3654..36ac the brackets: 2 passes x 8 frames from a centred
        //   20x20, growing by the expand step after each frame; every frame
        //   drawn is latched in data_035fa..03600 for the close to shrink
        //   back from.
        let (ex, ey) = self.xor_bracket_anim_expand_step;
        for _ in 0..2 {
            let (mut x, mut y) = self.xor_bracket_anim_center;
            let (mut w, mut h) = (0x14, 0x14);
            for _ in 0..8 {
                self.xor_bracket_anim_shape = (x, y, w, h);
                self.xor_corner_brackets(x, y, w, h);
                self.present_transition_frame();
                x -= ex;
                w += 2 * ex;
                y -= ey;
                h += 2 * ey;
            }
        }
    }

    // = segvga:3841 xor_bracket_zoom_from_panel — the troop-contact popup's close
    // effect (vga_effect_dispatch al=4), the open played backwards from the
    // state xor_bracket_zoom_to_panel staged: the brackets shrink from the last
    // latched shape back towards a centred 20x20 (2 passes x 8 frames, the
    // second pass erasing the first), then the box trail steps from the panel
    // centre back to the icon (2 passes x 8 frames).
    fn xor_bracket_zoom_from_panel(&mut self) {
        if self.is_headless() {
            return;
        }
        // = segvga:3847..388c the brackets, shrinking by the expand step
        //   after each frame.
        let (ex, ey) = self.xor_bracket_anim_expand_step;
        for _ in 0..2 {
            let (mut x, mut y, mut w, mut h) = self.xor_bracket_anim_shape;
            for _ in 0..8 {
                self.xor_corner_brackets(x, y, w, h);
                self.present_transition_frame();
                x += ex;
                w -= 2 * ex;
                y += ey;
                h -= 2 * ey;
            }
        }
        // = segvga:388e..38bc the trail, stepping back from the centre before
        //   each frame's box.
        let (dx, dy) = self.xor_bracket_anim_move_step;
        for _ in 0..2 {
            let (mut x, mut y) = self.xor_bracket_anim_center;
            for _ in 0..8 {
                x -= dx;
                y -= dy;
                self.xor_rect_outline(x, y, 0x14, 0x14);
                self.present_transition_frame();
            }
        }
    }

    // = segvga:37b1 vga_xor_corner_brackets — XOR-draw the corner
    // brackets at (x, y), size (w, h), into the visible screen with colour
    // 0x0f: a 10-pixel horizontal edge segment and a 9-pixel vertical spine
    // at each corner. DOS writes the framebuffer offsets unclamped; the port
    // skips off-screen pixels.
    fn xor_corner_brackets(&mut self, x: i16, y: i16, w: i16, h: i16) {
        let yoff = self.y_offset as i16;
        let scr = &mut self.screen;
        let mut toggle = |px: i16, py: i16| {
            let py = py + yoff;
            if (0..320).contains(&px) && (0..200).contains(&py) {
                let (px, py) = (px as u16, py as u16);
                scr.set(px, py, scr.get(px, py) ^ 0x0f);
            }
        };
        // = segvga:37ba/37cc the top and segvga:37fe/3810 the bottom edge
        //   segments (5 word XORs each = 10 pixels per corner).
        for i in 0..10 {
            toggle(x + i, y);
            toggle(x + w - 10 + i, y);
            toggle(x + i, y + h - 1);
            toggle(x + w - 10 + i, y + h - 1);
        }
        // = segvga:37d7/37f1 the right and segvga:381d/3837 the left spine
        //   segments (9 byte XORs each).
        for j in 1..10 {
            toggle(x + w - 1, y + j);
            toggle(x + w - 1, y + h - 1 - j);
            toggle(x, y + h - 1 - j);
            toggle(x, y + j);
        }
    }

    // = an interpolated COMMAND/PHRASE string (the live-number placeholders
    // read the staged CONDIT block) at a pen with a colour word — the DOS
    // font_draw_interpolated_string_w_color_at_pos.
    fn map_draw_interp_string(&mut self, id: u16, color: u16, x: u16, y: u16) {
        let s = self.get_phrase_or_command_string(id).to_vec();
        let text = self.format_interpolated_string(&s);
        self.font_state.color = color;
        self.font_set_draw_position(x, y);
        self.font_draw_string(&text);
    }

    // = seg000:78e9 loc_078e9 — the troop info panel's content: the header,
    // the location name, the occupation/status lines and the equipment row,
    // all from the staged CONDIT block.
    pub(crate) fn map_draw_troop_info_panel_content(&mut self, ti: usize) {
        // = seg000:78e9/78ee a troop that lost its icon closes the panel.
        if self.troop_find_icon(ti).is_none() {
            self.map_close_troop_info_popup();
            return;
        }
        // = seg000:78f4 call troop_prepare_troop_data_for_condit; 78f7
        //   subst_id_04 += 0xc — the panel wording of the occupation caption
        //   the 0x84 placeholder expands to (COMMAND 0x24.. "Spice Mining",
        //   "Spice Prospecting", "Awaiting orders", .. instead of the 0x18..
        //   dialogue wording).
        self.troop_prepare_troop_data_for_condit(ti);
        self.string_subst_id_table[4] += 0xc;
        // = seg000:78fc/78ff the panel fill + frame again (the refresh entry).
        let r = self.map_info_panel_rect;
        gfx::vga_fill_rect(
            self,
            self.active_fb(),
            r.x0 as u16,
            r.y0 as u16,
            r.x1 as u16,
            r.y1 as u16,
            0xf0,
        );
        self.draw_rect_outline(r.x0, r.y0, r.x1 - 1, r.y1 - 1, 0xfb);
        // = seg000:7902 the small font; 7905..7916 the header pen (x0+12,
        //   y0+4), colour 0x9a on the panel fill 0xf0 (ch = [data_018e8]).
        self.font_select_small_font();
        let header_x = (r.x0 + 12) as u16;
        let mut y = (r.y0 + 4) as u16;
        // = seg000:7919..7924 the header: "Settled in", "Going to" for a moving
        //   troop (occupation bit 6). Only the header draws at x0+12.
        let occ = self.troops[ti].occupation;
        let hdr = if occ & 0x40 != 0 {
            cmd::GOING_TO
        } else {
            cmd::SETTLED_IN
        };
        self.map_draw_interp_string(hdr, 0xf09a, header_x, y);
        // = seg000:7929 sub dx,8 — the pen drops to x0+4 for the location
        //   name AND stays there for every following line.
        let x0 = header_x - 8;
        // = seg000:7927..7933 the location name at (x0+4, y+9), colour 0x96.
        y += 9;
        let li = location_index_from_ptr(self.troops[ti].offset_of_location);
        self.draw_location_name(li, 0xf096, x0, y);
        // = seg000:7936..7938 back to 0x9a; y += 10.
        y += 10;
        if occ & 0x20 != 0 {
            // = seg000:793e..794a occupation bit 5: "Captured", "Freed
            //   Prisoner" for occupation 0x22.
            let id = if occ == 0x22 {
                cmd::FREED_PRISONER
            } else {
                cmd::CAPTURED
            };
            self.map_draw_interp_string(id, 0xf09a, x0, y);
            y += 0x11;
        } else {
            // = seg000:794c..794f the troop line: the 0x84 occupation caption
            //   (subst id 4) then "N men  Motiv. N%".
            self.map_draw_interp_string(cmd::MEN_AND_MOTIVATION, 0xf09a, x0, y);
            y += 0x0f;
            // = seg000:7955 occupation 2 skips the caption + status lines.
            if occ != 2 {
                // = seg000:795c..796b the skill caption for the occupation's
                //   skill ("On trial".."Expert"): the id at
                //   string_subst_id_table[6 + ((occ & 0xf) >> 2)]
                //   (seg001:11f7 = the table + 12), staged by
                //   troop_prepare_troop_data_for_condit.
                let idx = 6 + (((occ & 0x0f) >> 2) & 3) as usize;
                let phrase = self.string_subst_id_table[idx];
                self.font_draw_phrase_or_command_string_with_color_at_pos(phrase, 0xf09a, x0, y);
                y += 10;
                // = seg000:7971..79b9 the status line (stationed troops only).
                if occ & 0x40 == 0 {
                    let bf = self.troops[ti].bitfield_10;
                    let dissat = self.troops[ti].dissatisfaction_and_speech;
                    // = seg000:7978..79b4 the status pick. The DOS 0x100-clear
                    //   and dissatisfaction branches both land on "Inactive".
                    let id = if bf & 0x200 != 0 {
                        cmd::REPAIRING
                    } else if bf & 0x100 == 0 || dissat & 0x30 != 0 {
                        cmd::INACTIVE
                    } else if occ == 0 {
                        cmd::SPICE_RATES
                    } else if occ & 0x0f == 1 {
                        cmd::COVERED_AREA
                    } else if occ == 6 {
                        cmd::BATTLE_LOSSES
                    } else {
                        0
                    };
                    if id != 0 {
                        self.map_draw_interp_string(id, 0xf09a, x0, y);
                        y += 0x11;
                    }
                }
            }
        }
        // = seg000:79bc..79c7 the "Equipment:" header, colour 0x96.
        y += 4;
        self.font_draw_phrase_or_command_string_with_color_at_pos(cmd::EQUIPMENT, 0xf096, x0, y);
        y += 8;
        // = seg000:79ca..79d8 the equipment icon row: troop_unpack_equipment_
        //   flags (bitmask -> 0/1 per type) into the row, bottom = panel y1.
        let mask = self.troops[ti].equipment;
        let flags = std::array::from_fn(|slot| u8::from(mask & (0x80 >> slot) != 0));
        let bottom = self.map_info_panel_rect.y1;
        self.map_draw_equipment_columns(&flags, bottom, x0 as i16, y as i16);
    }

    // = seg000:7e3d loc_07e3d — the equipment row: `counts` is 7 per-type
    // ONMAP icon counts (the seg001:192f sprite table, harvesters..bulbs).
    // Each nonzero type stacks `count` icons vertically within [y, bottom]
    // (loc_061d3) and advances x by the icon width; an all-zero row draws the
    // "none" phrase. The troop info panel passes 0/1 flags
    // (troop_unpack_equipment_flags); the location popup passes real counts.
    fn map_draw_equipment_columns(&mut self, counts: &[u8; 7], bottom: i16, x0: i16, y: i16) {
        // = seg000:7e4f..7e64 nothing owned: the "none" phrase 12 px in
        //   (add dx,0ch; add bx,5) in the current colour.
        if counts.iter().all(|&c| c == 0) {
            self.font_set_draw_position((x0 + 12) as u16, (y + 5) as u16);
            self.font_draw_phrase_or_command_string(cmd::NONE);
            return;
        }
        let clip = self.map_view_clip_rect();
        let yoff = self.y_offset as i16;
        let mut x = x0;
        // = seg000:7e6b..7e93 one column per nonzero type.
        for (slot, &count) in counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let sprite = crate::troop_icons::equipment_icon_sprite(slot);
            // = seg000:61d3 loc_061d3 — read the sprite dims.
            let (mut w, mut sh) = (0i16, 0i16);
            self.with_active_bank_sheet(|_, sheet| {
                if let Some(sp) = sheet.get_sprite(sprite) {
                    w = sp.width() as i16;
                    sh = sp.height() as i16;
                }
            });
            if w == 0 {
                continue;
            }
            // = seg000:61e2..620d the vertical spacing: fit `count` icons in
            //   the available height, squeezing (min step 2) only if they do
            //   not fit at the natural step of sprite_height + 2.
            let avail = bottom - y;
            let step_full = sh + 2;
            let n = count as i16;
            let (mut draw_n, step) = if avail / n >= step_full {
                (n, step_full)
            } else {
                let s = ((avail - step_full).max(0) / n).max(2);
                if s > 2 { (n, s) } else { (avail / 2, 2) }
            };
            draw_n = draw_n.max(1).min(n);
            // = seg000:6211..6220 stack the icons.
            let mut iy = y;
            for _ in 0..draw_n {
                self.with_active_bank_sheet(|s, sheet| {
                    s.draw_sprite_from_sheet_clipped(sheet, sprite, x, iy + yoff, clip);
                });
                iy += step;
            }
            // = seg000:6224..622d advance x by the icon width.
            x += w;
        }
    }

    // = seg000:79de loc_079de — close the troop info panel: clear data_046fa
    // and map_popup_ptr, repaint the map beneath the panel rect.
    pub(crate) fn map_close_troop_info_popup(&mut self) {
        // = seg000:79e0..79e6 xchg data_046fa,0; jz -> just fb1 active.
        if self.map_info_popup_troop.take().is_none() {
            return;
        }
        // = seg000:79e8/79eb si = data_018df; loc_05f9f: fb1 active, clear
        //   map_popup_ptr, repaint the map under the panel, then the outline
        //   scale-out (loc_07b2b, effect al=8) unless suppressed.
        self.set_fb1_as_active_framebuffer();
        self.map_popup_ptr = 0;
        let r = self.map_info_panel_rect;
        self.troop_icons_update_dirty_rect(r);
        if !self.map_popup_anim_suppress {
            self.animate_popup_outline(true);
        }
    }

    // = seg000:5fb0 loc_05fb0 — an LMB click near a location marker: open the
    // location info popup, and (unless it is the player's own location or an
    // in-room/desert case that cannot travel) push a GO THERE command menu.
    fn map_click_location_marker(&mut self, li: usize) {
        // = seg000:5fb0 call loc_058fa — leave the spice-density sub-mode
        //   (not ported). = seg000:5fb3 call map_dismiss_troop_popups.
        self.map_dismiss_troop_popups();
        // = seg000:5fb6..6008 decide the GO THERE menu.
        let menu = if li == self.current_location_index as usize {
            // = seg000:5fba the player's own location: panel only.
            None
        } else if self.data_00008 == 0xff {
            // = seg000:5fbc..5ffe the desert: GO THERE RIDING A WORM, but only
            //   once the worm is available (bitfield_Paul_events bit 0x40).
            if self.bitfield_paul_events & 0x40 != 0 {
                Some(MoveMenu::Worm)
            } else {
                None
            }
        } else if self.current_room <= 2 || self.data_00008 < 0x20 {
            // = seg000:5fc3..5fd8 GO THERE FLYING AN ORNI, but only from a
            //   shallow room: current_room <= 2, or the room's appearance is
            //   below 0x20. Deep inside a location (room > 2 AND appearance
            //   >= 0x20) shows the panel with no menu (seg000:5fca/5fcf jnb;
            //   the 5fd1 appearance >= 0x28 compare is subsumed by it). The
            //   verb greys without an ornithopter at the current location
            //   (seg000:5fd9 di = [current_location_ptr], not the clicked one).
            self.compute_location_available_equipment(self.current_location_index as usize);
            Some(MoveMenu::Orni)
        } else {
            None
        };
        // = seg000:6004 call loc_0600e — draw the panel.
        self.map_draw_location_popup(li);
        // = seg000:6008/600b bx = loc_05f91; jmp loc_0d323 — push the GO
        //   THERE menu over the panel (its cleanup closes the panel).
        if let Some(menu) = menu {
            let menu_ref = match menu {
                // = seg000:5fe1..5ff4 GO THERE FLYING AN ORNI (greyed without
                //   an orni) + Cancel.
                MoveMenu::Orni => {
                    let orni_greyed = self.available_equipment.ornithopters == 0;
                    self.menu_go_there_flying_an_orni.records[0].set_grayed_if(orni_greyed);
                    MenuRef::MenuGoThereFlyingAnOrni
                }
                // = seg000:6000 GO THERE RIDING A WORM + Cancel.
                MoveMenu::Worm => MenuRef::MenuGoThereRidingAWorm,
            };
            self.screen_overlay_request_transition();
            self.menu_stack_push(menu_ref, Some(GameState::map_close_location_popup));
            self.play_pending_panel_fold();
        }
    }

    // = seg000:0600e loc_0600e — draw the location info popup: place the
    // panel next to the marker (location_05ee4), the location type + name,
    // then the class-specific extras and the equipment/battle section.
    pub(crate) fn map_draw_location_popup(&mut self, li: usize) {
        // = seg000:600e call set_screen_as_active_framebuffer.
        let saved = self.active_fb();
        self.set_screen_as_active_framebuffer();
        // = seg000:6011..6016 location_05ee4 places the panel and sets
        //   data_046f8 = the location.
        let placed = self.map_place_location_panel(li);
        self.map_location_popup_loc = Some(li);
        if !placed {
            self.active_fb = saved;
            return;
        }
        let r = self.map_location_popup_rect;
        // = seg000:601a..6034 the location type header (0x9a) at (x0+12, y0+4)
        //   and the name (0x96) at (x0+4, +9).
        self.font_select_tall_font();
        let hx = (r.x0 + 12) as u16;
        let ty = (r.y0 + 4) as u16;
        self.draw_string_location_type(li, 0xf09a, hx, ty);
        self.draw_location_name(li, 0xf096, hx - 8, ty + 9);
        // = seg000:603f..6056 the class dispatch: class 2 (Atreides /
        //   undiscovered / plain) shows only the header; class 0 with the
        //   Paul-events 0x20 water flag draws the water/spice extra first.
        let class = self.location_class(li);
        if class != 2 {
            if self.bitfield_paul_events & 0x20 != 0 && class == 0 {
                // = seg000:6052 call location_0605c — the water/spice line;
                //   niche late-game state, not ported. TODO.
                println!("map_draw_location_popup: water/spice extra (loc_0605c) not ported");
            }
            // = seg000:6056 call loc_060ac — the equipment/battle section.
            self.map_draw_location_equipment_or_battle(li);
        }
        // = seg000:6059 jmp set_fb1_as_active_framebuffer.
        self.active_fb = saved;
        if !self.front_buffer_is_fb1() {
            self.send_frame_to_display();
        }
    }

    // = seg000:5ee4 location_05ee4 — place the location panel next to the
    // location's visible marker, its height keyed on the class
    // (troop_icon_panel_heights). Returns false when the location has no
    // visible marker.
    fn map_place_location_panel(&mut self, li: usize) -> bool {
        // = seg000:5ee4 call location_find_visible_marker; jnz ret.
        let Some(m) = self
            .visible_location_markers
            .iter()
            .find(|m| m.location_index as usize == li)
            .copied()
        else {
            return false;
        };
        // = seg000:5eec/5ef1 the class + its panel height [class+11d0h].
        let class = self.location_class(li);
        self.map_location_popup_class = class + 1;
        const HEIGHTS: [i16; 4] = [0x58, 0x3c, 0x1e, 0];
        let h = HEIGHTS[(class as usize).min(3)];
        // = seg000:5f1b..5f23 the marker position; bh sign-bit off-window bail.
        if m.y < 0 {
            return false;
        }
        // = data_01668: fill 0x10 (+9), frame 0xf8 (+8).
        self.map_location_popup_rect =
            self.map_place_popup_panel(MAP_POPUP_LOCATION, m.x, m.y, h, 0x10, 0xf8);
        true
    }

    // = seg000:60ac loc_060ac — the location panel's equipment-or-battle
    // section: a "Battle:" gauge when the location has active combat
    // (location_has_battle), else the "Equipment:" header + the location's
    // own equipment column row.
    fn map_draw_location_equipment_or_battle(&mut self, li: usize) {
        self.open_onmap_spritesheet();
        self.font_select_tall_font();
        let r = self.map_location_popup_rect;
        let x0 = (r.x0 + 4) as u16;
        // = seg000:60b8 bx += 0xc — the section sits a header-height below the
        //   name; the port derives the pen from the panel top.
        let y = (r.y0 + 4 + 9 + 0xc) as u16;
        if !self.location_has_battle(li) {
            // = seg000:60c3..60d3 "Equipment:" (colour 0x9a) then the
            //   location's own equipment counts (record +0x14), bottom = y1.
            self.font_draw_phrase_or_command_string_with_color_at_pos(
                cmd::EQUIPMENT,
                0xf09a,
                x0,
                y,
            );
            let e = &self.locations[li].equipment;
            let counts = [
                e.harvesters,
                e.ornithopters,
                e.krys_knives,
                e.laser_guns,
                e.weirding_modules,
                e.atomics,
                e.bulbs,
            ];
            self.map_draw_equipment_columns(&counts, r.y1, x0 as i16, (y + 0x0a) as i16);
        } else {
            // = seg000:60d6..60f5 "Battle:" then the battle gauge sprite
            //   (0x8e + (gauge + 0xf) >> 5) at (x0+0x2f, y+6).
            self.font_draw_phrase_or_command_string_with_color_at_pos(cmd::BATTLE, 0xf09a, x0, y);
            let gauge = self.location_battle_gauge(li);
            let sprite = 0x8e + ((gauge as u16 + 0x0f) >> 5);
            let clip = self.map_view_clip_rect();
            let yoff = self.y_offset as i16;
            self.with_active_bank_sheet(|s, sheet| {
                s.draw_sprite_from_sheet_clipped(
                    sheet,
                    sprite,
                    x0 as i16 + 0x2f,
                    y as i16 + 6 + yoff,
                    clip,
                );
            });
        }
    }

    // = seg000:6252 location_06252 — the location's popup class (0 full,
    // 1 battle, 2 header-only). Battle present -> 1; Atreides -> 2; else keyed
    // on the location-type string (type 3 -> 0, type 2 -> 1, else 2);
    // undiscovered (status bit 4 clear) or no type -> 2.
    fn location_class(&mut self, li: usize) -> u8 {
        // = seg000:6252 call location_0627e; jb -> class 1.
        if self.location_has_battle(li) {
            return 1;
        }
        // = seg000:6257 Atreides -> class 2.
        if self.location_is_atreides(li) {
            return 2;
        }
        // = seg000:6260 status bit 4 (discovered) clear -> class 2.
        if self.locations[li].status & 0x10 == 0 {
            return 2;
        }
        // = seg000:6266..6278 keyed on the location-type string offset.
        match self.get_location_type_string_offset(li) {
            0 => 2,
            3 => 0,
            2 => 1,
            _ => 2,
        }
    }

    // = seg000:627e location_0627e — the location has active combat: status
    // bit 1 (a battle site), or a non-Atreides location with attacking /
    // occupation-6 troops (location_do_accumulation_on_troops).
    fn location_has_battle(&mut self, li: usize) -> bool {
        // = seg000:6281 test status,2; jnz -> battle.
        if self.locations[li].status & 2 != 0 {
            return true;
        }
        // = seg000:6287 Atreides -> no battle.
        if self.location_is_atreides(li) {
            return false;
        }
        // = seg000:628c location_do_accumulation_on_troops (05098): dx counts
        //   the occupation-6 non-Harkonnen troops.
        let mut dx = 0u16;
        self.for_each_troop_in_location(li, |s, ti| {
            let t = &s.troops[ti];
            // = seg000:5082 callback: skip occupation bit 5; Harkonnen
            //   (bitfield_10 bit 7) goes to cx; occupation 6 -> dx.
            if t.occupation & 0x20 == 0 && t.bitfield_10 & 0x80 == 0 && t.occupation == 6 {
                dx += 1;
            }
        });
        dx != 0
    }

    // = seg000:60f8 location_060f8 — the location's battle gauge (0..0xff):
    // the population-weighted balance of the two sides' skill, biased toward
    // 0x80 (even). Accumulates over the location's troops (06155).
    fn location_battle_gauge(&mut self, li: usize) -> u8 {
        // = seg000:60fe/6118 data_0d81c = the Harkonnen population sum.
        let mut hark_pop = 0u32;
        // = seg000:6155 bx = Σ harvest_total, cx = Σ harvest_rate, dx = Σ population
        //   (occupation-6 non-0x20 troops); Harkonnen add to hark_pop.
        let (mut sum_e, mut sum_c, mut attacker_pop) = (0u32, 0u32, 0u32);
        self.for_each_troop_in_location(li, |s, ti| {
            let t = &s.troops[ti];
            let pop = t.population as u32;
            if t.bitfield_10 & 0x80 != 0 {
                hark_pop += pop;
                return;
            }
            if t.occupation == 6 {
                if t.occupation & 0x20 == 0 {
                    attacker_pop += pop;
                }
                sum_c += t.harvest_rate as u32;
                sum_e += t.harvest_total as u32;
            }
        });
        // = seg000:6108..6114 bx = Σfield_e / (Σfield_e's pop? ) — the DOS
        //   `add bx,dx; div bx` averages harvest_total over the attacker pop.
        let avg_e = (sum_e << 8).checked_div(sum_e + attacker_pop).unwrap_or(0) as u16;
        // = seg000:6116..6126 cx = Σfield_c / (Σfield_c + hark_pop).
        let avg_c = (sum_c << 8).checked_div(sum_c + hark_pop).unwrap_or(0) as u16;
        // = seg000:6128..6143 the balance: si = max(avg_e, avg_c); the signed
        //   difference over si, halved and biased +0x80.
        let hi = avg_e.max(avg_c) as i32;
        let diff = avg_e as i32 - avg_c as i32;
        let bal = if hi != 0 { (diff * 0x80 / hi) >> 1 } else { 0 };
        (bal + 0x80).clamp(0, 0xff) as u8
    }

    // = seg000:5f91 loc_05f91 — the GO THERE menu's cleanup: clear the
    // location popup gate (data_046f8/046f7) and close the panel.
    pub(crate) fn map_close_location_popup(&mut self) {
        // = seg000:5f91/5f97 data_046f8 = 0; data_046f7 = 0.
        if self.map_location_popup_loc.take().is_none() {
            return;
        }
        self.map_location_popup_class = 0;
        // = seg000:5f9c si = data_01668; loc_05f9f: fb1 active, clear
        //   map_popup_ptr, repaint the map under the panel, then the outline
        //   scale-out (loc_07b2b, effect al=8) unless suppressed.
        self.set_fb1_as_active_framebuffer();
        self.map_popup_ptr = 0;
        let r = self.map_location_popup_rect;
        self.troop_icons_update_dirty_rect(r);
        if !self.map_popup_anim_suppress {
            self.animate_popup_outline(true);
        }
    }

    // = seg000:50db menu_callback_choice_move_to_location_orni — GO THERE
    // FLYING AN ORNI: set ornithopter travel and confirm it.
    pub(crate) fn menu_callback_choice_move_to_location_orni(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        // = seg000:50db travel_vehicle_mode = 2; map_ornithopter_mode = 1.
        self.travel_vehicle_mode = 2;
        self.map_ornithopter_mode = 1;
        // = seg000:50e6 al = 4 (the orni mode flag); fall into the shared
        //   confirm tail.
        self.map_move_to_location_confirm(4);
    }

    // = seg000:50ea menu_callback_choice_move_to_location_worm — GO THERE
    // RIDING A WORM: the worm setup (loc_04285, CALL A WORM) is not ported.
    pub(crate) fn menu_callback_choice_move_to_location_worm(
        &mut self,
        _text_id: u16,
        _index: usize,
    ) {
        println!("GO THERE RIDING A WORM: the worm setup (loc_04285) is not ported");
    }

    // = seg000:50ef map_move_to_location_confirm — the shared GO THERE tail:
    // aim the pending travel at the popup's location, commit the room move
    // that boards the vehicle, tear the map down and confirm the travel
    // (map_confirm_travel_and_close).
    fn map_move_to_location_confirm(&mut self, mode_flag: u8) {
        // = seg000:50ef di = data_046f8 — the destination location.
        let Some(li) = self.map_location_popup_loc else {
            return;
        };
        let destination = crate::locations::location_ptr(li as u16);
        // = seg000:50f5 call dismiss_stacked_overlays; 50f8 travel_reset_trail.
        self.dismiss_stacked_menus();
        self.travel_reset_trail();
        // = seg000:50fb/50ff dx = location_and_room, bx = location_appearance —
        //   the room the player is standing in, read after the overlays are
        //   dismissed.
        let mut new_room = self.location_and_room;
        let new_appearance = self.location_appearance;
        // = seg000:5105..5109 cmp al,4; jnz loc_0510b; mov dl,1 — the orni
        //   mode boards from the location's outdoor room 1 (the pad), so the
        //   move commits room 1 whatever room the verb was used from. The worm
        //   mode keeps the current room.
        if mode_flag == 4 {
            new_room = (new_room & 0xff00) | 1;
        }
        // = seg000:510b call callback_transition_04057 — commit the room move.
        //   data_046eb still carries the map's bit 7 here, so this only records
        //   the destination globals; the room redraw is the ui_toggle_room_view
        //   below.
        self.commit_room_move(new_room, new_appearance);
        // = seg000:510e call ui_toggle_room_view — back to the room view (the
        //   0x34 transition renders the just-committed room).
        self.ui_toggle_room_view();
        // = seg000:5111/5112 game_screen_mode_flags = al — set after the
        //   toggle, so the nav panel above still installs from the pre-travel
        //   flags.
        self.game_screen_mode_flags = mode_flag;
        // = seg000:5115/5116 pop di; jmp map_confirm_travel_and_close. A
        //   location destination ignores the x/y args (arm_pending_travel aims
        //   at the location ptr).
        self.map_confirm_travel_and_close(destination, 0, 0);
    }

    // = seg000:599f map_main_mouse_release — end a popup panel drag
    // (data_04723). Panel dragging is not ported yet.
    pub(crate) fn dune_map_mouse_release(&mut self) {}

    // = seg000:59c1 map_main_mouse_drag — move a dragged popup panel. Panel
    // dragging is not ported yet.
    pub(crate) fn dune_map_mouse_drag(&mut self, _dx: i16, _dy: i16) {}

    // = seg000:0f66 nullsub_00f66 — the rmb_release slot.
    pub(crate) fn dune_map_mouse_noop(&mut self) {}

    // = seg000:0f66 nullsub_00f66 — the rmb_drag slot.
    pub(crate) fn dune_map_mouse_drag_noop(&mut self, _dx: i16, _dy: i16) {}
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{
        GameState, cmd,
        dat_file::DatFile,
        menu_defs::{CMD_GREY, MenuRef},
    };

    // SEE DUNE MAP from the room screen: the full-planet map renders into the
    // (4,4)-(316,148) window, the map main menu becomes the active screen
    // element, and EXIT MAPS returns to the room verb menu. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored see_dune_map
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn see_dune_map_renders_and_exits() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Stage troop icons: at the raw game start every troop still carries
        // occupation bit 7 (unrallied — "Number of rallied troops = 0"), and
        // those only show the 0x181f flag icon once their location's status
        // has bit 4. Give troop 1 (at location 12) a plain occupation so the
        // basic animated icon spawns, and flag troop 2's location (13).
        game.troops[0].occupation = 0x01;
        game.locations[13].status |= 0x10;

        // The SEE DUNE MAP verb (record seg001:220c, handler 0x186b).
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}

        assert_eq!(game.data_046eb, 0x80, "full-map mode owns the screen");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMapTroops,
            "the map main menu is the active element"
        );
        assert_eq!(
            game.menu_map_troops.records[0].text_id,
            cmd::EXIT_MAPS,
            "EXIT MAPS"
        );
        // The visible location markers at the game start carry stationed
        // troops, so the troop icon renderer spawns icons for them.
        eprintln!("troop icons spawned: {}", game.troop_icons.len());
        for ic in &game.troop_icons {
            eprintln!(
                "  icon sprite {:#04x} rect ({},{})-({},{}) flags {:#04x}",
                ic.sprite, ic.rect.x0, ic.rect.y0, ic.rect.x1, ic.rect.y1, ic.flags
            );
        }
        assert!(!game.troop_icons.is_empty(), "troop icons on the map");
        // With location_visibility_distance still 1 the contact slot is GIVE
        // ORDERS TO TROOP, greyed because the current location (index 0 at the
        // start) has no troop chain.
        assert_eq!(
            game.menu_map_troops.records[1].text_id,
            CMD_GREY | cmd::GIVE_ORDERS_TO_TROOP,
            "GIVE ORDERS TO TROOP greyed with no troop at the current location"
        );
        game.framebuffer
            .write_png(&game.palette, "troop_map_screen.png")
            .unwrap();
        game.screen
            .write_png(&game.palette, "troop_map_screen_popup.png")
            .unwrap();
        // The map window carries the map palette bank (0x10..0x20) everywhere
        // but the black polar-edge corners and the overlaid markers; probe the
        // equator row's ends and a mid-latitude column off the player marker.
        for (x, y) in [(4u16, 76u16), (315, 76), (80, 40), (240, 110)] {
            let p = game.framebuffer.get(x, y);
            assert!(
                (0x10..0x20).contains(&p),
                "map pixel at ({x},{y}) = {p:#04x} outside the map bank"
            );
        }

        // The rallied-troops popup stays up across idle passes until 1000
        // ticks after game_clock_tick_base (stamped by the room present and
        // by every button edge in the live loop).
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_RALLIED, "popup open");
        game.dune_map_mouse_idle();
        assert_eq!(
            game.map_popup_ptr,
            super::MAP_POPUP_RALLIED,
            "popup survives an early idle pass"
        );
        // Simulate the timeout by rewinding the stamp: the next idle pass
        // dismisses the popup and restores the map beneath it.
        game.game_clock_tick_base = (game.game_ticks() as u16).wrapping_sub(1000);
        game.dune_map_mouse_idle();
        assert_eq!(
            game.map_popup_ptr, 0,
            "popup auto-dismissed after 1000 ticks"
        );

        // The troop icon animation task (armed by the open): every 4th firing
        // steps the armed icons' scripts — the worker icon cycles its 4
        // sprites and repaints through the fb2 restore.
        let animated = game
            .troop_icons
            .iter()
            .position(|ic| ic.flags & 1 != 0)
            .expect("an animated troop icon");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..16 {
            game.tick_troop_icon_anim();
            let s = game.troop_icons[animated].sprite;
            assert!(
                (0x17..=0x1a).contains(&s),
                "the worker icon stays in its 4-sprite cycle (got {s:#04x})"
            );
            seen.insert(s);
        }
        assert!(seen.len() >= 2, "the animated icon cycled through sprites");
        while rx.try_recv().is_ok() {}

        // The troop popups: give troop 1 a gps position at the player so the
        // info panel's visibility gate passes.
        let (px, plat) = game.get_map_position();
        game.troops[0].gps_coordinates_1 = px;
        game.troops[0].gps_coordinates_2 = plat as u16;
        // RMB over the unrallied troop's flag icon reports as a miss —
        // nothing opens.
        game.mouse_pos_x = 207;
        game.mouse_pos_y = 29;
        game.dune_map_mouse_rmb();
        assert_eq!(game.map_popup_ptr, 0, "no popup for an unrallied troop");
        // RMB over the rallied worker icon opens the troop info panel.
        game.mouse_pos_x = 241;
        game.mouse_pos_y = 145;
        game.dune_map_mouse_rmb();
        assert_eq!(
            game.map_popup_ptr,
            super::MAP_POPUP_TROOP_INFO,
            "the troop info panel is open"
        );
        assert_eq!(game.map_info_popup_troop, Some(0));
        // The panel's men/motivation line opens with the 0x84 placeholder:
        // string_subst_id_table[4] = (occupation & 0xf) + 0x18, bumped by 0xc
        // to the panel wording — occupation 1 gives "Spice Prospecting".
        assert_eq!(
            game.string_subst_id_table[4], 0x25,
            "the occupation caption id staged for the panel"
        );
        let line = game
            .get_phrase_or_command_string(cmd::MEN_AND_MOTIVATION)
            .to_vec();
        let text = game.format_interpolated_string(&line);
        assert!(
            text.starts_with(b"Spice Prospecting"),
            "the 0x84 placeholder expands to the occupation caption: {:?}",
            String::from_utf8_lossy(&text)
        );
        while rx.try_recv().is_ok() {}
        game.screen
            .write_png(&game.palette, "troop_map_screen_info.png")
            .unwrap();
        // A second RMB on the same icon toggles the panel closed.
        game.dune_map_mouse_rmb();
        assert_eq!(game.map_popup_ptr, 0, "the info panel toggled closed");
        assert_eq!(game.map_info_popup_troop, None);
        // LMB on the icon selects the troop: data_01954 + the rotating
        // highlight ring in the focused slot (flag 0x40, on top).
        game.dune_map_mouse_lmb();
        assert_eq!(game.map_selected_troop_id, 1, "troop 1 selected");
        let ring = game.troop_icon_focused[0].expect("the highlight ring icon");
        assert!(game.troop_icons[ring].flags & 0x40 != 0, "ring draws last");
        // The anim task steps the focused ring every firing (not just every
        // 4th), painting it onto the screen.
        game.tick_troop_icon_anim();
        while rx.try_recv().is_ok() {}
        game.screen
            .write_png(&game.palette, "troop_map_screen_selected.png")
            .unwrap();

        // Click a location marker (loc_05fb0). Location 11's marker has no
        // troop icon over it (its troop is unrallied), so the icon hit-test
        // does not intercept. From a SHALLOW room (current_room <= 2) the
        // popup folds in the GO THERE FLYING AN ORNI menu.
        let marker = game
            .visible_location_markers
            .iter()
            .find(|m| m.location_index == 11)
            .copied()
            .expect("location 11's marker visible");
        game.current_room = 2;
        game.mouse_pos_x = marker.x as u16;
        game.mouse_pos_y = marker.y as u16;
        game.dune_map_mouse_lmb();
        assert_eq!(
            game.map_location_popup_loc,
            Some(11),
            "the location popup is open"
        );
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_LOCATION);
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuGoThereFlyingAnOrni,
            "the GO THERE menu is folded in from a shallow room"
        );
        assert_eq!(
            game.menu_go_there_flying_an_orni.records[0].text_id & 0xff,
            0x59,
            "GO THERE FLYING AN ORNI"
        );
        while rx.try_recv().is_ok() {}
        game.screen
            .write_png(&game.palette, "troop_map_screen_location.png")
            .unwrap();
        // Cancel (the menu's second verb) pops the menu; its cleanup closes
        // the info panel.
        game.menu_callback_choice_exit_menu(0xa3, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_location_popup_loc, None, "location popup closed");
        assert_eq!(game.map_popup_ptr, 0);
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMapTroops,
            "back to the map main menu"
        );

        // Deep inside a location (current_room > 2 AND appearance >= 0x20 —
        // the palace case) the same click shows the panel with NO GO THERE
        // menu: an ornithopter is not reachable from deep rooms.
        game.current_room = 0x0a; // data_00008 is already 0x20
        game.dune_map_mouse_lmb();
        assert_eq!(game.map_location_popup_loc, Some(11), "the panel is open");
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_LOCATION);
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMapTroops,
            "no GO THERE menu deep inside a location"
        );
        while rx.try_recv().is_ok() {}

        // A click INSIDE the open panel does not dismiss it (it routes to the
        // panel, whose controls are stubbed).
        let pr = game.map_location_popup_rect;
        game.mouse_pos_x = ((pr.x0 + pr.x1) / 2) as u16;
        game.mouse_pos_y = ((pr.y0 + pr.y1) / 2) as u16;
        game.dune_map_mouse_lmb();
        assert_eq!(
            game.map_location_popup_loc,
            Some(11),
            "an inside-panel click keeps the popup open"
        );
        // A click on empty map space (>= 20 px from every marker, off the icons
        // and outside the panel) dismisses the popup.
        let empty = (4..316i16)
            .flat_map(|x| (4..148i16).map(move |y| (x, y)))
            .find(|&(x, y)| {
                !pr.in_rect(x, y)
                    && game.troop_icon_hit_test(x, y).is_none()
                    && game
                        .visible_location_markers
                        .iter()
                        .all(|m| (m.x - x).abs() + (m.y - y).abs() >= 20)
            })
            .expect("an empty map point");
        game.mouse_pos_x = empty.0 as u16;
        game.mouse_pos_y = empty.1 as u16;
        game.dune_map_mouse_lmb();
        assert_eq!(
            game.map_location_popup_loc, None,
            "a click on empty map space dismisses the popup"
        );
        assert_eq!(game.map_popup_ptr, 0);
        while rx.try_recv().is_ok() {}

        // Scroll one step north (the alt nav panel's up arrow, live only
        // while the LMB is down): the shared zoomed centre moves 12 latitude
        // rows and the redraw presents the shifted map from the fb2 snapshot.
        let lat_before = game.zoomed_globe_latitude;
        game.prev_mouse_buttons = 1;
        game.ui_click_map_up();
        game.prev_mouse_buttons = 0;
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.zoomed_globe_latitude,
            lat_before - 12,
            "scrolled north"
        );

        // The panel outline scale animation (xor_rect_outline_advance): the
        // XOR outline draw self-erases (a second XOR restores the pixels), the
        // property the draw/pace/erase loop depends on. Probe a top-edge pixel
        // (the outline draws in y_offset space, like the panel fill).
        let probe = (60u16, 30 + game.y_offset);
        let before = game.screen.get(probe.0, probe.1);
        game.xor_rect_outline(50, 30, 40, 30);
        assert_ne!(
            game.screen.get(probe.0, probe.1),
            before,
            "the outline toggled the border pixel"
        );
        game.xor_rect_outline(50, 30, 40, 30);
        assert_eq!(
            game.screen.get(probe.0, probe.1),
            before,
            "a second XOR erases the outline"
        );
        // The contact popup's zoom brackets (vga_xor_corner_brackets): corner
        // segments only — a 10-px edge piece and a 9-px spine per corner —
        // leaving gaps mid-edge and mid-spine, and self-erasing like the
        // outline.
        let yoff = game.y_offset;
        let corner = game.screen.get(59, 30 + yoff);
        let edge_gap = game.screen.get(65, 30 + yoff);
        let spine_gap = game.screen.get(50, 45 + yoff);
        game.xor_corner_brackets(50, 30, 40, 30);
        assert_ne!(
            game.screen.get(59, 30 + yoff),
            corner,
            "the corner segment toggled"
        );
        assert_eq!(
            game.screen.get(65, 30 + yoff),
            edge_gap,
            "the mid-edge gap is untouched"
        );
        assert_eq!(
            game.screen.get(50, 45 + yoff),
            spine_gap,
            "the mid-spine gap is untouched"
        );
        game.xor_corner_brackets(50, 30, 40, 30);
        assert_eq!(
            game.screen.get(59, 30 + yoff),
            corner,
            "a second XOR erases the brackets"
        );
        // Overlay three grow frames (near the marker, mid, at the panel rect)
        // to visualise the scale from the marker point to the info-panel rect.
        let (sx, sy) = (241i16, 145i16);
        let panel = crate::rect::rect(226, 116, 296, 148);
        for t in [3i16, 8, 14] {
            let cx = (sx - 2) + (panel.x0 - (sx - 2)) * t / 15;
            let cy = (sy - 2) + (panel.y0 - (sy - 2)) * t / 15;
            let cw = (panel.x1 - panel.x0) * t / 15;
            let ch = (panel.y1 - panel.y0) * t / 15;
            game.xor_rect_outline(cx, cy, cw, ch);
        }
        game.screen
            .write_png(&game.palette, "troop_map_screen_outline.png")
            .unwrap();

        // EXIT MAPS (menu_map_main record 0) back to the room.
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        assert_eq!(game.data_046eb, 0, "back in the plain room view");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::CommandMenuBuf,
            "the room verb menu is active again"
        );
    }

    // GO THERE FLYING AN ORNI boards from the location's outdoor room 1 (the
    // ornithopter pad), whatever inner room the map was opened from:
    // map_move_to_location_confirm forces dl = 1 into location_and_room and
    // commits the move (seg000:5105..510b) before ui_toggle_room_view redraws
    // the room behind the closing map. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored go_there_orni
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn go_there_orni_boards_from_the_outdoor_room() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Walk up into an inner palace room (the equipment room is room 6 of
        // the starting location's appearance group); the pad is room 1.
        let inner_room = (game.location_and_room & 0xff00) | 6;
        game.commit_room_move(inner_room, game.location_appearance);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.current_room, 6, "standing in an inner room");

        // SEE DUNE MAP, then GO THERE FLYING AN ORNI on a location popup.
        game.ui_toggle_room_view();
        game.map_location_popup_loc = Some(13);
        game.menu_callback_choice_move_to_location_orni(0, 0);
        while rx.try_recv().is_ok() {}

        // location_and_room already carries the first travel step's desert
        // longitude by now (seg000:4760 travel_advance_step); current_room /
        // previous_room are only written by the commit, so they record which
        // room the flight departed from.
        assert_eq!(
            game.current_room, 1,
            "the departure commits the outdoor room 1, not the room the map was opened from"
        );
        assert_eq!(game.previous_room, 6, "the inner room became previous_room");
        // The confirm chain folded the map mode bit into the travel bit and
        // armed the travel pump.
        assert_eq!(game.game_screen_mode_flags & 3, 1, "orni travel mode");
        assert_eq!(game.travel_active, 0xff, "the travel pump is armed");
        // The departure's rebuild_and_draw_room_nav_panel (through
        // ui_draw_room_command_panel, seg000:2eec) installs the travel panel
        // for the new mode: this flight homes on a location, so the compass
        // clears instead of keeping the room move-direction buttons the map
        // screen was opened over.
        let o = crate::game_ui::NAV_PANEL_RECORD_OFFSET;
        assert_eq!(game.travel_no_location_dest, 0, "homing on the destination");
        for i in 0..6 {
            assert_eq!(
                game.ui_elements[o + i].func_ptr,
                0x0f66,
                "nav record {i} has no handler while homing",
            );
        }
    }

    // The map main menu's TAKE AN ORNITHOPTER slot (seg000:42d9): commits the
    // player into the current location's outdoor room 1, leaves the full-map
    // view through ui_toggle_room_view, and opens the travel map in
    // ornithopter (cockpit) mode via the shared notransition tail. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored map_menu_take_an_ornithopter
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn map_menu_take_an_ornithopter_boards_from_the_pad() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Stand in an inner room (room 6) so the room-1 commit is observable.
        let inner_room = (game.location_and_room & 0xff00) | 6;
        game.commit_room_move(inner_room, game.location_appearance);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.current_room, 6, "standing in an inner room");

        // SEE DUNE MAP, then the map menu's TAKE AN ORNITHOPTER.
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        assert_eq!(game.data_046eb, 0x80, "full-map mode owns the screen");
        game.menu_callback_choice_map_main_take_an_ornithopter(cmd::TAKE_AN_ORNITHOPTER, 0);
        while rx.try_recv().is_ok() {}

        // The commit boarded from the pad room 1, rotating the inner room
        // into previous_room.
        assert_eq!(game.location_and_room, inner_room & 0xff00 | 1);
        assert_eq!(game.current_room, 1, "boarded from the outdoor room 1");
        assert_eq!(game.previous_room, 6, "the inner room became previous_room");
        // The notransition tail opened the cockpit map view.
        assert_eq!(game.map_ornithopter_mode, 1, "cockpit mode");
        assert_eq!(game.game_screen_mode_flags, 4, "map-travel screen mode");
        assert_eq!(game.travel_vehicle_mode, 2, "travel by ornithopter");
        assert_eq!(game.data_046eb, 1, "the travel map view owns the screen");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuCancel,
            "the travel map's Cancel menu is the active element"
        );
    }

    // CONTACT FREMEN TROOPS (menu_callback_choice_map_main_contact_fremen_
    // troops): the verb selects a troop and opens its contact verb menu, NEXT
    // TROOP cycles the selection by troop id, and CUT CONTACT puts the map
    // main menu back — the push that pops the contact menu and runs its
    // cleanup. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored contact_fremen_troops
    // The prospector troop's spice-map scene: SPECIALIZE IN SPICE on
    // troops[2] (which starts at Carthag-Timin) presents "We're unbeatable in
    // spice prospecting…", whose event 0x03 installs the below-phase-0x14
    // continue-sequence and puts the " Continue…" panel up. The first
    // Continue raises the spice-density overlay and speaks "Here, take this
    // map of the planet."; the second drops it and speaks "You can update
    // this map…"; the third ends the scene. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored prospector_spice_map
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn prospector_spice_map_scene_plays() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // The prospector troop starts at Carthag-Timin (first_name 2 /
        // last_name 4).
        let home = crate::locations::location_index_from_ptr(game.troops[2].offset_of_location);
        assert_eq!(
            (
                game.locations[home].first_name,
                game.locations[home].last_name
            ),
            (2, 4),
            "the prospectors start at Carthag-Timin"
        );

        // Contact them on the map with an occupation other than prospecting,
        // so SPECIALIZE IN SPICE is a real change.
        game.troops[2].occupation = 0x02;
        game.number_of_rallied_troops = 1;
        game.location_visibility_distance = 4;
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        let (px, plat) = game.get_map_position();
        game.troops[2].gps_coordinates_1 = px;
        game.troops[2].gps_coordinates_2 = plat as u16;
        game.map_selected_troop_id = game.troops[2].troop_id;
        game.map_select_troop();
        while rx.try_recv().is_ok() {}

        // CHANGE TROOP OCCUPATION stages the occupation submenu over the
        // order menu — the verb below closes that submenu, so the order menu
        // (and with it the selection) survives the scene, as in the game.
        game.menu_callback_choice_map_troop_dialogue_change_troop_occupation(
            cmd::CHANGE_TROOP_OCCUPATION,
            0,
        );
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuSelectTroopOccupation,
            "the occupation submenu is up"
        );

        // SPECIALIZE IN SPICE: occupation 1, the first line, and the scene's
        // " Continue…" panel (the prospector variant, with the WHAT? slot).
        game.menu_callback_choice_troop_occupation_specialize_in_spice(cmd::SPECIALIZE_IN_SPICE, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.troops[2].occupation & 0x0f,
            1,
            "the prospectors take spice prospecting, not mining"
        );
        let line1 = game.current_subtitle_id;
        assert_ne!(line1, 0, "the first line was presented");
        assert!(game.sequence_script.is_some(), "the scene script installed");
        assert!(game.is_dialogue_active, "the scene owns the screen");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuProspectorContinue,
            "the prospector Continue panel is up"
        );
        assert_eq!(game.data_046eb & 0x40, 0, "no overlay yet");

        // Continue: the spice-density overlay comes up and the second line
        // plays over it.
        game.mouse_pos_x = 0;
        game.mouse_pos_y = 0;
        game.menu_callback_choice_continue_for_sequence(0, 0);
        while rx.try_recv().is_ok() {}
        assert_ne!(game.data_046eb & 0x40, 0, "the spice-density overlay is up");
        // (The panel registers itself in a popup slot as it is raised
        // (seg000:551d..5535); the contact popup the line then rebuilds takes
        // the primary slot back, exactly as the DOS interleaving does, so the
        // resting slot is not asserted here.)
        let line2 = game.current_subtitle_id;
        assert_ne!(line2, line1, "the second line was presented");
        // The overlay window carries the spice-field colours: the panel's map
        // window is filled with the backdrop 0x70 plus per-field shades, not
        // the map bank the plain view uses.
        let (ox, oy) = game.map_overlay_panel_pos;
        let yoff = game.y_offset;
        let window: Vec<u8> = (oy + 8..oy + 8 + 0x57)
            .flat_map(|y| (ox + 6..ox + 6 + 0x9e).map(move |x| (x, y)))
            .map(|(x, y)| game.screen.get(x as u16, (y as u16) + yoff))
            .collect();
        assert!(window.contains(&0x70), "the overlay backdrop is drawn");
        assert!(
            window.iter().any(|&p| (0x50..=0x5f).contains(&p)),
            "and the density-ramp field shades on top of it"
        );
        // The legend strip below the window (loc_05605/loc_0563e): the 0xf5
        // band with the SPICE DENSITY label and ramp bars on it.
        let legend: Vec<u8> = (ox + 6..ox + 6 + 0x9e)
            .map(|x| game.screen.get(x as u16, (oy + 0x63) as u16 + yoff))
            .collect();
        assert!(legend.contains(&0xf5), "the legend strip is drawn");
        game.screen
            .write_png(&game.palette, "prospector_spice_map_overlay.png")
            .unwrap();

        // Continue again: the overlay goes away and the closing line plays.
        game.menu_callback_choice_continue_for_sequence(0, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.data_046eb & 0x40, 0, "the overlay is down");
        assert_eq!(
            game.map_view_rect,
            super::FULL_MAP_VIEW_RECT,
            "the map window is the full-map one again"
        );
        assert_ne!(
            game.current_subtitle_id, line2,
            "the closing line was presented"
        );

        // The last Continue reads the 0xff fence: the scene ends and the
        // contacted troop's order menu comes back.
        game.menu_callback_choice_continue_for_sequence(0, 0);
        while rx.try_recv().is_ok() {}
        assert!(game.sequence_script.is_none(), "the scene ended");
        assert!(!game.is_dialogue_active, "the screen is handed back");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuTroopDialog,
            "the troop's order menu is back"
        );
    }

    // MOVE TROOP (seg000:8064): from an open contact, the verb switches into
    // the destination-pick mode; a click on a location marker makes the troop
    // speak its acknowledgement, issue the move order (occupation bit 6,
    // 7-sub-step head start) and drop back to the map main menu. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored move_troop
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn move_troop_orders_a_contacted_troop_to_a_marker() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // The contact preamble of contact_fremen_troops_opens_and_cycles_the_
        // order_menu: troop 1 rallied and in range, contacted via the map
        // menu, its order menu up.
        game.troops[0].occupation = 0x01;
        game.troops[0].motivation = 80;
        game.number_of_rallied_troops = 1;
        game.location_visibility_distance = 4;
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        let (px, plat) = game.get_map_position();
        game.troops[0].gps_coordinates_1 = px;
        game.troops[0].gps_coordinates_2 = plat as u16;
        game.menu_callback_choice_map_main_contact_fremen_troops(cmd::CONTACT_FREMEN_TROOPS, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_selected_troop_id, 1, "troop 1 contacted");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuTroopDialog,
            "the order menu is up"
        );

        // MOVE TROOP: the pick mode, its Cancel menu, and the instruction
        // caption in the popup box.
        game.menu_callback_choice_multiple_move_troop(cmd::MOVE_TROOP, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuCancel,
            "the destination-pick Cancel menu is up"
        );
        assert!(
            std::ptr::eq(
                game.active_mouse_handlers,
                &super::MOVE_TROOP_MOUSE_HANDLERS
            ),
            "the pick mouse handlers are installed"
        );

        // A click on a marker of another location orders the troop there —
        // one 14+ latitude rows out, so the order's 7-sub-step head start
        // leaves the dominant-axis distance at or above the arrival radius
        // (7) and the troop is still moving afterwards. The pick runs over
        // the spice-density overlay the caption raised, so the marker
        // coordinates are the ones that overlay's window rebuilt.
        let old_ptr = game.troops[0].offset_of_location;
        let troop_lat = game.troops[0].gps_coordinates_2 as i16;
        let target = game
            .visible_location_markers
            .iter()
            .find(|m| {
                m.mode == 0x40
                    && crate::locations::location_ptr_from_index(m.location_index as usize)
                        != old_ptr
                    && game.locations[m.location_index as usize]
                        .map_y
                        .abs_diff(troop_lat)
                        > 13
            })
            .copied()
            .expect("a distant destination marker in the overlay window");
        let dest_ptr = crate::locations::location_ptr_from_index(target.location_index as usize);
        let old_li = crate::locations::location_index_from_ptr(old_ptr);
        game.mouse_pos_x = target.x as u16;
        game.mouse_pos_y = target.y as u16;
        game.move_troop_pick_lmb();
        while rx.try_recv().is_ok() {}

        assert_ne!(
            game.troops[0].occupation & 0x40,
            0,
            "the troop is moving (the order was accepted)"
        );
        assert_eq!(
            game.troops[0].offset_of_location, dest_ptr,
            "toward the picked destination"
        );
        assert_eq!(game.troops[0].position, 0, "the position slot cleared");
        assert_ne!(
            game.locations[old_li].troop_id, game.troops[0].troop_id,
            "unlinked from the old location's chain"
        );
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMapTroops,
            "back at the map main menu"
        );
        assert!(
            std::ptr::eq(game.active_mouse_handlers, &super::DUNE_MAP_MOUSE_HANDLERS),
            "the map mouse handlers are back"
        );
        assert_eq!(game.map_selected_troop_id, 0, "the contact ended");

        // A click outside the map window in pick mode cancels instead: enter
        // the mode again through the moving troop's CHANGE DESTINATION (the
        // widened visibility keeps the now-travelling troop in range).
        game.location_visibility_distance = 99;
        game.map_selected_troop_id = 1;
        game.map_select_troop();
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuChangeTroopDestination,
            "a moving troop gets the CHANGE DESTINATION menu"
        );
        game.menu_callback_choice_multiple_move_troop(cmd::CHANGE_DESTINATION, 0);
        while rx.try_recv().is_ok() {}
        game.mouse_pos_x = 10;
        game.mouse_pos_y = 190;
        game.move_troop_pick_lmb();
        while rx.try_recv().is_ok() {}
        assert_ne!(
            game.get_active_menu_ref(),
            MenuRef::MenuCancel,
            "the outside click cancelled the pick mode"
        );
        assert_eq!(
            game.troops[0].offset_of_location, dest_ptr,
            "the destination is unchanged"
        );
    }

    // The prospector variant: MOVE TROOP on troops[2] collects up to three
    // destinations in the working queue (sietches or Atreides areas only)
    // and Done copies them back and orders the troop to the head.
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn move_prospectors_queues_destinations() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // The prospector troop rallied, selected and in range on the map.
        game.troops[2].occupation = 0x01;
        game.troops[2].motivation = 80;
        game.number_of_rallied_troops = 1;
        game.location_visibility_distance = 4;
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        let (px, plat) = game.get_map_position();
        game.troops[2].gps_coordinates_1 = px;
        game.troops[2].gps_coordinates_2 = plat as u16;
        game.map_selected_troop_id = game.troops[2].troop_id;
        game.map_select_troop();
        while rx.try_recv().is_ok() {}

        // MOVE TROOP brings up the prospector menu with an empty queue.
        game.menu_callback_choice_multiple_move_troop(cmd::MOVE_TROOP, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMoveProspectors,
            "the prospector destination menu is up"
        );
        assert_eq!(game.prospector_pick_count, 0);

        // Click two sietch markers: both queue, the menu stays up. Far ones
        // (14+ latitude rows, not the prospector's own location) so the
        // order's head start does not already arrive.
        let own = game.troops[2].offset_of_location;
        let troop_lat = game.troops[2].gps_coordinates_2 as i16;
        let sietches: Vec<_> = game
            .visible_location_markers
            .iter()
            .filter(|m| {
                let li = m.location_index as usize;
                m.mode == 0x40
                    && game.locations[li].appearance < 0x20
                    && crate::locations::location_ptr_from_index(li) != own
                    && game.locations[li].map_y.abs_diff(troop_lat) > 13
            })
            .take(2)
            .copied()
            .collect();
        assert_eq!(sietches.len(), 2, "two sietch markers visible");
        for m in &sietches {
            game.mouse_pos_x = m.x as u16;
            game.mouse_pos_y = m.y as u16;
            game.move_troop_pick_lmb();
            while rx.try_recv().is_ok() {}
        }
        assert_eq!(game.prospector_pick_count, 2, "two destinations queued");
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMoveProspectors,
            "still collecting"
        );

        // Done copies the working queue back into the live destination queue
        // (seg000:8221..822c, before the troop is asked) and closes the pick
        // menu. Whether the move itself issues then depends on the troop's
        // answer: the acknowledgement line's event can drop the interrupt
        // gate, which refuses the order (seg000:8238) — that is the branch
        // this troop's dialogue takes here, so the assertions stop at the
        // queue. The accepted-order path is covered by move_troop_orders_a_
        // contacted_troop_to_a_marker.
        game.menu_callback_choice_map_move_prospectors_done(cmd::DONE, 0);
        while rx.try_recv().is_ok() {}
        let head = crate::locations::location_ptr_from_index(sietches[0].location_index as usize);
        let second = crate::locations::location_ptr_from_index(sietches[1].location_index as usize);
        assert_eq!(game.prospector_destinations[0], head, "the queue head");
        assert_eq!(game.prospector_destinations[1], second, "the second stop");
        assert_ne!(
            game.get_active_menu_ref(),
            MenuRef::MenuMoveProspectors,
            "the destination menu closed"
        );
    }

    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn contact_fremen_troops_opens_and_cycles_the_order_menu() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Two troops with icons on the map (as in see_dune_map_renders_and_
        // exits), a rallied count so the verb is not a no-op, and a visibility
        // distance of 4 so the map menu offers CONTACT FREMEN TROOPS over the
        // whole planet rather than GIVE ORDERS TO TROOP at the current
        // location.
        game.troops[0].occupation = 0x01;
        game.locations[13].status |= 0x10;
        game.number_of_rallied_troops = 2;
        game.location_visibility_distance = 4;
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.menu_map_troops.records[1].text_id,
            cmd::CONTACT_FREMEN_TROOPS,
            "the contact slot is CONTACT FREMEN TROOPS, ungreyed with icons up"
        );
        // Rally the second icon's troop too, and park both at the player's own
        // position so troop_07c63's distance stays inside the visibility range.
        let second = game.troop_icons[1].troop_index;
        game.troops[second].occupation = 0x00;
        let (px, plat) = game.get_map_position();
        for ti in [0, second] {
            game.troops[ti].gps_coordinates_1 = px;
            game.troops[ti].gps_coordinates_2 = plat as u16;
        }
        let second_id = game.troops[second].troop_id;

        // The verb with nothing selected and nothing contacted yet starts the
        // cycle from id 0, so the lowest rallied troop id wins.
        game.menu_callback_choice_map_main_contact_fremen_troops(cmd::CONTACT_FREMEN_TROOPS, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_selected_troop_id, 1, "troop 1 contacted");
        assert_eq!(game.map_last_selected_troop_id, 1, "latched in data_01955");
        assert!(
            game.troop_icon_focused[0].is_some(),
            "the highlight ring is up over the selection"
        );
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuTroopDialog,
            "a stationary troop in range gets the full order menu"
        );
        // The contact popup is up over the map, opposite the troop's icon
        // (troop 1's icon sits in the lower half, so the popup takes the top),
        // and one dialogue line has been presented into it.
        assert_eq!(game.map_contact_troop, Some(0), "the popup is troop 1's");
        assert_eq!(game.map_popup_ptr, super::MAP_POPUP_TROOP_CONTACT);
        assert_eq!(
            game.map_contact_popup_rect,
            crate::rect::rect(5, 5, 232, 72),
            "the popup takes the half the icon is not in"
        );
        assert_eq!(game.map_contact_head_rect, crate::rect::rect(9, 8, 70, 69));
        assert_ne!(game.current_subtitle_id, 0, "a line was presented");
        assert_ne!(
            game.dialogue_resume_entry_ptr, 0,
            "the resume cursor moved into the record"
        );
        // The panel fill (0xfb) is on screen, and the head box carries the
        // troop's portrait: FRM2 for a plain Fremen troop, re-anchored into
        // the 0x3b box, so most of the box is head pixels rather than its
        // 0xe4 fill.
        let yoff = game.y_offset;
        assert_eq!(game.screen.get(200, 10 + yoff), 0xfb, "the popup panel");
        assert_eq!(
            game.talking_head.as_ref().map(|h| h.lip_sync_resource_id),
            Some(0x0f),
            "the generic Fremen head speaks for the troop"
        );
        let box_rect = game.map_contact_head_rect;
        let head_pixels = (box_rect.y0 + 1..box_rect.y1 - 1)
            .flat_map(|y| (box_rect.x0 + 1..box_rect.x1 - 1).map(move |x| (x, y)))
            .filter(|&(x, y)| game.screen.get(x as u16, (y + yoff as i16) as u16) != 0xe4)
            .count();
        assert!(
            head_pixels > 1000,
            "the head is drawn into the box (only {head_pixels} non-fill pixels)"
        );
        // An icon-animation repaint under the popup (troop_icon_anim_task ->
        // troop_icons_update_dirty_rect) preserves the popup's pixels
        // (loc_0c7d4): repaint a rect straddling the whole panel, then step
        // the anim task itself, and check the panel fill and the head are
        // still on screen.
        game.troop_icons_update_dirty_rect(crate::rect::rect(0, 0, 260, 100));
        for _ in 0..4 {
            game.tick_troop_icon_anim();
        }
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.screen.get(200, 10 + yoff),
            0xfb,
            "the popup panel survives an icon repaint beneath it"
        );
        let head_pixels_after = (box_rect.y0 + 1..box_rect.y1 - 1)
            .flat_map(|y| (box_rect.x0 + 1..box_rect.x1 - 1).map(move |x| (x, y)))
            .filter(|&(x, y)| game.screen.get(x as u16, (y + yoff as i16) as u16) != 0xe4)
            .count();
        assert!(
            head_pixels_after > 1000,
            "the head survives an icon repaint beneath it (only {head_pixels_after} pixels)"
        );
        // The last slot reads CUT CONTACT while map_view_reentry_count is 0,
        // and occupation nibble 1 (spice prospecting) keeps CHANGE TROOP
        // OCCUPATION greyed.
        assert_eq!(
            game.menu_troop_dialog.records[4].text_id,
            cmd::CUT_CONTACT,
            "CUT CONTACT on a map opened from the map itself"
        );
        assert_eq!(
            game.menu_troop_dialog.records[1].text_id,
            CMD_GREY | cmd::CHANGE_TROOP_OCCUPATION,
            "a prospecting troop cannot be reassigned"
        );

        // ASK FOR MORE INFORMATION asks the same troop for its next line: the
        // popup stays this troop's and the resume cursor moves on.
        let first_line = game.current_subtitle_id;
        let resume = game.dialogue_resume_entry_ptr;
        game.menu_callback_choice_map_troop_dialogue_ask_for_more_information(
            cmd::ASK_FOR_MORE_INFORMATION,
            0,
        );
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_contact_troop, Some(0), "still troop 1's popup");
        assert!(
            game.dialogue_resume_entry_ptr != resume || game.current_subtitle_id != first_line,
            "the verb moved the conversation on"
        );

        // CHANGE TROOP OCCUPATION opens the submenu for the troop's class. A
        // prospecting troop (occupation nibble 1, class 0) gets the spice
        // menu: GO & SEARCH FOR EQUIPMENT (greyed below game_phase 0x10),
        // SPECIALIZE IN ARMY, SPECIALIZE IN ECOLOGY (greyed without the
        // Paul-events 0x20 bit) and Cancel.
        game.menu_callback_choice_map_troop_dialogue_change_troop_occupation(
            cmd::CHANGE_TROOP_OCCUPATION,
            0,
        );
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuOccupationForSpiceTroop,
            "the occupation submenu is up over the order menu"
        );
        let occ: Vec<u16> = game
            .menu_occupation_for_spice_troop
            .records
            .iter()
            .map(|r| r.text_id)
            .collect();
        assert_eq!(
            occ,
            vec![
                CMD_GREY | cmd::GO_SEARCH_FOR_EQUIPMENT,
                cmd::SPECIALIZE_IN_ARMY,
                CMD_GREY | cmd::SPECIALIZE_IN_ECOLOGY,
                cmd::CANCEL,
            ],
            "the spice troop's occupation menu"
        );
        game.screen
            .write_png(&game.palette, "troop_map_screen_occupation.png")
            .unwrap();
        // Cancel pops it back to the order menu (the contact menu is 0xfc, the
        // submenu 0xf8, so the push deepened the stack rather than replacing).
        game.menu_callback_choice_exit_menu(cmd::CANCEL, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuTroopDialog,
            "Cancel returns to the order menu"
        );
        assert_eq!(game.map_selected_troop_id, 1, "the contact survives Cancel");
        // An ecology troop (nibble 8, class 2) gets the ecology menu instead,
        // and Paul's ecology knowledge ungreys SPECIALIZE IN ECOLOGY where it
        // is offered.
        game.troops[0].occupation = 0x08;
        game.bitfield_paul_events |= 0x20;
        game.game_phase = 0x10;
        game.menu_callback_choice_map_troop_dialogue_change_troop_occupation(
            cmd::CHANGE_TROOP_OCCUPATION,
            0,
        );
        while rx.try_recv().is_ok() {}
        let occ: Vec<u16> = game
            .menu_occupation_for_ecology_troop
            .records
            .iter()
            .map(|r| r.text_id)
            .collect();
        assert_eq!(
            occ,
            vec![
                cmd::GO_SEARCH_FOR_EQUIPMENT,
                cmd::ASSEMBLY_WIND_TRAP,
                cmd::SPECIALIZE_IN_SPICE,
                cmd::SPECIALIZE_IN_ARMY,
                cmd::CANCEL,
            ],
            "the ecology troop's occupation menu, GO & SEARCH ungreyed at game_phase 0x10"
        );
        // The occupation apply itself (troop_set_occupation): the occupation byte, the
        // class bit in the speech word (0x2000 << class), the restarted clocks
        // and the re-derived "working" flag.
        game.troops[0].equipment |= 0x80;
        game.game_time = 0x1234;
        game.troops[0].harvest_rate = 0x1111;
        game.troop_set_occupation(0, 4);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.troops[0].occupation, 4, "military training");
        assert_eq!(
            game.troops[0].dissatisfaction_and_speech & 0x4000,
            0x4000,
            "the army class marked in the speech word (0x2000 << 1)"
        );
        assert_eq!(
            game.troops[0].time_period_of_ralliement, 0x1234,
            "the occupation clocks restarted"
        );
        assert_eq!(game.troops[0].harvest_rate, 0, "the accumulators cleared");
        assert_eq!(
            game.troops[0].bitfield_10 & 0x100,
            0x100,
            "military training has no viability test, so the troop is working"
        );
        // Irrigation (8) does have one: without water, the location's status
        // bit 5 and the troop's own bulbs it cannot work here, so the working
        // flag stays clear and occupation bit 4 (the "stopped" bit the icon
        // script reads) goes on.
        game.troops[0].equipment &= !2;
        game.troop_set_occupation(0, 8);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.troops[0].occupation, 0x18, "irrigation, but stopped");
        assert_eq!(
            game.troops[0].bitfield_10 & 0x100,
            0,
            "an unworkable occupation leaves the troop inactive"
        );

        // SPECIALIZE IN ARMY through the verb: the troop is asked, and a line
        // whose event clears the interrupt gate is its refusal — which puts the
        // whole occupation byte back. Either way the submenu closes to the
        // order menu. (Which way it goes is the dialogue record's call, so the
        // test asserts the outcome that matches the gate.)
        game.troops[0].occupation = 0x08;
        let before = game.troops[0].occupation;
        game.menu_callback_choice_troop_occupation_specialize_in_army(cmd::SPECIALIZE_IN_ARMY, 0);
        while rx.try_recv().is_ok() {}
        if game.dialogue_interrupt_gate == 0xff {
            assert_eq!(game.troops[0].occupation & 0x0f, 4, "the troop agreed");
            assert_eq!(
                game.troops[0].equipment & 0x80,
                0,
                "leaving the spice class hands the harvester back"
            );
        } else {
            assert_eq!(
                game.troops[0].occupation & 0x0f,
                before & 0x0f,
                "a refusal puts the old occupation back"
            );
        }
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuTroopDialog,
            "the occupation submenu closed to the order menu"
        );
        // An army troop's submenu is the army one, with the greys rebuilt for
        // the new occupation.
        game.troops[0].occupation = 0x04;
        game.menu_callback_choice_map_troop_dialogue_change_troop_occupation(
            cmd::CHANGE_TROOP_OCCUPATION,
            0,
        );
        while rx.try_recv().is_ok() {}
        let occ: Vec<u16> = game
            .menu_occupation_for_army_troop
            .records
            .iter()
            .map(|r| r.text_id)
            .collect();
        assert_eq!(
            occ,
            vec![
                cmd::GO_SEARCH_FOR_EQUIPMENT,
                CMD_GREY | cmd::ESPIONAGE_5F,
                cmd::SPECIALIZE_IN_SPICE,
                cmd::SPECIALIZE_IN_ECOLOGY,
                cmd::CANCEL,
            ],
            "the army troop's occupation menu, ESPIONAGE greyed with no Harkonnen area near"
        );
        game.menu_callback_choice_exit_menu(cmd::CANCEL, 0);
        game.troops[0].occupation = 0x01;
        while rx.try_recv().is_ok() {}

        // NEXT TROOP walks to the next id above the selection, then wraps.
        game.menu_callback_choice_map_troop_contact_next_troop(cmd::NEXT_TROOP, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.map_selected_troop_id, second_id,
            "cycled to the next id"
        );
        // The second troop's occupation is 0 (spice mining), so its CHANGE
        // TROOP OCCUPATION slot ungreys.
        assert_eq!(
            game.menu_troop_dialog.records[1].text_id,
            cmd::CHANGE_TROOP_OCCUPATION,
            "a mining troop can be reassigned"
        );
        game.menu_callback_choice_map_troop_contact_next_troop(cmd::NEXT_TROOP, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_selected_troop_id, 1, "wrapped back to troop 1");

        // CUT CONTACT: map_setup_main_menu's 0xff push pops the contact menu,
        // whose cleanup drops the selection and the ring. data_01955 keeps the
        // id so the verb can resume it.
        game.menu_callback_choice_multiple_no_more_orders(cmd::CUT_CONTACT, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuMapTroops,
            "back to the map main menu"
        );
        assert_eq!(game.map_selected_troop_id, 0, "selection dropped");
        assert_eq!(game.map_last_selected_troop_id, 1, "data_01955 kept");
        assert!(game.troop_icon_focused[0].is_none(), "the ring is gone");
        assert_eq!(game.data_046eb, 0x80, "still on the map view");

        // The verb again resumes the last contacted troop rather than starting
        // the cycle over.
        game.menu_callback_choice_map_main_contact_fremen_troops(cmd::CONTACT_FREMEN_TROOPS, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_selected_troop_id, 1, "resumed troop 1");
        game.screen
            .write_png(&game.palette, "troop_map_screen_contact.png")
            .unwrap();

        // A troop on the move (occupation bit 6) gets the change-destination
        // menu instead of the order menu.
        game.menu_callback_choice_multiple_no_more_orders(cmd::CUT_CONTACT, 0);
        game.troops[0].occupation |= 0x40;
        game.menu_callback_choice_map_main_contact_fremen_troops(cmd::CONTACT_FREMEN_TROOPS, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.map_selected_troop_id, 1);
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuChangeTroopDestination,
            "a moving troop only takes a new destination"
        );

        // Out of visibility range (troop_07c63 > location_visibility_distance)
        // only the cycle menu is offered.
        game.menu_callback_choice_multiple_no_more_orders(cmd::CUT_CONTACT, 0);
        game.troops[0].occupation = 0x01;
        game.troops[0].gps_coordinates_2 = (plat + 50) as u16;
        game.map_selected_troop_id = 1;
        game.map_select_troop();
        while rx.try_recv().is_ok() {}
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuNextTroop,
            "a troop out of visibility range can only be cycled past"
        );
    }

    // The full-planet renderer's interpolation seed reads the map cell BEFORE
    // the row's rotation offset (segvga:20a5 `add si,ax; mov al,[si-1]`), so a
    // longitude that lands the offset on cell 0 reads the byte preceding the
    // row. Rotate the view through a full turn at three latitudes to cover
    // every row length's boundary longitude. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored map_view_longitude_rotation
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn map_view_longitude_rotation_renders_every_step() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}
        game.ui_toggle_room_view();
        while rx.try_recv().is_ok() {}

        for lat in [-0x4b, 0, 0x4b] {
            game.zoomed_globe_latitude = lat;
            for step in 0..512u32 {
                game.zoomed_globe_longitude = (step * 128) as u16;
                game.map_draw_zoomed_globe();
                while rx.try_recv().is_ok() {}
            }
        }
        // 0x4000 puts the 176-cell equator rows' rotation offset exactly on
        // cell 0 — the case that used to index past the row start.
        game.zoomed_globe_latitude = 0;
        game.zoomed_globe_longitude = 0x4000;
        game.map_draw_zoomed_globe();
        while rx.try_recv().is_ok() {}
        game.framebuffer
            .write_png(&game.palette, "troop_map_screen_rotated.png")
            .unwrap();
        for (x, y) in [(4u16, 76u16), (315, 76), (80, 40), (240, 110)] {
            let p = game.framebuffer.get(x, y);
            assert!(
                (0x10..0x20).contains(&p),
                "map pixel at ({x},{y}) = {p:#04x} outside the map bank"
            );
        }
    }

    // GIVE ORDERS TO TROOP from the dialogue panel (seg000:5a03): the Fremen
    // leader being talked to is contacted on the full-planet map instead, and
    // the visit is marked as the room's detour so the order menu offers NO MORE
    // ORDERS (which returns to the room) rather than CUT CONTACT. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored give_orders_to_troop
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn give_orders_to_troop_opens_the_map_on_that_troop() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Stand in a conversation with the room's Fremen-2 person, whose slot
        // the room-entry classification filled with a rallied troop.
        let ti = 0;
        game.troops[ti].occupation = 0x01;
        game.fremen2_troops[0] = Some(ti);
        game.selected_fremen2 = 0;
        game.number_of_rallied_troops = 1;
        game.location_visibility_distance = 4;
        // Within contact range, so the verb opens the full order menu rather
        // than the out-of-range cycle menu.
        let (px, plat) = game.get_map_position();
        game.troops[ti].gps_coordinates_1 = px;
        game.troops[ti].gps_coordinates_2 = plat as u16;
        let troop_id = game.troops[ti].troop_id;

        game.menu_callback_choice_give_orders_to_troop(cmd::GIVE_ORDERS_TO_TROOP, 0);
        while rx.try_recv().is_ok() {}

        // The map view owns the screen, with that troop selected and contacted.
        assert_eq!(game.data_046eb, 0x80, "the full-map view is up");
        assert_eq!(
            game.map_selected_troop_id, troop_id,
            "the troop being talked to is the map's selection"
        );
        assert_eq!(game.map_contact_troop, Some(ti), "its contact popup is up");
        assert!(
            game.troop_icon_focused[0].is_some(),
            "the highlight ring is over it"
        );
        // = seg000:5a06 the re-entry count: the order menu's last slot is NO
        //   MORE ORDERS, and choosing it goes back to the room.
        assert_eq!(game.map_view_reentry_count, 1);
        assert_eq!(
            game.menu_troop_dialog.records[4].text_id,
            cmd::NO_MORE_ORDERS,
            "NO MORE ORDERS, not CUT CONTACT"
        );
        assert_eq!(
            game.get_active_menu_ref(),
            MenuRef::MenuTroopDialog,
            "the order menu is up"
        );

        // NO MORE ORDERS returns to the room it was given from.
        game.menu_callback_choice_multiple_no_more_orders(cmd::NO_MORE_ORDERS, 0);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.data_046eb, 0, "back in the room view");
        assert_eq!(game.map_view_reentry_count, 0, "the detour is over");
    }
}
