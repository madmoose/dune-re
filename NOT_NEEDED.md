# DOS routines the port does not need

Routines in DNCDPRG.EXE that dune-re replaces wholesale rather than ports.
Each entry names the routine's entry address; `coverage.py` reads this file
and reports them as "not needed" instead of "unported". A routine that a
specific piece of Rust supersedes is tagged `[not needed]` next to that code
instead (see CLAUDE.md); this file is for machinery with no counterpart.

## Input devices

The port reads the mouse and keyboard from the window; there is no joystick.

- `seg000:ea32 initialize_joystick` — read the game port and calibrate the joystick centre/range into _word_22E5B_joystick_param.
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
- `seg000:f13f allocator_attempt_to_free_space` — pick the least-recently-used open resource (largest game_time delta in _word_2CF3C_open_res_info) and evict it to make room.
