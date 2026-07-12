#![allow(clippy::too_many_arguments)]

mod galois_noise_generator;
mod room;
mod room_sheet;

use std::{io::Cursor, mem::swap};

use bytes_ext::ReadBytesExt;
pub use room::Room;
pub use room_sheet::RoomSheet;

use crate::{
    Color, FrameBuffer, Palette, Point, SpriteSheet,
    room_renderer::room::{Character, Part, Polygon},
    sprite_blitter,
};

pub struct RoomRenderer {
    room: Option<Room>,
    sprite_sheet: Option<SpriteSheet>,
    character_sheet: Option<SpriteSheet>,
    position_markers: Vec<i8>,
    y_offset: i16,
}

pub struct DrawOptions {
    pub draw_sprites: bool,
    pub draw_polygons: bool,
    pub draw_lines: bool,
    /// Whether to draw the standing `Part::Character` people. = the
    /// `room_render_flags & 0x81` gate in sal_draw_character_entry (seg000:3d12):
    /// the dialogue-zoom re-render sets bit 7, suppressing the standing sprites
    /// so the zoomed backdrop behind the talking head has no tiny figure on it.
    pub draw_characters: bool,
}

impl Default for DrawOptions {
    fn default() -> Self {
        Self {
            draw_sprites: true,
            draw_polygons: true,
            draw_lines: true,
            draw_characters: true,
        }
    }
}

impl RoomRenderer {
    pub fn new() -> Self {
        Self {
            room: None,
            sprite_sheet: None,
            character_sheet: None,
            position_markers: Vec::new(),
            y_offset: 0,
        }
    }

    pub fn set_room(&mut self, room: Room) {
        self.room = Some(room);
    }

    pub fn set_sprite_sheet(&mut self, sprite_sheet: SpriteSheet) {
        self.sprite_sheet = Some(sprite_sheet);
    }

    /// The PERS.HSQ sprite sheet (RES_PERS_HSQ) holding the person sprites
    /// drawn for `Part::Character` slots. Without it, character parts are
    /// skipped. = the resource `sal_draw_character` (seg000:3d2f) opens.
    pub fn set_character_sheet(&mut self, character_sheet: SpriteSheet) {
        self.character_sheet = Some(character_sheet);
    }

    /// = sal_read_position_markers output: one entry per standing position in
    /// the room (0xff = empty). `Part::Character` entries consume these
    /// back-to-front as the room is drawn.
    pub fn set_position_markers(&mut self, markers: Vec<i8>) {
        self.position_markers = markers;
    }

    /// Set the destination y-offset added to every drawn part. Mirrors the
    /// segvga `fb_base_ofs` that the DOS room blits auto-apply: the intro
    /// draws the room into the game-view area at fb_base_ofs = 24.
    pub fn set_y_offset(&mut self, y_offset: i16) {
        self.y_offset = y_offset;
    }

    pub fn get_sprite_sheet(&mut self) -> Option<&SpriteSheet> {
        self.sprite_sheet.as_ref()
    }

    pub fn draw_and_write_ppm_parts(
        &self,
        room: &Room,
        sprite_sheet: &SpriteSheet,
        pal: &Palette,
        frame: &mut FrameBuffer,
    ) -> Result<(), std::io::Error> {
        for (i, part) in room.parts().iter().enumerate() {
            self.draw_part(part, sprite_sheet, frame)?;

            let filename = format!("room-part-{i:02}.ppm");
            frame.write_ppm_scaled(pal, &filename)?;
        }

        Ok(())
    }

    pub fn draw_sky(
        &self,
        sky_asset: &[u8],
        sky_palette_index: usize,
        pal: &mut Palette,
        // frame: &mut Framebuffer,
    ) {
        let mut c = Cursor::new(sky_asset);
        let toc_pos = c.read_le_u16().unwrap() as u64;
        c.set_position(toc_pos + (8 + sky_palette_index.min(32) as u64) * 2);
        let sub_ofs = c.read_le_u16().unwrap() as u64;
        c.set_position(toc_pos + sub_ofs + 6);

        let pal_ofs = 73;
        let pal_cnt = 151;
        for i in 0..pal_cnt {
            let r = c.read_u8().unwrap();
            let g = c.read_u8().unwrap();
            let b = c.read_u8().unwrap();

            pal.set(pal_ofs + i, Color(r, g, b));
        }

        // let sky_sprite_sheet = SpriteSheet::new(sky_asset).unwrap();

        // for sprite_id in 0..4 {
        //     let sprite = sky_sprite_sheet.get_sprite(sprite_id).unwrap();
        //     for col in 0..8 {
        //         let y = 20 * sprite_id;
        //         let x = 40 * col;
        //         sprite
        //             .draw(sprite_id, x, y, false, false, 0, 0, frame, &mut None)
        //             .unwrap();
        //     }
        // }
    }

    pub fn draw(
        &self,
        options: &DrawOptions,
        frame: &mut FrameBuffer,
    ) -> Result<(), std::io::Error> {
        let Some(room) = &self.room else {
            return Ok(());
        };
        let Some(sprite_sheet) = &self.sprite_sheet else {
            return Ok(());
        };

        // = draw_SAL (seg000:3b59): each `Part::Character` entry pops the next
        // position marker. DOS reads them back-to-front — sal_marker_sp points
        // at the last slot and is decremented per character entry.
        let mut marker_idx = self.position_markers.len() as isize - 1;
        for part in room.parts() {
            if let Part::Character(character) = part {
                // = sal_draw_character_entry (loc_03d12): `test room_render_flags,
                // 81h; jnz` — when bit 0 or 7 is set the whole entry is skipped,
                // not even consuming a position marker (the dialogue-zoom
                // re-render sets bit 7 so no standing person is drawn).
                if !options.draw_characters {
                    continue;
                }
                // Consume one marker; an empty slot (0xff) draws nobody.
                let id = if marker_idx >= 0 {
                    let id = self.position_markers[marker_idx as usize];
                    marker_idx -= 1;
                    id
                } else {
                    -1
                };
                if id != -1 {
                    self.draw_character(character, id, frame)?;
                }
                continue;
            }
            if Self::should_draw(options, part) {
                self.draw_part(part, sprite_sheet, frame)?;
            }
        }

        Ok(())
    }

    // = the (id, x, y) anchors sal_draw_character (seg000:3d2f) records into
    // character_x_table/character_y_table (seg001:47f8) as it draws each standing
    // person: indexed by person id, x = the entry anchor, y = anchor + y_offset
    // (the same coordinates `draw` blits at). The marker→character pairing mirrors
    // `draw` exactly (markers consumed back-to-front, one per `Part::Character`).
    pub fn character_screen_positions(&self) -> Vec<(i8, i16, i16)> {
        let mut out = Vec::new();
        let Some(room) = &self.room else {
            return out;
        };
        let mut marker_idx = self.position_markers.len() as isize - 1;
        for part in room.parts() {
            if let Part::Character(character) = part {
                let id = if marker_idx >= 0 {
                    let id = self.position_markers[marker_idx as usize];
                    marker_idx -= 1;
                    id
                } else {
                    -1
                };
                if id != -1 {
                    out.push((id, character.x as i16, character.y as i16 + self.y_offset));
                }
            }
        }
        out
    }

    fn should_draw(options: &DrawOptions, part: &Part) -> bool {
        match part {
            Part::Sprite(_) => options.draw_sprites,
            // Characters are handled in `draw` (they consume position markers),
            // never via `should_draw`.
            Part::Character(_) => false,
            Part::Polygon(_) => options.draw_polygons,
            Part::Line(_) => options.draw_lines,
        }
    }

    // = seg000:3d2f sal_draw_character. Draw the standing person `id` from the
    // PERS.HSQ sheet. character_id_to_sprite (seg000:9123) maps id -> sprite-
    // pair index — identity for id < 0x0d, the only case the intro hits — and
    // the pair (sprite*2, sprite*2+1) is blitted at the entry's (x, y) anchor
    // with its pal_offset (= byte_1C21A_pal_offset). DOS draws both sprites at
    // the same anchor: draw_sprite preserves bx/dx, then
    // draw_sprite_clobbering_bx_dx reuses them.
    fn draw_character(
        &self,
        character: &Character,
        id: i8,
        framebuffer: &mut FrameBuffer,
    ) -> Result<(), std::io::Error> {
        let Some(sheet) = &self.character_sheet else {
            return Ok(());
        };

        let sprite = id as u16;

        for sprite_id in [sprite * 2, sprite * 2 + 1] {
            let Some(sprite) = sheet.get_sprite(sprite_id) else {
                continue;
            };
            sprite_blitter(sprite, framebuffer)
                .at(character.x as i16, character.y as i16 + self.y_offset)
                .flip_x(character.flip_x)
                .flip_y(character.flip_y)
                .scale(character.scale)
                .pal_offset(character.pal_offset)
                .draw()?;
        }
        Ok(())
    }

    fn draw_part(
        &self,
        part: &Part,
        sprite_sheet: &SpriteSheet,
        framebuffer: &mut FrameBuffer,
    ) -> Result<(), std::io::Error> {
        match part {
            Part::Sprite(sprite_part) => {
                let Some(sprite) = sprite_sheet.get_sprite(sprite_part.id) else {
                    return Ok(());
                };

                sprite_blitter(sprite, framebuffer)
                    .at(sprite_part.x as i16, sprite_part.y as i16 + self.y_offset)
                    .flip_x(sprite_part.flip_x)
                    .flip_y(sprite_part.flip_y)
                    .scale(sprite_part.scale)
                    .pal_offset(sprite_part.pal_offset)
                    .draw()?;
            }
            Part::Character(_) => {}
            Part::Polygon(polygon_part) => {
                self.draw_polygon(polygon_part, framebuffer);
            }
            Part::Line(line_part) => {
                self.draw_line(
                    line_part.p0.into(),
                    line_part.p1.into(),
                    line_part.color,
                    line_part.dither,
                    framebuffer,
                );
            }
        }
        Ok(())
    }

    fn draw_line(&self, p0: Point, p1: Point, color: u8, dither: u16, frame: &mut FrameBuffer) {
        let mut dither = dither;

        bresenham_line(p0, p1, |p| {
            dither = dither.rotate_left(1);
            if dither & 1 != 0 {
                let x = p.x as u16;
                let y = (p.y + self.y_offset) as u16;
                frame.set(x, y, color);
            }
        });
    }

    fn draw_polygon(&self, polygon: &Polygon, frame: &mut FrameBuffer) {
        let mut right_side = [0i16; 200];
        let mut left_side = [0i16; 200];

        let mut xi = 0;
        let start_p = polygon.right_vertices[0];

        // Part 1
        let mut last_p = polygon.right_vertices[0];
        polygon.right_vertices.iter().skip(1).for_each(|&p| {
            draw_edge(last_p.into(), p.into(), &mut right_side, &mut xi);
            last_p = p;
        });
        let final_p = last_p;

        // Part 2
        xi = 0;
        let mut last_p = polygon.right_vertices[0];
        polygon.left_vertices.iter().for_each(|&p| {
            draw_edge(last_p.into(), p.into(), &mut left_side, &mut xi);
            last_p = p;
        });

        draw_edge(last_p.into(), final_p.into(), &mut left_side, &mut xi);

        let mut noise_generator = polygon.noise.clone();
        let mut line_color = (polygon.color as u16) << 8;

        for y in 0..final_p.1 - start_p.1 {
            let mut x0 = left_side[y as usize];
            let mut x1 = right_side[y as usize];
            if x0 > x1 {
                swap(&mut x0, &mut x1);
            }

            let mut color = line_color;
            for x in x0..=x1 {
                let rand = noise_generator.rand() & 3;

                let x = if !polygon.reverse_gradient {
                    x
                } else {
                    x0 + (x1 - x)
                };

                let y = y + start_p.1 + self.y_offset;

                let x = x as u16;
                let y = y as u16;
                frame.set(x, y, (rand + (color >> 8) - 1) as u8);
                color = color.strict_add_signed(polygon.h_gradient);
            }
            line_color = line_color.strict_add_signed(polygon.v_gradient);
        }
    }
}

impl Default for RoomRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// = seg000:3d83 sal_read_position_markers + seg000:3df4
// sal_assign_position_marker. Build the room's standing-position marker array:
// `count` slots, each 0xff (empty) unless a person is assigned. For each set
// bit of `persons_in_room ^ persons_travelling_with` (person index N),
// sal_assign_position_marker places N into slot (N + base) % count, or the
// first free slot if the preferred one is taken.
//
// The DOS `base` is `person_marker_base & 0x0f` (seg000:3dbe..3dc2); a second
// pass fills extra generic NPCs from `[476ah]`, in-game state that is 0 during
// the intro and is omitted here (TODO when in-game scenes drive this).
pub fn sal_position_markers(
    count: u8,
    persons_in_room: u16,
    persons_travelling_with: u16,
    person_marker_base: u8,
) -> Vec<i8> {
    let count = count as usize;
    let mut markers = vec![-1; count];
    if count == 0 {
        return markers;
    }

    // = seg000:3dbe mov ch,[person_marker_base]; seg000:3dc2 and ch,0fh.
    let base = (person_marker_base & 0x0f) as usize;
    let mut bits = persons_in_room ^ persons_travelling_with;
    let mut id = 0;
    while bits != 0 {
        if bits & 1 != 0 {
            assign_position_marker(&mut markers, id, base);
        }
        bits >>= 1;
        id += 1;
    }

    markers
}

// = seg000:3df4 sal_assign_position_marker.
fn assign_position_marker(markers: &mut [i8], id: i8, base: usize) {
    let count = markers.len();
    let preferred = (id as usize + base) % count;
    if markers[preferred] == -1 {
        markers[preferred] = id;
        return;
    }
    if let Some(slot) = markers.iter().position(|&m| m == -1) {
        markers[slot] = id;
    }
}

fn draw_edge(p0: Point, p1: Point, xs: &mut [i16; 200], xi: &mut usize) {
    let x0 = p0.x;
    let y0 = p0.y;
    let x1 = p1.x;
    let y1 = p1.y;
    let dx = x1.abs_diff(x0);
    let dy = y1.abs_diff(y0);

    if dx == 0 && dy == 0 {
        return;
    }

    if dy == 0 {
        xs[*xi] = i16::min(x0, x1);
        *xi += 1;
        return;
    }

    if dx == 0 {
        for _ in y0..=y1 {
            xs[*xi] = x0;
            *xi += 1;
        }
        return;
    }

    let sign_x: i16 = if x0 < x1 { 1 } else { -1 };
    let sign_y: i16 = if y0 < y1 { 1 } else { -1 };

    let bp_6 = sign_y;
    let bp_4 = sign_x;
    let mut bp_2 = sign_y;
    let mut bp_0 = sign_x;

    let mut minor_delta = dy;
    let mut major_delta = dx;

    if dx > dy {
        bp_2 = 0;
    } else {
        swap(&mut minor_delta, &mut major_delta);
        bp_0 = 0;
    }

    let mut x0 = x0;
    let mut ax = major_delta / 2;
    let mut cx = major_delta;
    loop {
        ax += minor_delta;

        let mut dx;
        let bx;
        if ax >= major_delta {
            ax -= major_delta;
            dx = bp_4;
            bx = bp_6;
        } else {
            dx = bp_0;
            bx = bp_2;
        }

        dx += x0;

        if bx == 1 {
            xs[*xi] = x0;
            *xi += 1;
        }

        x0 = dx;
        cx -= 1;
        if cx == 0 {
            break;
        }
    }
}

fn bresenham_line<F>(p0: Point, p1: Point, mut f: F)
where
    F: FnMut(Point),
{
    let mut x0 = p0.x;
    let mut y0 = p0.y;
    let mut x1 = p1.x;
    let mut y1 = p1.y;

    if x0 > x1 {
        swap(&mut x0, &mut x1);
        swap(&mut y0, &mut y1);
    }

    let dx = i16::abs(x1 - x0);
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -i16::abs(y1 - y0);
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        f((x0, y0).into());
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * error;
        if e2 >= dy {
            error += dy;
            x0 += sx;
        }
        if e2 <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sal_position_markers;

    #[test]
    fn no_persons_leaves_all_slots_empty() {
        assert_eq!(sal_position_markers(3, 0, 0, 0), [-1, -1, -1]);
    }

    #[test]
    fn zero_count_is_empty() {
        assert!(sal_position_markers(0, 2, 0, 0).is_empty());
    }

    #[test]
    fn lady_jessica_stands_in_the_single_slot() {
        // intro_14: persons_in_room = 2 (person index 1), one standing slot.
        assert_eq!(sal_position_markers(1, 2, 0, 0), [1]);
    }

    #[test]
    fn person_index_goes_to_preferred_slot() {
        // person index 1 -> slot (1 + base=0) % 3 = 1.
        assert_eq!(sal_position_markers(3, 0b10, 0, 0), [-1, 1, -1]);
    }

    #[test]
    fn travelling_companions_are_xored_out() {
        // A person both in the room and travelling with the player cancels.
        assert_eq!(sal_position_markers(2, 0b10, 0b10, 0), [-1, -1]);
    }

    #[test]
    fn multiple_persons_take_distinct_slots() {
        // person indices 1 and 2 -> slots 1 and 2.
        assert_eq!(sal_position_markers(3, 0b110, 0, 0), [-1, 1, 2]);
    }

    #[test]
    fn preferred_slot_collision_falls_back_to_first_free() {
        // count 2, person indices 0 and 2 both prefer slot 0; index 2 spills to
        // the first free slot.
        assert_eq!(sal_position_markers(2, 0b101, 0, 0), [0, 2]);
    }

    #[test]
    fn person_marker_base_rotates_the_preferred_slot() {
        // Only the low nibble of person_marker_base is the base. person index 1
        // -> slot (1 + base) % 3: base 1 -> slot 2, base 0x12 (nibble 2) -> slot
        // (1 + 2) % 3 = 0.
        assert_eq!(sal_position_markers(3, 0b10, 0, 1), [-1, -1, 1]);
        assert_eq!(sal_position_markers(3, 0b10, 0, 0x12), [1, -1, -1]);
    }
}
