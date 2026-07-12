use crate::room_renderer::galois_noise_generator::GaloisNoiseGenerator;

#[derive(Clone, Debug)]
pub struct Room {
    // = the SAL room chunk's leading byte: how many standing-person positions
    // the room defines. sal_read_position_markers sizes its marker array to
    // this; the room's `Part::Character` entries are matched against it.
    position_marker_count: u8,
    parts: Vec<Part>,
}

#[derive(Clone, Debug)]
pub struct Sprite {
    pub id: u16,
    pub x: u16,
    pub y: u8,
    pub flip_x: bool,
    pub flip_y: bool,
    pub scale: u8,
    pub pal_offset: u8,
}

#[derive(Clone, Debug)]
pub struct Character {
    pub x: u16,
    pub y: u8,
    pub pal_offset: u8,
    // = cmd bits 10..12 (cmd >> 10) & 7: the perspective scale-down selector,
    // shared with Part::Sprite. draw_sprite_clobbering_bx_dx (seg000:c25e)
    // routes a non-zero value through the scaled blit so a person standing
    // deeper in the room is drawn smaller.
    pub scale: u8,
    // = cmd bits 0x4000 / 0x2000. loc_03b80 keeps the cmd's high byte in AH
    // (only the x>=256 bit 0x0200 is masked off); the character-entry path
    // routes through sal_draw_character (3d2f) which stashes AH in CH and
    // restores it before draw_sprite, where bit 0x40 = flip_x and 0x20 =
    // flip_y. So characters honour the same flip bits as Part::Sprite.
    pub flip_x: bool,
    pub flip_y: bool,
}

#[derive(Clone, Debug)]
pub struct Polygon {
    pub right_vertices: Vec<(i16, i16)>,
    pub left_vertices: Vec<(i16, i16)>,
    pub h_gradient: i16,
    pub v_gradient: i16,
    pub reverse_gradient: bool,
    pub color: u8,
    pub noise: GaloisNoiseGenerator,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub p0: (i16, i16),
    pub p1: (i16, i16),
    pub color: u8,
    pub dither: u16,
}

#[derive(Clone, Debug)]
pub enum Part {
    Sprite(Sprite),
    Character(Character),
    Polygon(Polygon),
    Line(Line),
}

impl Room {
    pub fn new() -> Self {
        Self {
            position_marker_count: 0,
            parts: Vec::new(),
        }
    }

    pub fn set_position_marker_count(&mut self, count: u8) {
        self.position_marker_count = count;
    }

    pub fn position_marker_count(&self) -> u8 {
        self.position_marker_count
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn add_part(&mut self, part: Part) {
        self.parts.push(part);
    }

    pub fn remove_part(&mut self, index: usize) {
        self.parts.remove(index);
    }
}

impl Default for Room {
    fn default() -> Self {
        Self::new()
    }
}
