# Map screen — remaining work

Status as of 2026-07-15. The TAKE AN ORNITHOPTER verb (seg000:42e9) is ported
and wired end-to-end in `crates/dune/src/map_screen.rs`: the screen opens with
the ORNYPAN cockpit, the windowed one-cell-per-pixel map (curved globe edges),
the alternate nav panel, the Cancel menu, and the "select destination"
narration clip; Cancel restores the room. Verify with
`cargo test -p dune --bin dune -- --ignored ornithopter`
(writes `crates/dune/ornithopter_map.png`).

Everything below is stubbed (each stub carries its `= seg000:xxxx` link) or
not yet started.

## Map screen content

- [ ] **Location markers** — `loc_05dce` builds the visible-location list into
  `data_0a5c0` (entries `[location_ptr, cell, ...]`, 6 bytes each) using the
  visibility/position helpers `location_062c9` / `loc_062d6`, and draws the
  ICONES markers + name labels over the map window, clipped to
  `data_046e3_rect` (`loc_05b93` — the port passes `map_view_rect` as an
  explicit clip instead). `map_screen_cleanup` clears the list head. Also
  needed by SEE DUNE MAP.
- [ ] **Marker blink frame task** — `map_arm_location_marker_task`
  (seg000:445d, currently a no-op stub) resolves the location at the current
  map position, computes the marker rect (`data_04749`) from ICONES
  sub-resource 0x4c, and (re-)adds `frame_task_callback_044ab` at interval
  0x12c. Needs a new `TaskId` variant; `map_screen_cleanup` must then remove
  it (seg000:4429).
- [x] **"SELECT DESTINATION ON MAP" caption** — DONE 2026-07-15: the
  typewriter (`map_caption_frame_task`, seg000:46b5 — one glyph per firing,
  spaces free, high-bit terminator) is ported as `tick_map_caption` +
  `TaskId::MapCaption`; arm/disarm (seg000:4658/469b) fill the
  `map_caption_*` fields (= `data_0473f..data_04747`). Remaining sub-item:
  the cursor save/restore bracket around each glyph
  (`restore_mouse_if_rect_intersects` on the `data_014a4` strip /
  `draw_mouse_cursor_if_needed`) once the live cursor can sit over the
  caption.
- [ ] **Tablat cell caching** — `map_copy_window_row` (seg000:b7d2) caches each
  row's longitude cell in the tablat entry's +6 scratch word; the marker
  positioning reads it back. Not modelled in `Tablat` yet (the port recomputes
  instead) — decide when porting the markers.

## Interaction

- [ ] **Live mouse handlers** (`mouse_handlers_01ac8`, stubs in
  `MAP_MOUSE_HANDLERS`):
  - idle/drag hover tracker `loc_04586` — location-marker hover and the
    edge-scroll travel-arrow direction cache in `data_046fc`, plus the cursor
    shape switch (the 4 travel arrows, seg001:2650/2694/260c/26d8).
  - LMB destination click `loc_0450e` — select the clicked location/cell as
    the travel destination (arms `data_04728`).
- [ ] **Map scrolling** — the alternate nav panel buttons (NAV_PANEL_ALT
  func_ptrs seg000:5b05 / 8829 / 8824 / 882e / 881f) move
  `zoomed_globe_longitude/latitude` and re-run the view redraw. Requires the
  ui_element click dispatch to reach them.
- [ ] **`current_main_view_drawing_function` hookup** (`_word_23B9D`) — the
  game loop calls the installed redraw (seg000:4346 installs `map_view_redraw`
  = seg000:4377); the port has no function-pointer main-view redraw yet.
  `map_view_redraw`'s screen push is also simplified: it presents the whole
  game area instead of `update_screen_at_sprite_rect_updating_head`
  (seg000:4399).
- [ ] **`set_some_mouse_rect`** (seg000:4331) — the map-window mouse hotspot
  installed on open; skipped until the hotspot machinery is ported.

## Travel

- [ ] **Travel departure** — `map_screen_cleanup` enters `loc_049d4` (chain
  seg000:4988 → 49e3 → 4a5a) when `data_04728 > 0` (currently a println stub):
  the HNM flight sequence, selected by `travel_vehicle_mode` refined through
  `loc_04ec6` into `hnm_active_video_id` (day/night variants 2..5), the travel
  pump `loc_04f0c` (`data_04727`), and arrival (seg000:4fcb).
- [ ] **`cmd_arg_list` waypoints** — the cs-resident travel waypoint array
  (seg000:e40c, `[CmdArg; 23]`, words reset to 0x800 by `loc_049ea`); the
  port's `map_reset_travel_state` only models the `data_04728 = 0` half.
- [ ] **`data_011c5`** — pending-travel flag read by `map_screen_cleanup`
  (seg000:443b) to keep `game_screen_mode_flags` across the departure; the
  port resets unconditionally until travel exists.
- [ ] **Map-mode verbs** — SKIP TO DESTINATION (0x4ffb), CHANGE DESTINATION
  (0x497a), BACK TO STARTING POINT (0x50a5), TOWARDS NEAREST PLACE (0x50c4)
  in `dispatch_command_handler`.

## Sibling entry points sharing this code

- [ ] **SEE DUNE MAP** (0x186b → `ui_show_globe_map_view`, seg000:5a1a — stub
  in game_ui.rs) — the full-globe view: `data_046eb` bit 0x80 selects
  `vga_draw_map_zoomed` in `map_draw_zoomed_globe` (println stub; wire the
  standalone `MapRenderer`, which already ports that segvga routine), plus the
  globe-mode branches of `map_screen_draw_base` (seg000:43a9..43c9: fb2
  snapshot, `loc_05b69` border, `data_014a4` title strip) and
  `map_screen_restore_room_view` (seg000:43ea..43f9: `data_014ac` rect from
  fb2, gated on `hnm_counter_2`).
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
