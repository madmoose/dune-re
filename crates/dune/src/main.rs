#![feature(cursor_split)]
#![allow(clippy::identity_op)]
#![allow(clippy::collapsible_if)]
#![allow(dead_code)]

mod attack;
mod blit;
mod color;
mod condit;
mod container;
mod dat_file;
mod dialogue;
mod fixed_point;
mod font;
mod frame_slot;
mod framebuffer;
mod game_state;
mod game_ui;
mod gfx;
mod herad;
mod hnm;
mod hsq;
mod image;
mod input;
mod intro;
mod intro2;
mod language;
mod lipsync;
mod locations;
mod midi;
mod mouse;
mod music;
mod palace_plan;
mod palette;
mod pcm_player;
mod point;
mod rect;
mod room_game_screen;
mod room_renderer;
mod room_scene;
mod settings_ui;
mod sprite;
mod sprite_bank;
mod sprite_blitter;
mod sprite_sheet;
mod tablat;
mod talking_head;
mod troops;
mod ui_hud_head;
mod voc;
mod zoom;

use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

use clap::{Parser, ValueEnum};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CustomCursor, Window, WindowId},
};

use crate::{
    color::Color,
    dat_file::DatFile,
    font::{Font, FontState, TextSize},
    frame_slot::FrameSlot,
    framebuffer::FrameBuffer,
    game_state::{FbId, GameState, MIDI_SAMPLE_RATE, TaskId},
    image::Image,
    input::{InputState, SharedInput, keycode_to_scancode},
    lipsync::Lipsync,
    locations::{Equipment, Location},
    mouse::{CursorMode, CursorShapeId, SharedCursor, cursor_shape},
    palette::Palette,
    point::Point,
    rect::Rect,
    room_renderer::{DrawOptions, RoomRenderer, RoomSheet, sal_position_markers},
    sprite_blitter::{draw_sprite, draw_sprite_from_sheet, sprite_blitter},
    sprite_sheet::SpriteSheet,
    talking_head::TalkingHead,
};

/// Debug helper: print `buf` as a 16-bytes-per-row hex + ASCII dump.
fn hexdump(buf: &[u8]) {
    for (row, chunk) in buf.chunks(16).enumerate() {
        print!("{:08x} ", row * 16);
        for i in 0..16 {
            match chunk.get(i) {
                Some(b) => print!(" {b:02x}"),
                None => print!("   "),
            }
        }
        print!("  ");
        for &b in chunk {
            print!("{}", if b.is_ascii_graphic() { b as char } else { '.' });
        }
        println!();
    }
}

const SRC_W: u32 = 320;
const SRC_H: u32 = 200;
const LANCZOS_A: f32 = 3.0;

// Smallest window dimension we'll configure/present a surface for. Below this the
// swapchain drawable is effectively degenerate and the backend (Metal) returns
// uninitialized magenta frames; we treat anything smaller as non-presentable.
const MIN_PRESENT_DIM: u32 = 8;

// Uniform block handed to dune.wgsl. Layout must match the WGSL `Uniforms`
// struct (five 16-byte vec4 slots); #[repr(C)] + Pod so it uploads byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    dims: [u32; 4],         // (src_w, src_h, dst_w, dst_h)
    pre: [u32; 4],          // (nx, ny, supp_x, supp_y)
    kernel: [f32; 4],       // (a, _, _, _)
    cursor: [i32; 4],       // (x_top_left, y_top_left, shape_idx, visible)
    cursor_color: [f32; 4], // (r, g, b, _) for palette[0x0f] at present time
}

// One cursor shape — what the GPU overlay needs to know about it.
#[derive(Clone, Copy)]
struct CursorOverlayDraw {
    x: i32,
    y: i32,
    shape_idx: u32,
    visible: bool,
    rgb: [f32; 3],
}

impl CursorOverlayDraw {
    fn hidden() -> Self {
        Self {
            x: 0,
            y: 0,
            shape_idx: 0,
            visible: false,
            rgb: [0.0; 3],
        }
    }
}

const CURSOR_SHAPE_COUNT: u32 = 6;
const CURSOR_SHAPE_SIZE: u32 = 16;

// How many Lanczos taps the downscale from `pre` to `dst` needs along one axis.
// Computed on the host and passed in the uniform block so the shader's loop
// bounds cover the full kernel footprint.
fn lanczos_support(pre: u32, dst: u32) -> u32 {
    let scale = dst as f32 / pre as f32;
    let s_in = scale.min(1.0);
    let radius = LANCZOS_A / s_in;
    (2.0 * radius).ceil() as u32 + 2
}

// All wgpu state for presenting frames. Created once the window exists; the
// 320x200 source lives in a single texture re-uploaded each frame, and the
// Lanczos scale happens entirely in the fragment shader.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    uniform_buffer: wgpu::Buffer,
    // Reusable staging buffer for the indexed->RGBA expansion (320*200*4).
    rgba: Vec<u8>,
    // Whether `texture` holds a frame yet; gates rendering before the first frame.
    have_frame: bool,
    // Set when we skip a render because the window had a zero dimension (e.g.
    // dragged to ~0 height). `config` is left untouched while degenerate, so its
    // dimensions can still match the restored size — which would make the
    // mismatch check below skip the reconfigure the invalidated surface needs.
    // This flag forces one reconfigure on the next non-degenerate render.
    needs_reconfigure: bool,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Gpu {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).unwrap();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("no suitable GPU adapter found");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("dune device"),
            ..Default::default()
        }))
        .expect("failed to request device");

        // Present without sRGB conversion so the raw 8-bit DOS palette values
        // reach the screen unchanged; the resample in the shader runs in this
        // same non-gamma space.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported by adapter");
        config.format = format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        // Cap the swapchain to a single in-flight frame (default is 2). In
        // Overlay mode the cursor is composited at present time from the
        // freshest pointer position, so a deeper queue means the displayed
        // cursor trails the OS pointer by up to that many vsync intervals.
        // One frame of latency keeps the overlay glued to the real pointer.
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width: SRC_W,
                height: SRC_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Cursor mask atlas — every CursorShapeId rendered as one 16×16 tile,
        // stacked vertically. Each texel encodes AND in bit 0 and OR in bit 1
        // (matching the per-pixel test in `vga_draw_cursor`).
        let cursor_masks = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cursor masks"),
            size: wgpu::Extent3d {
                width: CURSOR_SHAPE_SIZE,
                height: CURSOR_SHAPE_SIZE * CURSOR_SHAPE_COUNT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let cursor_masks_view = cursor_masks.create_view(&wgpu::TextureViewDescriptor::default());
        upload_cursor_masks(&queue, &cursor_masks);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // textureLoad does its own sampling, so no filtering.
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&cursor_masks_view),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lanczos"),
            source: wgpu::ShaderSource::Wgsl(include_str!("dune.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lanczos pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Gpu {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            texture,
            uniform_buffer,
            rgba: vec![0u8; (SRC_W * SRC_H * 4) as usize],
            have_frame: false,
            needs_reconfigure: false,
        }
    }

    // Expand the indexed 320x200 frame through its palette into RGBA8 and upload
    // it to the source texture. This is the only per-pixel CPU work left; the
    // scale itself runs on the GPU.
    fn upload_frame(&mut self, framebuffer: &FrameBuffer, palette: &Palette) {
        for (i, &idx) in framebuffer.pixels().iter().enumerate() {
            let c = palette.get_rgb888(idx as usize);
            let o = i * 4;
            self.rgba[o] = c.0;
            self.rgba[o + 1] = c.1;
            self.rgba[o + 2] = c.2;
            self.rgba[o + 3] = 255;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SRC_W * 4),
                rows_per_image: Some(SRC_H),
            },
            wgpu::Extent3d {
                width: SRC_W,
                height: SRC_H,
                depth_or_array_layers: 1,
            },
        );
        self.have_frame = true;
    }

    // Draw the current frame, scaled and letterboxed to fit `win_w`x`win_h`.
    // `cursor` is composited in the fragment shader against the source-space
    // pixel grid so it scales with the rest of the image.
    fn render(&mut self, win_w: u32, win_h: u32, cursor: CursorOverlayDraw) {
        // A degenerate surface (window dragged to ~0 height, minimized, etc.) is
        // non-presentable: Metal hands back an uninitialized drawable that shows
        // up as magenta, and presenting/skipping inconsistently across the
        // boundary makes it flicker. Refuse to configure or present below a small
        // minimum and leave the last good configuration alone, but remember the
        // surface was invalidated so the next live render forces a reconfigure
        // even if the restored size matches `config` — otherwise it stays stuck
        // and never recovers.
        if win_w < MIN_PRESENT_DIM || win_h < MIN_PRESENT_DIM {
            self.needs_reconfigure = true;
            return;
        }
        if !self.have_frame {
            return;
        }
        if self.needs_reconfigure || self.config.width != win_w || self.config.height != win_h {
            self.config.width = win_w;
            self.config.height = win_h;
            self.surface.configure(&self.device, &self.config);
            self.needs_reconfigure = false;
        }

        let (scaled_w, scaled_h, offset_x, offset_y) = fit_rect(win_w, win_h);
        if scaled_w == 0 || scaled_h == 0 {
            return;
        }

        // Integer prescale factors and matching Lanczos tap counts for this size.
        let nx = scaled_w.div_ceil(SRC_W).max(1);
        let ny = scaled_h.div_ceil(SRC_H).max(1);
        let uniforms = Uniforms {
            dims: [SRC_W, SRC_H, scaled_w, scaled_h],
            pre: [
                nx,
                ny,
                lanczos_support(SRC_W * nx, scaled_w),
                lanczos_support(SRC_H * ny, scaled_h),
            ],
            kernel: [LANCZOS_A, 0.0, 0.0, 0.0],
            cursor: [
                cursor.x,
                cursor.y,
                cursor.shape_idx as i32,
                cursor.visible as i32,
            ],
            cursor_color: [cursor.rgb[0], cursor.rgb[1], cursor.rgb[2], 1.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // Surface went stale (e.g. mid-resize) — reconfigure and skip; the
            // next redraw paints it.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    // Clear the whole surface so the pill/letterbox bars are black.
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // The viewport confines the fullscreen triangle to the centred fit
            // rect; the shader maps uv 0..1 across it.
            pass.set_viewport(
                offset_x as f32,
                offset_y as f32,
                scaled_w as f32,
                scaled_h as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.queue.present(frame);
    }
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    frame_slot: FrameSlot,
    start_signal: Option<std::sync::mpsc::Sender<()>>,
    // Shared keyboard + mouse state filled here (the "ISR") and polled by the
    // game thread via any_key_pressed.
    input: SharedInput,
    cursor_mode: CursorMode,
    shared_cursor: SharedCursor,
    // True while the OS pointer is inside the scaled game image (not in the
    // letterbox/pillarbox bars and not outside the window). The GPU cursor
    // overlay hides itself when this is false so the dune cursor never
    // appears clamped to the image edge.
    cursor_in_game_area: bool,
    // Running mouse-button bitmask (bit0 left, bit1 right, bit2 middle); winit
    // delivers one press/release at a time, so we accumulate it here before
    // pushing the combined mask into the shared input.
    mouse_buttons: u8,
    // Most recent presented frame (indexed pixels + its palette), retained so the
    // F12 screenshot hotkey can dump it in any phase — intro, credits or in-game.
    last_frame: Option<(FrameBuffer, Palette)>,
    // Counter numbering successive screenshots.
    screenshot_seq: u32,
    // True while the window is fully hidden (minimised / another space / wholly
    // covered). The present loop re-arms `request_redraw` every frame and is
    // paced only by `Queue::present` blocking on vsync; once the surface is
    // off-screen that vsync pacing disappears, so the loop would spin unbounded,
    // pegging the CPU and flooding the GPU drawable queue (overload + drawable
    // timeouts). While occluded we stop presenting and let the event loop idle.
    occluded: bool,
    // `--cursor system`: instead of the GPU overlay, hand the cursor to the OS
    // as a scaled custom bitmap so the compositor moves it with zero present-
    // loop latency. The game still runs in `Overlay` mode (publishes shape,
    // never bakes); these fields hold the built cursor set and the last applied
    // (shape, visible) so redundant `set_cursor` calls are skipped.
    system_cursor: bool,
    system_cursors: Option<SystemCursors>,
    system_cursor_applied: Option<(u32, bool)>,
}

impl App {
    // Port-only debug hotkey: write the last presented frame as a 320x200 PNG and
    // its palette as a 16x16 grid of 8x8 colour blocks (128x128 PNG), into the
    // working directory as dune-screen-NNN.png / dune-palette-NNN.png.
    fn save_screenshot(&mut self) {
        let seq = self.next_screenshot_seq();
        let screen_path = format!("dune-screen-{seq:03}.png");
        let palette_path = format!("dune-screen-{seq:03}-pal.png");

        let Some((framebuffer, palette)) = self.last_frame.as_ref() else {
            eprintln!("screenshot: no frame presented yet");
            return;
        };
        let screen_result = framebuffer.write_png(palette, &screen_path);
        let palette_result = palette.write_png_grid(&palette_path);

        match (screen_result, palette_result) {
            (Ok(()), Ok(())) => {
                self.screenshot_seq += 1;
                eprintln!("screenshot: wrote {screen_path} and {palette_path}");
            }
            (screen_result, palette_result) => {
                if let Err(e) = screen_result {
                    eprintln!("screenshot: failed to write {screen_path}: {e}");
                }
                if let Err(e) = palette_result {
                    eprintln!("screenshot: failed to write {palette_path}: {e}");
                }
            }
        }
    }

    fn compute_cursor_overlay(&self) -> CursorOverlayDraw {
        let Some((_, pal)) = self.last_frame.as_ref() else {
            return CursorOverlayDraw::hidden();
        };
        compute_cursor_overlay_inner(
            self.cursor_mode,
            &self.shared_cursor,
            &self.input,
            pal,
            self.cursor_in_game_area,
        )
    }

    fn next_screenshot_seq(&mut self) -> u32 {
        loop {
            let seq = self.screenshot_seq;
            let screen_path = format!("dune-screen-{seq:03}.png");
            let palette_path = format!("dune-screen-{seq:03}-pal.png");

            if !std::fs::exists(screen_path).unwrap_or(false)
                && !std::fs::exists(palette_path).unwrap_or(false)
            {
                break;
            }

            self.screenshot_seq += 1;
        }

        self.screenshot_seq
    }

    // Drive the OS cursor in `--cursor system` mode: (re)build the scaled cursor
    // set when the window scale changes, then apply the shape and visibility the
    // game thread published via `SharedCursor`. The OS composites it, so it
    // tracks the pointer independently of the present loop.
    fn update_system_cursor(&mut self, event_loop: &ActiveEventLoop) {
        // Cloning the Arc releases the borrow on `self` so we can mutate
        // `self.system_cursors` below.
        let Some(window) = self.window.clone() else {
            return;
        };

        let overlay = self.shared_cursor.snapshot();
        let visible = !overlay.hidden && self.cursor_in_game_area;
        let shape_idx = cursor_shape_index(overlay.shape);

        if !visible {
            if self.system_cursor_applied != Some((shape_idx, false)) {
                window.set_cursor_visible(false);
                self.system_cursor_applied = Some((shape_idx, false));
            }
            return;
        }

        let size = window.inner_size();
        let (sw, sh, _, _) = fit_rect(size.width, size.height);
        if sw == 0 || sh == 0 {
            return;
        }
        // Match the on-screen game-pixel size in logical units: macOS draws the
        // bitmap at point size == pixel count and handles HiDPI itself, so we
        // size in points (physical fit-rect / scale factor) and clamp so 16×N
        // stays within MAX_CURSOR_SIZE (2048).
        let sf = window.scale_factor();
        let scale_x = (((sw as f64 / sf) / SRC_W as f64).round().max(1.0) as u32).min(128);
        let scale_y = (((sh as f64 / sf) / SRC_H as f64).round().max(1.0) as u32).min(128);

        // (Re)build when first shown or when the window scale changes.
        let rebuild = self
            .system_cursors
            .as_ref()
            .is_none_or(|c| c.scale_x != scale_x || c.scale_y != scale_y);
        if rebuild {
            self.system_cursors = Some(build_system_cursors(event_loop, scale_x, scale_y));
            self.system_cursor_applied = None;
        }

        if self.system_cursor_applied == Some((shape_idx, true)) {
            return;
        }
        let cursors = self.system_cursors.as_ref().unwrap();
        window.set_cursor(cursors.cursors[shape_idx as usize].clone());
        window.set_cursor_visible(true);
        self.system_cursor_applied = Some((shape_idx, true));
    }
}

/// Compute what the GPU overlay should draw this present, sampling the
/// freshest pointer position from `SharedInput` and the active shape from
/// `SharedCursor`. In `Baked` mode this returns `hidden()` so the shader
/// path runs but draws nothing — the cursor pixels are already in the
/// framebuffer.
fn compute_cursor_overlay_inner(
    mode: CursorMode,
    shared_cursor: &SharedCursor,
    input: &SharedInput,
    palette: &Palette,
    in_game_area: bool,
) -> CursorOverlayDraw {
    if mode != CursorMode::Overlay || !in_game_area {
        return CursorOverlayDraw::hidden();
    }
    let overlay = shared_cursor.snapshot();
    if overlay.hidden {
        return CursorOverlayDraw::hidden();
    }
    let (mx, my) = {
        let inp = input.lock().unwrap();
        (inp.mouse_x, inp.mouse_y)
    };
    // Match the hotspot subtraction in `vga_draw_cursor` (gfx.rs:804-805).
    let shape = cursor_shape(overlay.shape);
    let top_x = (mx as i32) - shape.hotspot_x as i32;
    let top_y = (my as i32) - shape.hotspot_y as i32;
    let c = palette.get_rgb888(0x0f);
    CursorOverlayDraw {
        x: top_x,
        y: top_y,
        shape_idx: cursor_shape_index(overlay.shape),
        visible: true,
        rgb: [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0],
    }
}

fn cursor_shape_index(id: CursorShapeId) -> u32 {
    match id {
        CursorShapeId::Arrow => 0,
        CursorShapeId::Hand => 1,
        CursorShapeId::Up => 2,
        CursorShapeId::Right => 3,
        CursorShapeId::Down => 4,
        CursorShapeId::Left => 5,
    }
}

/// Build the cursor mask atlas — one 16×16 R8Uint tile per shape, stacked
/// vertically. Each texel: bit 0 = AND, bit 1 = OR; matches the per-pixel
/// test in `vga_draw_cursor` (gfx.rs:832-837).
fn upload_cursor_masks(queue: &wgpu::Queue, texture: &wgpu::Texture) {
    const SHAPES: [CursorShapeId; CURSOR_SHAPE_COUNT as usize] = [
        CursorShapeId::Arrow,
        CursorShapeId::Hand,
        CursorShapeId::Up,
        CursorShapeId::Right,
        CursorShapeId::Down,
        CursorShapeId::Left,
    ];
    let w = CURSOR_SHAPE_SIZE as usize;
    let h = (CURSOR_SHAPE_SIZE * CURSOR_SHAPE_COUNT) as usize;
    let mut data = vec![0u8; w * h];
    for (idx, id) in SHAPES.iter().enumerate() {
        let shape = cursor_shape(*id);
        for row in 0..w {
            let and = shape.and_mask[row];
            let or = shape.or_mask[row];
            for col in 0..w {
                let bit = 0x8000u16 >> col;
                let a = (and & bit != 0) as u8;
                let o = (or & bit != 0) as u8;
                data[(idx * w + row) * w + col] = a | (o << 1);
            }
        }
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(CURSOR_SHAPE_SIZE),
            rows_per_image: Some(CURSOR_SHAPE_SIZE * CURSOR_SHAPE_COUNT),
        },
        wgpu::Extent3d {
            width: CURSOR_SHAPE_SIZE,
            height: CURSOR_SHAPE_SIZE * CURSOR_SHAPE_COUNT,
            depth_or_array_layers: 1,
        },
    );
}

/// One OS cursor per shape, built at a fixed integer upscale. Rebuilt only when
/// the window scale changes; the foreground/background colours are the fixed
/// `CURSOR_FG`/`CURSOR_BG` (never read from the game palette).
struct SystemCursors {
    scale_x: u32,
    scale_y: u32,
    cursors: [CustomCursor; CURSOR_SHAPE_COUNT as usize],
}

/// Expand one 16×16 cursor shape into a nearest-neighbour-upscaled RGBA buffer
/// for an OS cursor. Mirrors the per-pixel AND/OR test in `vga_draw_cursor`
/// (gfx.rs:1074-1076): AND set → transparent, else OR set → foreground
/// (palette[0x0f]), else the black outline (palette[0x00]).
fn cursor_shape_rgba(id: CursorShapeId, scale_x: u32, scale_y: u32) -> Vec<u8> {
    const CURSOR_FG: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
    const CURSOR_BG: [u8; 4] = [0x00, 0x00, 0x00, 0xff];

    let shape = cursor_shape(id);
    let sw = CURSOR_SHAPE_SIZE * scale_x;
    let mut rgba = vec![0u8; (sw * CURSOR_SHAPE_SIZE * scale_y * 4) as usize];
    for row in 0..CURSOR_SHAPE_SIZE as usize {
        let and = shape.and_mask[row];
        let or = shape.or_mask[row];
        for col in 0..CURSOR_SHAPE_SIZE {
            let bit = 0x8000u16 >> col;
            let px = if and & bit != 0 {
                [0, 0, 0, 0]
            } else if or & bit != 0 {
                CURSOR_FG
            } else {
                CURSOR_BG
            };
            // Nearest-neighbour expand this source texel into its scale block.
            for dy in 0..scale_y {
                let y = row as u32 * scale_y + dy;
                for dx in 0..scale_x {
                    let x = col * scale_x + dx;
                    let o = ((y * sw + x) * 4) as usize;
                    rgba[o..o + 4].copy_from_slice(&px);
                }
            }
        }
    }
    rgba
}

/// Build the full set of OS cursors at `(scale_x, scale_y)`. The hotspot is
/// placed at the centre of the scaled hotspot pixel.
fn build_system_cursors(event_loop: &ActiveEventLoop, scale_x: u32, scale_y: u32) -> SystemCursors {
    const SHAPES: [CursorShapeId; CURSOR_SHAPE_COUNT as usize] = [
        CursorShapeId::Arrow,
        CursorShapeId::Hand,
        CursorShapeId::Up,
        CursorShapeId::Right,
        CursorShapeId::Down,
        CursorShapeId::Left,
    ];
    let cursors = SHAPES.map(|id| {
        let shape = cursor_shape(id);
        let rgba = cursor_shape_rgba(id, scale_x, scale_y);

        let w = (CURSOR_SHAPE_SIZE * scale_x) as u16;
        let h = (CURSOR_SHAPE_SIZE * scale_y) as u16;

        // Centre the hotspot within its scaled pixel block (+ scale/2)
        let hx = shape.hotspot_x * scale_x as u16 + scale_x as u16 / 2;
        let hy = shape.hotspot_y * scale_y as u16 + scale_y as u16 / 2;

        let source = CustomCursor::from_rgba(rgba, w, h, hx, hy)
            .expect("16×16 cursor scaled within MAX_CURSOR_SIZE");
        event_loop.create_custom_cursor(source)
    });
    SystemCursors {
        scale_x,
        scale_y,
        cursors,
    }
}

/// Map the 320×200 game framebuffer into a window of `win_w`×`win_h`, returning
/// `(scaled_w, scaled_h, offset_x, offset_y)`. Dune's pixels have a 5:6 aspect
/// (effective 320×240 / 4:3); the image is scaled to fit and centred with
/// pill/letterboxing. Both the redraw blit and the cursor-coordinate mapping
/// use this so they never disagree about where the image sits.
fn fit_rect(win_w: u32, win_h: u32) -> (u32, u32, u32, u32) {
    let eff_w = 320 * 5; // 1600
    let eff_h = 200 * 6; // 1200  =>  eff_w/eff_h == 4/3
    if win_w * eff_h > win_h * eff_w {
        // Window wider than source — fit to height, pillarbox left/right.
        let scaled_h = win_h;
        let scaled_w = win_h * eff_w / eff_h;
        (scaled_w, scaled_h, (win_w - scaled_w) / 2, 0)
    } else {
        // Window taller (or same) — fit to width, letterbox top/bottom.
        let scaled_w = win_w;
        let scaled_h = win_w * eff_h / eff_w;
        (scaled_w, scaled_h, 0, (win_h - scaled_h) / 2)
    }
}

/// True when `(px, py)` lies inside the scaled game image (i.e. not in a
/// letterbox/pillarbox bar). Used to hide the GPU cursor overlay while the
/// OS pointer is in the bars instead of letting it sit clamped at the
/// nearest image edge.
fn point_in_fit_rect(px: f64, py: f64, win_w: u32, win_h: u32) -> bool {
    let (sw, sh, ox, oy) = fit_rect(win_w, win_h);
    if sw == 0 || sh == 0 {
        return false;
    }
    let ox = ox as f64;
    let oy = oy as f64;
    px >= ox && px < ox + sw as f64 && py >= oy && py < oy + sh as f64
}

/// Convert a window-space cursor position into 320×200 game coordinates,
/// inverting [`fit_rect`] and clamping to the framebuffer bounds.
fn window_to_game_coords(px: f64, py: f64, win_w: u32, win_h: u32) -> (u16, u16) {
    let (sw, sh, ox, oy) = fit_rect(win_w, win_h);
    if sw == 0 || sh == 0 {
        return (0, 0);
    }
    let rx = (px as i64 - ox as i64).clamp(0, sw as i64 - 1);
    let ry = (py as i64 - oy as i64).clamp(0, sh as i64 - 1);
    let gx = (rx * 320 / sw as i64).clamp(0, 319) as u16;
    let gy = (ry * 200 / sh as i64).clamp(0, 199) as u16;
    (gx, gy)
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // Already initialized
        }

        let dst_w = SRC_W;
        let dst_h = SRC_H * 6 / 5;
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Dune Reimplemented")
                        .with_inner_size(winit::dpi::LogicalSize::new(3 * dst_w, 3 * dst_h))
                        .with_min_inner_size(winit::dpi::LogicalSize::new(dst_w, dst_h)),
                )
                .unwrap(),
        );

        // The game renders its own mouse cursor sprite, so hide the host one.
        window.set_cursor_visible(false);

        self.gpu = Some(Gpu::new(window.clone()));
        self.window = Some(window);

        // Signal the game thread to start
        if let Some(start_signal) = self.start_signal.take() {
            let _ = start_signal.send(());
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if occluded {
                    event_loop.set_control_flow(ControlFlow::Wait);
                } else {
                    event_loop.set_control_flow(ControlFlow::Poll);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::F12 && event.state == ElementState::Pressed && !event.repeat
                    {
                        self.save_screenshot();
                    }
                    if let Some(scancode) = keycode_to_scancode(code) {
                        let pressed = event.state == ElementState::Pressed;
                        self.input.lock().unwrap().on_key(scancode, pressed);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.cursor_in_game_area =
                        point_in_fit_rect(position.x, position.y, size.width, size.height);
                    let (gx, gy) =
                        window_to_game_coords(position.x, position.y, size.width, size.height);
                    self.input.lock().unwrap().on_mouse_move(gx, gy);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_in_game_area = false;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let bit = match button {
                    MouseButton::Left => 1,
                    MouseButton::Right => 2,
                    MouseButton::Middle => 4,
                    _ => 0,
                };
                if bit != 0 {
                    if state == ElementState::Pressed {
                        self.mouse_buttons |= bit;
                    } else {
                        self.mouse_buttons &= !bit;
                    }
                    self.input
                        .lock()
                        .unwrap()
                        .on_mouse_button(self.mouse_buttons);
                }
            }
            WindowEvent::Resized(size) => {
                let cursor = self.compute_cursor_overlay();
                if let Some(gpu) = &mut self.gpu {
                    gpu.render(size.width, size.height, cursor);
                }
            }
            WindowEvent::RedrawRequested => {
                if self.occluded {
                    return;
                }

                let new_frame = self.frame_slot.take_latest();
                let had_new = new_frame.is_some();
                if let Some((framebuffer, palette)) = new_frame {
                    self.last_frame = Some((framebuffer, palette));
                }

                let gpu_overlay = self.cursor_mode == CursorMode::Overlay && !self.system_cursor;
                let re_render = had_new || gpu_overlay;
                if re_render
                    && let Some((fb, pal)) = self.last_frame.as_ref()
                    && let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window)
                {
                    if had_new {
                        gpu.upload_frame(fb, pal);
                    }
                    let size = window.inner_size();
                    let cursor = if gpu_overlay {
                        compute_cursor_overlay_inner(
                            self.cursor_mode,
                            &self.shared_cursor,
                            &self.input,
                            pal,
                            self.cursor_in_game_area,
                        )
                    } else {
                        CursorOverlayDraw::hidden()
                    };
                    gpu.render(size.width, size.height, cursor);
                }

                if self.system_cursor {
                    self.update_system_cursor(event_loop);
                }

                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}

/// How the mouse cursor is rendered. Each variant selects a (`CursorMode`,
/// `system_cursor`) pair: the game thread either bakes the DOS cursor sprite
/// into the framebuffer (`Baked`) or just publishes its shape (`Overlay`), and
/// the present thread draws it via the GPU shader or hands it to the OS.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Cursor {
    /// DOS-faithful: the game bakes the cursor sprite into the framebuffer.
    Software,
    /// Composite the cursor in the GPU shader at present time (lowest latency
    /// that still scales with the image).
    Gpu,
    /// Hand the cursor to the OS as a scaled custom bitmap, so the compositor
    /// moves it with zero present-loop latency.
    System,
}

#[derive(Parser, Debug)]
#[command(name = "dune", about = "Dune reimplemented")]
struct Args {
    /// Jump straight to the in-game UI (skip intro / credits / intro2).
    #[arg(long)]
    skip_intro: bool,

    /// How the mouse cursor is rendered.
    #[arg(long, value_enum, default_value_t = Cursor::System)]
    cursor: Cursor,

    /// The DUNE.DAT data file.
    dat_file: PathBuf,
}

fn main() {
    let args = Args::parse();
    let skip_intro = args.skip_intro;

    // Map the cursor mode to (CursorMode, system_cursor). Overlay means the game
    // only publishes the cursor shape (never bakes it); the GPU overlay then
    // draws it unless system_cursor hands it to the OS instead.
    let (cursor_mode, system_cursor) = match args.cursor {
        Cursor::Software => (CursorMode::Baked, false),
        Cursor::Gpu => (CursorMode::Overlay, false),
        Cursor::System => (CursorMode::Overlay, true),
    };

    let dat_path = &args.dat_file;
    let Ok(dat_file) = DatFile::open(dat_path) else {
        println!("Failed to open dat file '{}'", dat_path.display());
        return;
    };

    let event_loop = EventLoop::new().unwrap();

    let frame_slot = FrameSlot::new();
    let game_frame_slot = frame_slot.clone();
    let (start_sender, start_receiver) = mpsc::channel();

    // Shared keyboard + mouse state: the event loop (this thread) writes it, the
    // game thread polls it. Both ends hold a handle to the same Arc.
    let input = InputState::shared();
    let game_input = input.clone();

    // Cursor shape/visibility published by the game thread (in Overlay mode)
    // for the present thread's GPU overlay. Position is sampled separately
    // from `input` so the present path picks up the freshest pointer move.
    let shared_cursor = SharedCursor::new();
    let game_cursor = shared_cursor.clone();

    // Spawn game thread that waits for the event loop to start
    std::thread::spawn(move || {
        // Wait for signal that event loop is ready
        let _ = start_receiver.recv();

        let mut game = GameState::new_with_input_and_cursor(
            dat_file,
            game_frame_slot,
            game_input,
            cursor_mode,
            game_cursor,
        );

        game.start(skip_intro);
        // = seg000:0037 call game_loop — run the in-game loop after start's setup
        // (the port hoists this call out of start() so headless renders can reuse
        // start without entering the loop).
        game.game_loop();
    });

    let mut app = App {
        window: None,
        gpu: None,
        frame_slot,
        start_signal: Some(start_sender),
        input,
        cursor_mode,
        shared_cursor,
        cursor_in_game_area: false,
        mouse_buttons: 0,
        last_frame: None,
        screenshot_seq: 0,
        occluded: false,
        system_cursor,
        system_cursors: None,
        system_cursor_applied: None,
    };

    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app).unwrap();
}
