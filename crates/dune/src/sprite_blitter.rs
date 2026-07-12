use crate::{FrameBuffer, Rect, SpriteSheet, blit, sprite::Sprite};

pub struct SpriteBlitter<'a> {
    sprite: &'a Sprite,
    framebuffer: &'a mut FrameBuffer,
    x: i16,
    y: i16,
    clip_rect: Option<Rect>,
    flip_x: bool,
    flip_y: bool,
    scale: u8,
    pal_offset: u8,
}

impl<'a> SpriteBlitter<'a> {
    pub fn new(sprite: &'a Sprite, framebuffer: &'a mut FrameBuffer) -> Self {
        Self {
            sprite,
            framebuffer,
            x: 0,
            y: 0,
            clip_rect: None,
            flip_x: false,
            flip_y: false,
            scale: 0,
            pal_offset: sprite.pal_offset(),
        }
    }

    pub fn at(mut self, x: i16, y: i16) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn clip_rect(mut self, clip_rect: Rect) -> Self {
        self.clip_rect = Some(clip_rect);
        self
    }

    pub fn flip_x(mut self, flip_x: bool) -> Self {
        self.flip_x = flip_x;
        self
    }

    pub fn flip_y(mut self, flip_y: bool) -> Self {
        self.flip_y = flip_y;
        self
    }

    pub fn scale(mut self, scale: u8) -> Self {
        self.scale = scale;
        self
    }

    pub fn pal_offset(mut self, pal_offset: u8) -> Self {
        if pal_offset != 0 {
            self.pal_offset = pal_offset;
        }
        self
    }

    pub fn draw(self) -> std::io::Result<()> {
        blit::Blitter::new(self.sprite.data(), self.framebuffer)
            .at(self.x, self.y)
            .size(self.sprite.width(), self.sprite.height())
            .clip_rect(self.clip_rect)
            .flip_x(self.flip_x)
            .flip_y(self.flip_y)
            .scale(self.scale)
            .rle(self.sprite.rle())
            .pal_offset(self.pal_offset)
            .draw()
    }
}

pub fn sprite_blitter<'a>(
    sprite: &'a Sprite,
    framebuffer: &'a mut FrameBuffer,
) -> SpriteBlitter<'a> {
    SpriteBlitter::new(sprite, framebuffer)
}

pub fn draw_sprite(
    sprite: &Sprite,
    x: i16,
    y: i16,
    framebuffer: &mut FrameBuffer,
) -> std::io::Result<()> {
    blit::Blitter::new(sprite.data(), framebuffer)
        .at(x, y)
        .size(sprite.width(), sprite.height())
        .pal_offset(sprite.pal_offset())
        .rle(sprite.rle())
        .draw()
}

pub fn draw_sprite_from_sheet(
    sheet: &SpriteSheet,
    sprite_id: u16,
    x: i16,
    y: i16,
    framebuffer: &mut FrameBuffer,
) -> std::io::Result<()> {
    if let Some(sprite) = sheet.get_sprite(sprite_id) {
        draw_sprite(sprite, x, y, framebuffer)
    } else {
        Ok(())
    }
}
