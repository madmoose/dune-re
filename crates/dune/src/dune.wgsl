// Fullscreen-triangle upscaler for the 320x200 game framebuffer.
//
// Two stages: a nearest-neighbour integer prescale (factors nx, ny) of the
// source, followed by a separable Lanczos-3 downscale to the on-screen fit-rect
// size. The prescale first keeps the pixel grid on whole texels (crisp, even
// edges) while the downscale stays in the ring-free, even regime. Everything is
// driven by the Uniforms block so the host resizes the window without rebuilding
// any GPU state.

struct Uniforms {
    // (src_w, src_h, dst_w, dst_h) — dst is the fit-rect (viewport) size in px.
    dims: vec4<u32>,
    // (nx, ny, supp_x, supp_y) — integer prescale factors and Lanczos tap counts.
    pre: vec4<u32>,
    // (a, _, _, _) — Lanczos kernel radius parameter (a = 3).
    kernel: vec4<f32>,
    // (x_top_left, y_top_left, shape_idx, visible) for the present-time
    // cursor overlay, expressed in 320×200 source-pixel coordinates.
    cursor: vec4<i32>,
    // (r, g, b, _) — present-time palette[0x0f], the cursor's "white".
    cursor_color: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: Uniforms;
// 16 × (16 × shape count) atlas: each texel encodes AND in bit 0 and OR in
// bit 1, matching the per-pixel test in `vga_draw_cursor`.
@group(0) @binding(2) var cursor_masks: texture_2d<u32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // One oversized triangle covering the viewport; uv spans 0..1 with the origin
    // at the top-left to match the texture's row order.
    var clip = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = clip[vi];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}

const PI: f32 = 3.14159265358979323846;

fn sinc(x: f32) -> f32 {
    if (x == 0.0) {
        return 1.0;
    }
    let px = PI * x;
    return sin(px) / px;
}

fn lanczos(x: f32, a: f32) -> f32 {
    if (abs(x) >= a) {
        return 0.0;
    }
    return sinc(x) * sinc(x / a);
}

// Apply the present-time cursor overlay to a source-space texel. If the
// (src_col, src_row) coordinate falls inside the cursor's 16×16 footprint,
// the AND/OR mask decides between keeping the background, drawing the
// cursor "white" (palette[0x0f]) or drawing black — same rules as
// `vga_draw_cursor` (gfx.rs:832-837).
fn apply_cursor(texel: vec3<f32>, src_col: i32, src_row: i32) -> vec3<f32> {
    if (u.cursor.w == 0) {
        return texel;
    }
    let lx = src_col - u.cursor.x;
    let ly = src_row - u.cursor.y;
    if (lx < 0 || lx >= 16 || ly < 0 || ly >= 16) {
        return texel;
    }
    let mask_y = u.cursor.z * 16 + ly;
    let m = textureLoad(cursor_masks, vec2<i32>(lx, mask_y), 0).r;
    // AND bit set → transparent: keep background.
    if ((m & 1u) != 0u) {
        return texel;
    }
    // AND clear, OR set → cursor colour (palette[0x0f]); else black.
    if ((m & 2u) != 0u) {
        return u.cursor_color.rgb;
    }
    return vec3<f32>(0.0, 0.0, 0.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let src_w = u.dims.x;
    let src_h = u.dims.y;
    let dst_w = u.dims.z;
    let dst_h = u.dims.w;
    let nx = u.pre.x;
    let ny = u.pre.y;
    let supp_x = u.pre.z;
    let supp_y = u.pre.w;
    let a = u.kernel.x;

    let pre_w = i32(src_w * nx);
    let pre_h = i32(src_h * ny);

    // Output pixel index within the viewport.
    let d_x = min(u32(floor(in.uv.x * f32(dst_w))), dst_w - 1u);
    let d_y = min(u32(floor(in.uv.y * f32(dst_h))), dst_h - 1u);

    // Per-axis Lanczos setup. The source sizes here are the prescaled
    // dimensions, so scale <= 1 (a downscale).
    let scale_x = f32(dst_w) / f32(pre_w);
    let scale_y = f32(dst_h) / f32(pre_h);
    let s_in_x = min(scale_x, 1.0);
    let s_in_y = min(scale_y, 1.0);
    let radius_x = a / s_in_x;
    let radius_y = a / s_in_y;
    let s_pos_x = (f32(d_x) + 0.5) / scale_x - 0.5;
    let s_pos_y = (f32(d_y) + 0.5) / scale_y - 0.5;
    let s0_x = i32(floor(s_pos_x - radius_x));
    let s0_y = i32(floor(s_pos_y - radius_y));

    // Per-axis weight sums for normalisation, each axis normalised separately.
    // The 2D normaliser is their product, since the weights are separable.
    var wsum_x = 0.0;
    for (var k = 0u; k < supp_x; k = k + 1u) {
        let s = s0_x + i32(k);
        wsum_x = wsum_x + lanczos((f32(s) - s_pos_x) * s_in_x, a);
    }
    var wsum_y = 0.0;
    for (var j = 0u; j < supp_y; j = j + 1u) {
        let s = s0_y + i32(j);
        wsum_y = wsum_y + lanczos((f32(s) - s_pos_y) * s_in_y, a);
    }
    let wnorm = wsum_x * wsum_y;
    if (wnorm == 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    var acc = vec3<f32>(0.0, 0.0, 0.0);
    for (var j = 0u; j < supp_y; j = j + 1u) {
        let sy = s0_y + i32(j);
        let wy = lanczos((f32(sy) - s_pos_y) * s_in_y, a);
        // Prescaled row -> source row via integer division by ny (nearest block).
        let src_row = clamp(sy, 0, pre_h - 1) / i32(ny);
        for (var k = 0u; k < supp_x; k = k + 1u) {
            let sx = s0_x + i32(k);
            let wx = lanczos((f32(sx) - s_pos_x) * s_in_x, a);
            let src_col = clamp(sx, 0, pre_w - 1) / i32(nx);
            let raw = textureLoad(src_tex, vec2<i32>(src_col, src_row), 0).rgb;
            let texel = apply_cursor(raw, src_col, src_row);
            acc = acc + texel * (wx * wy);
        }
    }

    return vec4<f32>(acc / wnorm, 1.0);
}
