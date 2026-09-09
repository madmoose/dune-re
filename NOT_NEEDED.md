# DOS routines the port does not need

Routines in DNCDPRG.EXE that dune-re replaces wholesale rather than ports.
Each entry names the routine's entry address; `coverage.py` reads this file
and reports them as "not needed" instead of "unported". A routine that a
specific piece of Rust supersedes is tagged `[not needed]` next to that code
instead (see CLAUDE.md); this file is for machinery with no counterpart.

## Input devices

The port reads the mouse and keyboard from the window; there is no joystick.

- `seg000:ea32 initialize_joystick` — read the game port and calibrate the joystick centre/range into _word_22E5B_joystick_param.
- `seg000:e9f4 mouse_func_uncalled` — a mouse-driver event handler that nothing installs (dead code).
- `seg000:db44 shl_cx_and_dx` — the mouse-range scaling helper of define_mouse_range.
- `seg000:db14 define_mouse_range` — store the mouse clip region and set the INT 33h range; the window bounds the pointer in the port.
- `seg000:dd10 joystick_poll_into_mouse` — the cmd-arg JOY path of poll_pointer_input: fold the game port's stick into the mouse position and buttons.
- `seg000:dce0 read_game_port` — time the game port's axis pulses (port 201h) into bx/dx; called by the joystick calibration and the mouse-handler joystick path (seg000:dd10 / dd91).

## HNM streaming

DOS plays HNM clips from a streaming read buffer refilled by `hnm_prefetch`,
with an optional interrupt-driven decode path (resource flag bit 7). The port
reads each clip whole into memory and decodes from the frame task
(`crates/dune/src/hnm/mod.rs`), so the file I/O, buffer bookkeeping and the
timer path have no counterpart.

- `seg000:cd8f hnm_read_header_size` — read one size word from the file.
- `seg000:cdbf hnm_read_bytes` — int 21h seek and read into the stream buffer, retrying short reads.
- `seg000:cdf7 hnm_advance_read_ptr` — advance the buffer write cursor and byte count.
- `seg000:cc0c hnm_note_read_block` — post-read buffer wrap and block-size bookkeeping (the frame-record count it bumps is ported in hnm_advance_to_next_frame).
- `seg000:cc2b hnm_buffer_has_room` — space check before appending a record to the stream buffer.
- `seg000:ca8f hnm_prefetch_guarded` — set hnm_flag_msb around the prefetch to fence the timer decode.
- `seg000:ca9a hnm_clear_flag_msb` — clear that fence.
- `seg000:cec9 hnm_render_timer_callback` — the PIT-handler decode of the next buffered frame while the fence is up.
- `seg000:ce6c hnm_initialize_memory_handler` — startup: strip the streaming and preload flags on low-memory configurations, then preload clips 2..8.
- `seg000:ceb0 hnm_preload_clip` — open one clip at startup and keep its first-frame rect in the resource entry.

## DOS file system

The port reads resources from DUNE.DAT by name and save games through
`std::fs`; the DOS-side filename buffer, drive selection and heap staging have
no counterpart.

- `seg000:a93f read_audio_file` — the INT 21h seek + read of the next 2000h-byte chunk of the streaming voice file; the port hands the whole VOC to the player.
- `seg000:e826 dat_toc_fill_lookup_table` — fill the resource lookup table from DUNE.DAT's table of contents at startup; the port resolves entries by name.
- `seg000:e75b res_store_in_lookup_table` — store one entry in the resource lookup table.
- `seg000:f0a0 open_resource_force_hsq` — load DIALOGUE with unpacking disabled into the GLOBDATA scratch area, then unpack from there into the real buffer (a heap-layout trick; the port unpacks straight into a Vec).
- `seg000:f2fc strcpy_to_filename_buf` — copy a name into the DOS filename buffer before int 21h.
- `seg000:f29b set_default_drive` — int 21h/0e; no callers.
- `seg000:f2e7 seek_and_read_dune_dat` — seek-then-read entry; no callers.

## VGA hardware

DNVGA.BIN talks to the VGA card and the PIT directly; the port renders into
its own framebuffer and paces frames itself.

- `segvga:0967 vga_set_mode_13h` — BIOS mode set / query.
- `segvga:09d9 vga_get_framebuffer_info` — returns the A000h segment.
- `segvga:09b8 vsync_wait` — poll the CRT status port for a vsync edge.
- `segvga:261d fade_vsync_wait` — vsync pacing between fade chunks.
- `segvga:253d vsync_wait_5_ticks` — vsync edge or 5 PIT ticks between synchronous effect frames.
- `segvga:0b68 dac_write` — write palette entries to the DAC ports 3c8h/3c9h.
- `segvga:0a21 palette_byte_range_to_entry_range` — DAC bookkeeping for dac_write.
- `segvga:0975 vga_set_grayscale_mode` — the grayscale DAC patch and CRT port setup; the port has no grayscale mode.
- `segvga:2588 swap_transition_buffers` — swap the two transition buffer segments; the port addresses its buffers directly.
- `segvga:0aa4 vga_fade_save_and_black` — no callers (dead code).
- `segvga:2e2c mosaic_pass_4x4_flush` — no callers (dead code).

## Interrupts and startup checks

- `seg000:f05c reset_keyboard` — the keyboard reset watchdog: once armed (Ctrl+Alt+Del) it pulses the keyboard controller, acknowledges the PIC and exits to DOS through exit_to_dos.
- `seg000:efe1 clear_and_retry` — the keyboard ISR's retry after an 0ffh from port 60h: clear the key array and return.
- `seg000:e913 install_interrupt_handlers` — hook the PIT, keyboard and mouse interrupt vectors from _word_21DC3_interrupt_table.
- `seg000:e85c initialize_pit_timer` — hook INT 8, wait for the first tick and derive the timer calibration byte data_0efd9; the port's clock is the frame sink's.
- `seg000:e57b load_driver_ax_with_vtable_at_si` — load DNVGA.BIN or DN386.BIN and bind its vtable; the gfx module is that driver compiled in.
- `seg000:efba pit_timer_callback_midi` — the PIT ISR's MIDI tick (MIDI_Tick into the status/measure/ticks words); the port's MIDI driver ticks on its own clock.
- `seg000:a87e audio_test_frequency` — play FREQ.HSQ at startup to time the PCM driver.

## Dead code

- `seg000:08e5 unused_midi_fade_out` — fade the music out over 200h ticks and return with CF clear, shaped like an intro script step; nothing references it.
- `seg000:676e loc_0676e` — a `stc; ret` after map_spawn_troop_icon's own return; nothing jumps to it.
- `seg000:bf55 results_stat_string_replace_5_unused` — unreferenced: the 5-digit results replace without the forced trailing zero.
- `seg000:d677 ui_element_release_redraw_unused` — unreferenced: redraw the HUD element ui_element_press_feedback recorded.
- `seg000:5381 find_nearest_location_unused` — an unreferenced variant of iterate_over_locations_and_coordinates without the hidden-location test.
- `seg000:53b6 find_nearest_fortress_unused` — an unreferenced variant of iterate_over_locations_and_coordinates restricted to fortresses.
- `seg000:6a6b loc_06a6b` — end the troop contact and clear the occupation bits; nothing references it.

## Memory management

Every resource the port opens is read whole into a `Vec<u8>` from DUNE.DAT
(`DatFile` in `crates/dune/src/dat_file.rs`), so DOS's fixed resource cache,
its page-slot allocator and the XMS/EMS drivers behind it have no equivalent.

- `seg000:ea7b init_extended_memory_allocator` — probe HIMEM/EMS and install the memory_func vtable.
- `seg000:eab7 memory_func_qq` — claim a page slot in the resource cache for a freshly loaded resource (stamps the page table at es:[si+172h] and bumps the XMS/EMS page limit).
- `seg000:eb74 page_cache_evict_oldest` — free the slot chain with the oldest use stamp.
- `seg000:ebaa page_cache_free_chain` — clear a chain of page slots.
- `seg000:ebb8 page_cache_chain_size` — byte size of a paged allocation (no callers).
- `seg000:ebe3 alloc_extended_memory` — allocate from the XMS/EMS pool through the memory_func vtable.
- `seg000:ec46 call_memory_func_2` — vtable trampoline.
- `seg000:ec59 call_memory_func_1` — vtable trampoline.
- `seg000:ec9c xms_memory_func_0ec9c` — XMS page move-in entry.
- `seg000:ecec xms_memory_func_0ecec` — XMS page move-out entry.
- `seg000:ed40 get_ems_emm_handle` — EMS handle accessor.
- `seg000:ed45 call_ems_func` — the int 67h call.
- `seg000:ed4c init_ems_allocator` — EMS page frame / page allocation setup.
- `seg000:edb9 map_ems_for_midi_audio` — map the EMS pages the MIDI driver lives in.
- `seg000:ee02 ems_memory_func_2` — EMS page map entry.
- `seg000:ee46 ems_memory_func_1` — EMS page unmap entry.
- `seg000:eea0 initialize_himem_sys` — detect HIMEM.SYS and allocate the XMS block.
- `seg000:eee9 xms_memory_func_2` — XMS move entry.
- `seg000:ef22 call_xms_driver_func` — the XMS driver call.
- `seg000:ef2b call_xms_func_on_block` — XMS call on the block handle.
- `seg000:ef32 xms_move_memory` — XMS move-memory (0bh) call.

The conventional-memory bump allocator that feeds the resource cache goes
with it:

- `seg000:f0f6 bump_alloc_get_addr_in_di_and_bump` — store the next free far pointer at [di], then bump.
- `seg000:f0ff bump_allocate_bump_cx_bytes` — advance allocator_next_free_seg by cx bytes (rounded to paragraphs), exiting through out_of_memory_error past allocator_last_free_seg.
- `seg000:f11c alloc_check_cx_pages_available` — retry allocator_attempt_to_free_space until cx paragraphs fit.
- `seg000:f131 out_of_memory_error` — print the error and exit_to_dos.
- `seg000:e8d5 uninitialize_memory_driver` — free the XMS block / EMS handle at exit.
- `seg000:f13f allocator_attempt_to_free_space` — pick the least-recently-used open resource (largest game_time delta in _word_2CF3C_open_res_info) and evict it to make room.
- `seg000:e8b8 pit_timer_callback` — the boot PIT probe ISR: latch the 8253 counter into pit_timer_value and bump pit_timer_counter; the port's clock is the frame sink's.
- `seg000:ae3f herad_alloc_buffer` — bump-allocate the 9c40h-byte HERAD music buffer once the driver is present; the port's MIDI driver owns its buffers.
- `seg000:a9a1 close_pcm_voice_file_handle` — close the streaming voice file's DOS handle (INT 21h/3Eh) unless it is the shared DUNE.DAT handle; the port reads voice clips whole from the DAT.
- `seg000:6670 location_sum_harvest_total_of_same_occupation` — unreachable: no caller in DNCDPRG sums harvest_total over the location's troops sharing a troop's occupation nibble.
- `seg000:667d callback_troop_sum_harvest_total_if_same_occupation` — the per-troop step of that unreachable sum.
- `seg000:8454 troop_settle_into_location_unreachable` — unreachable: no caller settles a troop into its location this way (clear occupation bits 0-1, link, clear bit 6, register its equipment).
- `seg000:a1ca clear_dialogue_interrupt_gate_unreferenced` — an unreferenced twin of callback_event_dialogue_line_02_stay_here.
- `seg000:ac35 pcm_break_loop_unreferenced` — an unreferenced call of the PCM driver's break-loop entry.
- `seg000:ae54 pcm_alloc_voc_buffer` — bump-allocate the 4e20h-byte voice clip buffer at start-up when a PCM card is present; the port reads clips into Vecs.
- `seg000:c422 copy_fb2_to_active_framebuffer_unreferenced` — an unreferenced fb2 -> active framebuffer copy through vga_copy_screen_1.
- `seg000:c64c troop_icon_spawn_and_dirty_unreferenced` — unreferenced: spawn a troop icon record and repaint its rect.
- `seg000:e270 save_regs` — push bx/cx/dx/si/di/bp under the caller's return address; a register-preservation helper.
- `seg000:e283 restore_regs` — the matching pop; a register-preservation helper.
- `seg000:e2ca font_draw_two_digits_unreferenced` — an unreferenced two-digit drawer (aam, two glyph calls) after font_draw_number_byte.
- `seg000:50a2 menu_callback_choice_back_to_starting_point_unreferenced` — an unreferenced entry that pops the menu before menu_callback_choice_back_to_starting_point.
- `seg000:e56b parse_cmd_is_end_of_arg` — the PSP command-tail tokenizer's end-of-argument test (parse_command_line).
- `seg000:e851 alloc_check_ram_available` — allocator_next_free_seg + 2f13h paragraphs against allocator_last_free_seg: the start-up RAM check.
- `seg000:e8a8 set_pit_timer_frequency` — program 8253 channel 0 (mode 3) with the divisor in ax; the port's clock is the frame sink's.
- `seg000:e94a int_skip_2_bytes_iret` — an interrupt stub that returns past a 2-byte instruction after the int.
