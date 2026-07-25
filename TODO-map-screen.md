# Map screen — remaining work

Status as of 2026-07-17. The TAKE AN ORNITHOPTER verb (seg000:42e9) is ported
and wired end-to-end in `crates/dune/src/travel_map_screen.rs`: the screen opens with
the ORNYPAN cockpit, the windowed one-cell-per-pixel map (curved globe edges),
the alternate nav panel, the Cancel menu, and the "select destination"
narration clip; Cancel restores the room; hovering resolves markers/compass
rays into the label strip, and clicking a destination narrates it, plays the
ornithopter takeoff, closes back to the room and flies there: the travel pump
drives the MNT flight HNM with the minimap + trail in the top-right, lands
at the destination and plays the arrival approach video / orni landing.
Verify with
`cargo test -p dune --bin dune -- --ignored ornithopter` (writes
`crates/dune/ornithopter_map.png`),
`cargo test -p dune --bin dune -- --ignored map_hover`,
`cargo test -p dune --bin dune -- --ignored map_scrolling` and
`cargo test -p dune --bin dune -- --ignored travel_flight` (writes
`crates/dune/travel_flight.png` + `travel_arrival.png`).

Everything below is stubbed (each stub carries its `= seg000:xxxx` link) or
not yet started.

## Map screen content

- [x] **Location markers** — DONE 2026-07-16:
  `map_build_and_draw_location_markers` (seg000:5dce) rebuilds
  `visible_location_markers` (= `data_0a5c0`) and draws the tiered ICONES
  markers (`calc_location_marker_sprite`, base 0x3a, +5 distant-sietch variant
  past `location_visibility_distance`) through the projection chain
  `location_visible_on_map` (62c9) → `map_position_to_screen_if_visible`
  (62d6) → `map_position_to_screen` (b647, using
  `Tablat::lng_units_per_cell` = the seg001:4880 table). The globe-mode
  rotated-offset precompute (`loc_0633b`) is still TODO with the full globe.
  Hover NAME labels belong to the mouse-handler item below.
- [x] **Player marker blink task** — DONE 2026-07-16:
  `map_arm_player_marker_task` (seg000:445d) + `tick_map_player_marker`
  (seg000:44ab, `TaskId::MapPlayerMarker`, interval 0x12c): the "you are
  here" ICONES sprite 0x4c blinks over the player's projected position,
  restoring from fb1 on even phases; `map_screen_cleanup` removes it.
- [x] **"SELECT DESTINATION ON MAP" caption** — DONE 2026-07-15: the
  typewriter (`map_caption_frame_task`, seg000:46b5 — one glyph per firing,
  spaces free, high-bit terminator) is ported as `tick_map_caption` +
  `TaskId::MapCaption`; arm/disarm (seg000:4658/469b) fill the
  `map_caption_*` fields (= `data_0473f..data_04747`). Remaining sub-item:
  the cursor save/restore bracket around each glyph
  (`restore_mouse_if_rect_intersects` on the `data_014a4` strip /
  `draw_mouse_cursor_if_needed`) once the live cursor can sit over the
  caption.
- [x] **Tablat cell caching** — RESOLVED 2026-07-16, no port needed:
  `map_copy_window_row` (seg000:b7e3) is the only writer and
  `map_screen_to_position` (seg000:b62c, only caller `arm_pending_travel`
  seg000:4950) is the only reader of the +6 scratch word — the port already
  recomputes the identical truncating `len * lng >> 16` cell at the read
  site, and the value can never go stale because every longitude change
  (scroll/recentre/open) redraws before any click can read it. The globe
  path reuses +4..+7 as a different quantity entirely (the band's 32-bit
  fixed-point rotated offset, `Tablat::rotated_offset` — written by
  vga_globe_init, read at seg000:b627/bb27/bd9c), so there is no shared
  cache to model. Both `.chani` comments (b7d2/b5f9) now document the
  writer/reader pair.

## Interaction

- [x] **Live mouse handlers** — DONE 2026-07-16 (`mouse_handlers_01ac8`,
  wired in `MAP_MOUSE_HANDLERS`; verify with
  `cargo test -p dune --bin dune -- --ignored map_hover`):
  - idle/drag hover tracker `map_mouse_hover_tracker` (seg000:4586, was
    loc_04586) — the hover state in `data_046fc` (location ptr / desert
    compass ray 0xfff0+n / 0xffff in-window / 0 outside) via
    `find_nearest_location_marker` (5e6d) + `compass_angle_from_delta`
    (514e), and the hover label strip `map_draw_hover_label` (45de) with
    `draw_string_location_type`/`draw_location_name` (629d/62a6).
  - LMB destination click `map_mouse_lmb_select_destination` (seg000:450e,
    was loc_0450e) — narrates the destination
    (`map_hover_narration_clip` 456c, `duck_music_and_start_narration_
    voice_clip` ab45, `wait_for_narration_voice_clip` aba9), blinks the
    label 9 times, sets `data_04732 = 0x80` and enters the travel-confirm
    chain (`map_confirm_travel_and_close`, DONE — see Travel below).
  - the cursor shape switch (hand + the 4 travel arrows,
    seg001:260c/2650/2694/26d8) — `get_mouse_cursor_image` now ports the
    full seg000:dc6a hot-zone chain off `mouse_nav_rect`.
- [x] **Map scrolling** — DONE 2026-07-16: the alternate nav panel buttons
  (NAV_PANEL_ALT, now wired with handlers: `ui_click_map_center` seg000:5b05,
  `ui_click_map_up/right/down/left` 8829/8824/882e/881f) add the
  `map_scroll_delta_*` pairs (seg001:145e — ±12 latitude rows, ±0x1002
  longitude units; LMB-gated by `test al,1`) to
  `zoomed_globe_longitude/latitude` and redraw through
  `map_refresh_main_view` (seg000:8850, was loc_08850). The arrow-cursor
  pseudo-element dispatch (`set_di_to_ui_elements_ptr_based_on_cursor_image`,
  seg000:d694 — an arrow cursor unconditionally hits live records 13..17) is
  ported inside `hit_test_ui_elements`, and the arrows' 0x4000 flag rides the
  already-ported held auto-repeat (seg000:d8da). Verify with
  `cargo test -p dune --bin dune -- --ignored map_scrolling`. Remaining stubs:
  `map_dismiss_troop_popups` (seg000:7b36 — the data_046f8/046fa
  troop-contact gates are unmodelled, SEE DUNE MAP territory), the
  full-globe scroll branches (seg000:8858 / loc_05beb) and the centre
  button's globe-0x40 paths (loc_082a0 ZF → loc_0541f, the loc_05575 nav
  rect).
- [x] **`current_main_view_drawing_function` hookup** (`_word_23B9D`) — DONE
  2026-07-16: `GameState::current_main_view_drawing_function`
  (`Option<fn(&mut GameState)>`, None = the initial 0 word; DOS never clears
  it). `map_screen_open` installs `map_view_redraw` (seg000:4346) and
  `map_refresh_main_view` dispatches through it (seg000:8853). The other two
  installers and three dispatch sites belong to unported flows and land with
  them: the travel departure installs `loc_049a0` (seg000:499a) and
  dispatches at seg000:49e6; SEE DUNE MAP installs
  `ui_main_view_map_interface` (seg000:5a8f); the
  run_events_for_current_time_period tail dispatches via `loc_05d6d`
  (seg000:5d7e, the `data_046ec` gate) and CONTACT FREMEN TROOPS at
  seg000:86c6. `map_view_redraw`'s screen push also matches DOS now: it
  presents only the map window rect (`update_screen_at_sprite_rect_updating_
  head`, seg000:c4ed = `present_screen_rect` on the `loc_05b93` sprite clip
  rect), keeping the screen-only caption/hover strip intact across scrolls.
- [x] **`set_mouse_nav_rect`** (seg000:4331) — DONE 2026-07-16 with the
  mouse handlers: `mouse_nav_rect` (= `mouse_nav_rect_ptr`, seg001:dc58), installed on map open,
  cleared by `map_screen_cleanup`, consumed by `get_mouse_cursor_image`.

## Travel

- [x] **Travel confirm** (`map_confirm_travel_and_close`, seg000:4703, was
  loc_04703) — DONE 2026-07-16 (covered by the `map_hover` test's click):
  `arm_pending_travel` (4944: `travel_destination_ptr`/`travel_heading`/
  `travel_heading_mode`, the desert-ray click through
  `map_screen_to_position` (b5f9) → `compass_angle_to_map_position` (5133),
  `adjust_travel_heading` (5119)), the mode-flag fold (`flags |= flags >>
  2`), the map-screen pop (the cleanup's seg000:443b gate now keeps the
  flags while `travel_destination_ptr` is armed), `ungrey_skip_to_
  destination_verb` (41c5, via the new `cmd_skip_to_destination_flags`
  template byte), the fb2 room snapshot (`copy_game_rect_fb1_to_fb2`,
  c474 — renamed, it copies fb1 INTO fb2), `run_travel_departure_npc_scans`
  (40d5), the ornithopter takeoff animation (`play_travel_departure_
  transition` 4795 → `orni_anim_loop` 47fb → `orni_anim_draw_frame` 4821,
  ORNYTK.HSQ + SN6.VOC), the first `travel_advance_step` (4b3b), the scene
  reload (loc_02dbf) and the pump arm (`data_04727 = 0xff`, now named
  `travel_active`). Remaining stubs inside it, each with its seg link:
  - `run_events_for_n_time_periods` (seg000:0fd9) on every 16th step;
  - the phase-0x50 branch of `play_travel_departure_transition`
    (seg000:47a0..47ca, the worm/globe flows);
  - the companion detach callees (`NPC_09556`/`NPC_09655`) in
    `npc_travel_detach_companion` (40e6);
  - the night-attack teardown (`loc_00b21`).
- [x] **Travel departure** — DONE 2026-07-16 (verify with
  `cargo test -p dune --bin dune -- --ignored travel_flight`, writes
  `crates/dune/travel_flight.png` + `travel_arrival.png`): the room draw's
  travel branch (loc_037f4) builds the flight view — the minimap + trail in
  the NEW back framebuffer (`FbId::Back` = `_word_2D0E2_framebuffer_back`)
  via `travel_minimap_setup`/`travel_minimap_redraw` (seg000:4988/49a0, which
  install themselves as the main-view drawing function) and opens the flight
  HNM by the vehicle id (`hnm_load_first_frame_by_id`, MNT1 = 2). The travel
  pump (`travel_pump`, seg000:4f0c, was game_loop_sub_04f0c) drives one HNM
  frame per game-loop pass — presented through `hnm_present_flight_frame`
  (seg000:4afd: minimap restore rect from the back buffer over fb1, then the
  game-area push; the decoder now writes index-0 pixels for the bit-0x30
  full-screen-copy clips) — and a `travel_advance_step` every 0x300 ticks
  with the REAL movement math (`travel_step_position` seg000:5206 +
  `travel_update_heading` 51cb + `travel_heading_deltas` 5198: homing/fixed
  headings, the 0x100-per-row step accumulator, the polar flip), the trail
  stamp/append (`travel_trail_stamp_last`/`travel_trail_append`, 4a1a/4a00),
  the auto-recenter (`travel_minimap_state` 1 when the marker leaves the
  inset, consumed via `travel_refresh_view` 49d9), the live marker (ICONES
  0x30), the terrain probe → flight-clip select (`travel_probe_terrain_
  ahead`/`travel_select_flight_video`, 4e8e/4ec6; the HNM loop point now
  switches clips when `hnm_active_video_id` differs, seg000:cb7c), and the
  arrival (seg000:4fb0: pad landing, disarm, `desert_check_arrival` =
  loc_04002 → `arrive_at_location` → scene reload). `map_screen_cleanup`'s
  `travel_minimap_state > 0` gate re-enters the flight view
  (`travel_enter_minimap_view`, 49d4). `travel_flyover_detect` (seg000:41e1)
  is ported but its data_01968/196a/196c silhouette-array latch is vestigial
  in this build (no draw consumer exists). The real fly-over cabin — ORNYCAB
  over the game area + the companion talking head when the flight passes a
  revealed sietch/landmark — is ported: `travel_scan_nearby_location`
  (40f9), `travel_settle_companion_dispatch` (35e9, wired into `travel_pump`),
  `travel_pick_speaking_companion` (366f), `travel_show_companion_cabin`
  (368b), the companion's spoken line `travel_play_flyover_line`
  (loc_096d8: the fixed dialogue block 0x10 topic 4, presented over the
  companion head via `present_dialogue_line_with_auto_mask`), and the
  follow-up command menu `install_pending_room_action_menu` (loc_03551: the
  GO TOWARDS THIS PLACE menu for action 3 and the CHANGE DESTINATION / IGNORE
  WARNING menu + nav-panel rebuild for action 4, at seg001:1f92 / seg001:1f9e,
  staged as `NpcActionsMenu` via the `stage_command_submenu` = loc_0d323
  helper). loc_03551 is shared: `room_person_present_auto_dialogue` (3520)
  falls into it for the room-leave speaker branch, so `npc_auto_dialogue` now
  calls it too. Remaining tail: the divert-verb payload pre-armed in loc_040f9
  (`arm_pending_travel` + the room command-panel rebuild), so GO TOWARDS THIS
  PLACE actually diverts the flight.
- [x] **Arrival landing / approach video** — DONE 2026-07-17
  (`travel_arrival_landing_sequence`, seg000:488a, gated on `data_04732`
  bit 0 at the scene reload's loc_02dfb; verify with
  `cargo test -p dune --bin dune -- --ignored travel_arrival_approach`,
  writes `crates/dune/travel_arrival_approach.png`, plus the end-to-end
  `travel_flight`): branches on `6 + calc_SAL_index(current_location)`
  (the seeded ax makes the SAL result directly the approach video id) —
  the sietch/palace types (< 8) hide the minimap (`travel_minimap_state =
  0x80`), pump flight frames forcing sand terrain until the clip loops
  back to MNT1 and its per-loop frame count `hnm_counter_2` reaches
  0x3c (SIET) / 0x16, then arm `hnm_switch_active_video` (seg000:ce4b:
  active id = the approach clip, `hnm_counter_4` = the handoff frame) so
  the loop point (seg000:cb00 — now modelled in `hnm_step_frame`:
  reaching `hnm_counter_4` frames counts as the loop point) redirects
  into SIET/PALACE.HNM mid-loop and plays it out; SAL 9 plays FORT.HNM
  in the game area (loc_0c8fb = `play_hnm_to_completion`, bp =
  gfx_copy_whole_framebuf_to_screen); the landing-pad types (8/10)
  re-render the pad scene and run the reverse orni landing (frames
  0x1f→0, SN7.VOC, `orni_anim_loop(-1)`). The `hnm_counter_2/4` pair is
  modelled in hnm/mod.rs (counted per decoded frame — the same stream
  position DOS's prefetcher counts ahead; reset at every loop rewind,
  seg000:cb70/ce07). The approach clips carry empty header palettes, so
  the port's redirect-by-reopen stays palette-identical to DOS's
  jump-into-cached-body.
- [x] **Flight-video loop-seam frame skip** — DONE 2026-07-17: the flag
  bit-2 companion resources (video_id + 0x61, resources 0x63..0x68) are the
  `.LOP` files (MNT1.LOP..PALACE.LOP): four size-prefixed full-frame video
  chunks after a small header, which DOS splices into the stream as four
  records at EVERY flight-clip loop point (seg000:cbb8..cc04, consumed by
  the 'mm' handler at loc_0cd37) — the bridge frames played across the
  loop seam before the (possibly redirected-to) body resumes. The port
  queues them at the loop point (`hnm_lop_queue_bridge`) and
  `hnm_step_frame` decodes one per pass through the shared
  `hnm_decode_record`; they count in `hnm_frame_counter`/`hnm_counter_2`
  like stream records (loc_0cc0c/loc_0cc4e), so the arrival handoff
  arithmetic matches DOS exactly. Remaining jitter suspect (minor): the
  PIT pacing carry across the rewind, which the port's
  `hnm_last_frame_tick` tick pacing does not reproduce.
- [x] **`cmd_arg_list` waypoints** — DONE 2026-07-16: it is the travel-trail
  ring, now named `travel_trail_ring` (seg000:e40c, 276 (longitude, latitude)
  pairs to loc_0e85c, empty sentinel 0x800, cursor seg001:149a
  `travel_trail_cursor`), reset by `travel_reset_trail` (seg000:49ea, was
  loc_049ea) and drawn as the minimap trail dots (ICONES 0x2f).
- [x] **Map-mode verbs** — DONE 2026-07-17, wired in
  `dispatch_command_handler` (verify with
  `cargo test -p dune --bin dune -- --ignored map_mode_verbs`):
  - SKIP TO DESTINATION (`menu_callback_choice_skip_to_destination`,
    seg000:4ffb) — fast-forwards up to 0xc8 `travel_advance_step`s, checking
    arrival (map offset == the destination's `map_offset`) and running the
    per-step `travel_route_hostile_zone_check` (seg000:4182, was loc_04182 —
    the Atreides-destination / terrain-0x30 gates, the `data_04726`
    accumulator, the verb greying and `pending_room_action` 4); both the
    arrival and step-exhaustion exits land through
    `travel_finish_at_destination` (seg000:4fc3, was loc_04fc3 — now split
    out of `travel_arrive` and shared with the pump's arrival).
  - CHANGE DESTINATION (`menu_callback_choice_change_destination`,
    seg000:497a) — reopens `map_screen_open` with the Cancel menu,
    `travel_minimap_state` 1 so the cleanup re-enters the flight view.
  - BACK TO STARTING POINT (`menu_callback_choice_back_to_starting_point`,
    seg000:50a5) — restarts the flight clip and aims home at
    `last_location_ptr` via `travel_aim_at_location` (seg000:4965, was
    loc_04965) / `travel_commit_destination` (seg000:496a, was loc_0496a —
    both split out of `arm_pending_travel`).
  - TOWARDS NEAREST PLACE (`menu_callback_choice_towards_nearest_place`,
    seg000:50c4) — `iterate_over_locations_and_coordinates` (seg000:5344,
    the max(|Δlng|>>8, |Δlat|) byte-compare metric) into
    `arm_pending_travel`.
  Remaining stub inside the chain: the hostile-zone warning itself fires
  from `finish_room_screen_setup` (seg000:35ad, the loc_02e52 settle), which
  is still a no-op port stub. (The seg000:5116 tail into
  `map_confirm_travel_and_close` belongs to the map-main-menu
  `move_to_location` orni/worm verbs, seg000:50db/50ea — sibling entry
  points, not these.)

## Sibling entry points sharing this code

- [x] **SEE DUNE MAP** (0x186b → `ui_show_globe_map_view`, seg000:5a1a) —
  DONE 2026-07-24 (troop_map_screen.rs): the open/close toggle, the transition
  callback (seg000:5a56: full-map rect, border, `data_046eb = 0x80`, map
  frieze sides, alt nav panel), `ui_main_view_map_interface` (seg000:5a9a:
  the `MapRenderer` full-planet draw wired into `map_draw_zoomed_globe`'s
  0x80 path, vegetation marks seg000:633b, location markers, the player
  sprite seg000:6314, the fb2 snapshot + `troop_icons_update_dirty_rect`
  present), the map main menu (seg001:20f2 + the seg000:878c grey-bit
  config), the rallied-troops popup (seg000:5bb0/5beb), the nav-arrow
  scroll and the mouse-handler record (seg001:1a9e). ALSO DONE 2026-07-24
  (troop_icons.rs): the troop icon renderer — spawn/remove/anim
  (seg000:c58d..c661, `TaskId::TroopIconAnim` = troop_icon_anim_task 6b34),
  the dirty-rect repaint with the pluggable draw order (seg000:c6ad +
  seg001:2786, fifo c827 / by-depth c835), the map spawn pass
  (map_spawn_troop_icons 6715: marker troop chains + moving troops) with the
  position/script selection (troop_icon_screen_pos 686e,
  troop_icon_pick_script 6770/6827) and the seg001:1672..1935 icon-script
  data block embedded verbatim. AND 2026-07-24, the troop interactions:
  the icon click hit-test (troop_icon_hit_test 6946), troop selection +
  the rotating highlight ring (map_click_troop_icon 872c /
  map_select_troop 8685 / map_focus_troop_icon 697c, the 18fd ring
  script), and the RMB troop info panel (map_open_troop_info_popup 78bc /
  map_place_info_panel 5f25 / map_draw_troop_info_panel_content 78e9 with
  the CONDIT-staged interpolated strings + the equipment row 7e3d, close
  79de, toggle wired in the rmb handler). Still stubbed: the contact verb
  menu + troop dialogue (troop_0780a / troop_07c02 and the 7b58 contact
  strip), the hover label (loc_05692), the occupation panel + popup
  dragging, the comm-glow pointer arrow (loc_0c0e8 / 7b0f / 7b2b), SEE SPICE
  DENSITY (the `data_046eb` bit-0x40 overlay, seg000:53f1/5406) and the
  map-main-menu verbs beyond EXIT MAPS.
- [x] **Location troop popup** (loc_05fb0) — DONE 2026-07-24
  (troop_map_screen.rs): the LMB click near a marker opens the location info
  panel (map_draw_location_popup 0600e: type + name, class code
  location_popup_class 6252, battle gate location_has_battle 627e via troop
  accumulation, the equipment column row with vertical stacking
  draw_equipment_column 61d3, the battle gauge location_battle_gauge 60f8),
  and folds in the GO THERE command menu (menu_multiple_move_to_location_
  flying_an_orni / riding_a_worm, ScreenElement::MoveToLocationMenu, cleanup
  map_close_location_popup 5f91). The GO THERE ORNI verb (50db) wires to the
  ported travel confirm; the WORM verb (50ea) and the departure transition
  are stubbed. The water/spice extra (loc_0605c) is stubbed.
- [ ] The globe-mode branches of `map_screen_draw_base` (seg000:43a9..43c9:
  fb2 snapshot, `loc_05b69` border, `data_014a4` title strip) and
  `map_screen_restore_room_view` (seg000:43ea..43f9: `data_014ac` rect from
  fb2, gated on `hnm_counter_2`) — the CALL A WORM windowed view without the
  cockpit.
- [ ] **CALL A WORM** (0x42d1 → seg000:4285) — VER.HSQ setup + the
  `data_00167`/`data_0aa6e` sub-resource plumbing, then the shared
  `map_screen_open_with_cancel_menu`; greyed until `game_phase >= 0x4f`.
- [ ] **Map-main-menu ornithopter entry** (seg000:42d9,
  `menu_callback_choice_map_main_take_an_ornithopter`) — commit_room_move
  (dl=1) + `ui_toggle_room_view`, falling into the ported notransition entry.
  Reached from the map main menu (`data_02100`), not the room verb.
- [x] **Room ornithopter hover + click** — DONE 2026-07-15: person_hit_test
  carries the orni-hotspot tail (seg000:92ab, pseudo-person 0x2f; hotspot
  recorded by the orni pass at seg000:3a5a and cleared per scene draw at
  seg000:37b8), the hover highlight resolves 0x78 + 0x2f = text 0xa7 with no
  special case, and `callback_main_ui_element_21_22`'s seg000:922a branch
  opens the map screen on click.
