#![allow(unused)]

use crate::{FbId, GameState, SpriteSheet, draw_sprite_from_sheet};

pub const ICONES: i16 = 0x00;
pub const FRESK: i16 = 0x01;
pub const LETO: i16 = 0x02;
pub const JESS: i16 = 0x03;
pub const HAWA: i16 = 0x04;
pub const IDAH: i16 = 0x05;
pub const GURN: i16 = 0x06;
pub const STIL: i16 = 0x07;
pub const KYNE: i16 = 0x08;
pub const CHAN: i16 = 0x09;
pub const HARA: i16 = 0x0a;
pub const BARO: i16 = 0x0b;
pub const FEYD: i16 = 0x0c;
pub const EMPR: i16 = 0x0d;
pub const HARK: i16 = 0x0e;
pub const SMUG: i16 = 0x0f;
pub const FRM1: i16 = 0x10;
pub const FRM2: i16 = 0x11;
pub const FRM3: i16 = 0x12;
pub const GENERIC: i16 = 0x13;
pub const PROUGE: i16 = 0x14;
pub const COMM: i16 = 0x15;
pub const EQUI: i16 = 0x16;
pub const BALCON: i16 = 0x17;
pub const CORR: i16 = 0x18;
pub const POR: i16 = 0x19;
pub const SIET1: i16 = 0x1a;
pub const XPLAIN9: i16 = 0x1b;
pub const BUNK: i16 = 0x1d;
pub const FINAL: i16 = 0x1e;
pub const SERRE: i16 = 0x1f;
pub const BOTA: i16 = 0x20;
pub const PALPLAN: i16 = 0x21;
pub const SUN: i16 = 0x22;
pub const VIS: i16 = 0x23;
pub const ORNYPAN: i16 = 0x24;
pub const ONMAP: i16 = 0x25;
pub const PERS: i16 = 0x26;
pub const CHANKISS: i16 = 0x27;
pub const SKY: i16 = 0x28;
pub const SKYDN: i16 = 0x29;
pub const ORNYTK: i16 = 0x2a;
pub const ATTACK: i16 = 0x2b;
pub const STARS: i16 = 0x2c;
pub const INTDS: i16 = 0x2d;
pub const SUNRS: i16 = 0x2e;
pub const PAUL: i16 = 0x2f;
pub const BACK: i16 = 0x30;
pub const MOIS: i16 = 0x31;
pub const BOOK: i16 = 0x32;
pub const ORNY: i16 = 0x33;
pub const ORNYCAB: i16 = 0x34;
pub const VER: i16 = 0x39;
pub const MAP2: i16 = 0x3a;
pub const MIRROR: i16 = 0x3b;
pub const DS0: i16 = 0x3c;
pub const DS1: i16 = 0x3d;
pub const DS2: i16 = 0x3e;
pub const DS3: i16 = 0x3f;
pub const DS4: i16 = 0x40;
pub const DN20: i16 = 0x42;
pub const DN21: i16 = 0x43;
pub const DN22: i16 = 0x44;
pub const DN23: i16 = 0x45;
pub const DN24: i16 = 0x46;
pub const DN25: i16 = 0x47;
pub const DN26: i16 = 0x48;
pub const DN27: i16 = 0x49;
pub const DN28: i16 = 0x4a;
pub const DN29: i16 = 0x4b;
pub const DN30: i16 = 0x4c;
pub const DN31: i16 = 0x4d;
pub const DN32: i16 = 0x4e;
pub const DN33: i16 = 0x4f;
pub const DN34: i16 = 0x50;
pub const DN35: i16 = 0x51;
pub const DN36: i16 = 0x52;
pub const DN37: i16 = 0x53;
pub const DN38: i16 = 0x54;
pub const MIXR: i16 = 0x55;
pub const INT02: i16 = 0x56;
pub const INT04: i16 = 0x57;
pub const INT05: i16 = 0x58;
pub const INT06: i16 = 0x59;
pub const INT07: i16 = 0x5a;
pub const INT08: i16 = 0x5b;
pub const INT09: i16 = 0x5c;
pub const INT10: i16 = 0x5d;
pub const INT11: i16 = 0x5e;
pub const INT13: i16 = 0x5f;
pub const INT14: i16 = 0x60;
pub const INT15: i16 = 0x61;
pub const PALAIS: i16 = 0x62;
pub const MNT1: i16 = 0x63;
pub const MNT2: i16 = 0x64;
pub const MNT3: i16 = 0x65;
pub const MNT4: i16 = 0x66;
pub const SIET: i16 = 0x67;
pub const PALACE: i16 = 0x68;
pub const IRUL1: i16 = 0x69;
pub const IRUL2: i16 = 0x6a;
pub const IRUL3: i16 = 0x6b;
pub const IRUL4: i16 = 0x6c;
pub const IRUL5: i16 = 0x6d;
pub const IRUL6: i16 = 0x6e;
pub const IRUL7: i16 = 0x6f;
pub const IRUL8: i16 = 0x70;
pub const IRUL9: i16 = 0x71;
pub const DP1: i16 = 0x72;
pub const DP0: i16 = 0x73;
pub const DP2: i16 = 0x74;
pub const DP3: i16 = 0x75;
pub const DF1: i16 = 0x76;
pub const DF2: i16 = 0x77;
pub const DF3: i16 = 0x78;
pub const DF4: i16 = 0x79;
pub const VIL1: i16 = 0x7a;
pub const VIL2: i16 = 0x7b;
pub const VIL3: i16 = 0x7c;
pub const VIL4: i16 = 0x7d;
pub const VIL5: i16 = 0x7e;
pub const VIL6: i16 = 0x7f;
pub const DV1: i16 = 0x80;
pub const DV2: i16 = 0x81;
pub const DV3: i16 = 0x82;
pub const DV4: i16 = 0x83;
pub const DH0: i16 = 0x84;
pub const DH1: i16 = 0x85;
pub const DH2: i16 = 0x86;
pub const DH3: i16 = 0x87;
pub const VG01: i16 = 0x88;
pub const VG02: i16 = 0x89;
pub const VG03: i16 = 0x8a;
pub const VG04: i16 = 0x8b;
pub const VG05: i16 = 0x8c;
pub const VG06: i16 = 0x8d;
pub const VG07: i16 = 0x8e;
pub const VG08: i16 = 0x8f;
pub const VG09: i16 = 0x90;
pub const VG10: i16 = 0x91;
const NO_BANK: u16 = u16::MAX;

const BANK_NAMES: &[&str] = &[
    "ICONES.HSQ",   // 0x00
    "FRESK.HSQ",    // 0x01
    "LETO.HSQ",     // 0x02
    "JESS.HSQ",     // 0x03
    "HAWA.HSQ",     // 0x04
    "IDAH.HSQ",     // 0x05
    "GURN.HSQ",     // 0x06
    "STIL.HSQ",     // 0x07
    "KYNE.HSQ",     // 0x08
    "CHAN.HSQ",     // 0x09
    "HARA.HSQ",     // 0x0a
    "BARO.HSQ",     // 0x0b
    "FEYD.HSQ",     // 0x0c
    "EMPR.HSQ",     // 0x0d
    "HARK.HSQ",     // 0x0e
    "SMUG.HSQ",     // 0x0f
    "FRM1.HSQ",     // 0x10
    "FRM2.HSQ",     // 0x11
    "FRM3.HSQ",     // 0x12
    "GENERIC.HSQ",  // 0x13
    "PROUGE.HSQ",   // 0x14
    "COMM.HSQ",     // 0x15
    "EQUI.HSQ",     // 0x16
    "BALCON.HSQ",   // 0x17
    "CORR.HSQ",     // 0x18
    "POR.HSQ",      // 0x19
    "SIET1.HSQ",    // 0x1a
    "XPLAIN9.HSQ",  // 0x1b
    "libre",        // 0x1c
    "BUNK.HSQ",     // 0x1d
    "FINAL.HSQ",    // 0x1e
    "SERRE.HSQ",    // 0x1f
    "BOTA.HSQ",     // 0x20
    "PALPLAN.HSQ",  // 0x21
    "SUN.HSQ",      // 0x22
    "VIS.HSQ",      // 0x23
    "ORNYPAN.HSQ",  // 0x24
    "ONMAP.HSQ",    // 0x25
    "PERS.HSQ",     // 0x26
    "CHANKISS.HSQ", // 0x27
    "SKY.HSQ",      // 0x28
    "SKYDN.HSQ",    // 0x29
    "ORNYTK.HSQ",   // 0x2a
    "ATTACK.HSQ",   // 0x2b
    "STARS.HSQ",    // 0x2c
    "INTDS.HSQ",    // 0x2d
    "SUNRS.HSQ",    // 0x2e
    "PAUL.HSQ",     // 0x2f
    "BACK.HSQ",     // 0x30
    "MOIS.HSQ",     // 0x31
    "BOOK.HSQ",     // 0x32
    "ORNY.HSQ",     // 0x33
    "ORNYCAB.HSQ",  // 0x34
    "libre.HSQ",    // 0x35
    "libre.HSQ",    // 0x36
    "libre.HSQ",    // 0x37
    "libre",        // 0x38
    "VER.HSQ",      // 0x39
    "MAP2.HSQ",     // 0x3a
    "MIRROR.HSQ",   // 0x3b
    "DS0.HSQ",      // 0x3c
    "DS1.HSQ",      // 0x3d
    "DS2.HSQ",      // 0x3e
    "DS3.HSQ",      // 0x3f
    "DS4.HSQ",      // 0x40
    "libre",        // 0x41
    "DN20.HSQ",     // 0x42
    "DN21.HSQ",     // 0x43
    "DN22.HSQ",     // 0x44
    "DN23.HSQ",     // 0x45
    "DN24.HSQ",     // 0x46
    "DN25.HSQ",     // 0x47
    "DN26.HSQ",     // 0x48
    "DN27.HSQ",     // 0x49
    "DN28.HSQ",     // 0x4a
    "DN29.HSQ",     // 0x4b
    "DN30.HSQ",     // 0x4c
    "DN31.HSQ",     // 0x4d
    "DN32.HSQ",     // 0x4e
    "DN33.HSQ",     // 0x4f
    "DN34.HSQ",     // 0x50
    "DN35.HSQ",     // 0x51
    "DN36.HSQ",     // 0x52
    "DN37.HSQ",     // 0x53
    "DN38.HSQ",     // 0x54
    "MIXR.HSQ",     // 0x55
    "INT02.HSQ",    // 0x56
    "INT04.HSQ",    // 0x57
    "INT05.HSQ",    // 0x58
    "INT06.HSQ",    // 0x59
    "INT07.HSQ",    // 0x5a
    "INT08.HSQ",    // 0x5b
    "INT09.HSQ",    // 0x5c
    "INT10.HSQ",    // 0x5d
    "INT11.HSQ",    // 0x5e
    "INT13.HSQ",    // 0x5f
    "INT14.HSQ",    // 0x60
    "INT15.HSQ",    // 0x61
    "PALAIS.HSQ",   // 0x62
    "MNT1.LOP",     // 0x63
    "MNT2.LOP",     // 0x64
    "MNT3.LOP",     // 0x65
    "MNT4.LOP",     // 0x66
    "SIET.LOP",     // 0x67
    "PALACE.LOP",   // 0x68
    "IRUL1.HSQ",    // 0x69
    "IRUL2.HSQ",    // 0x6a
    "IRUL3.HSQ",    // 0x6b
    "IRUL4.HSQ",    // 0x6c
    "IRUL5.HSQ",    // 0x6d
    "IRUL6.HSQ",    // 0x6e
    "IRUL7.HSQ",    // 0x6f
    "IRUL8.HSQ",    // 0x70
    "IRUL9.HSQ",    // 0x71
    "DP1.HSQ",      // 0x72
    "DP0.HSQ",      // 0x73
    "DP2.HSQ",      // 0x74
    "DP3.HSQ",      // 0x75
    "DF1.HSQ",      // 0x76
    "DF2.HSQ",      // 0x77
    "DF3.HSQ",      // 0x78
    "DF4.HSQ",      // 0x79
    "VIL1.HSQ",     // 0x7a
    "VIL2.HSQ",     // 0x7b
    "VIL3.HSQ",     // 0x7c
    "VIL4.HSQ",     // 0x7d
    "VIL5.HSQ",     // 0x7e
    "VIL6.HSQ",     // 0x7f
    "DV1.HSQ",      // 0x80
    "DV2.HSQ",      // 0x81
    "DV3.HSQ",      // 0x82
    "DV4.HSQ",      // 0x83
    "DH0.HSQ",      // 0x84
    "DH1.HSQ",      // 0x85
    "DH2.HSQ",      // 0x86
    "DH3.HSQ",      // 0x87
    "VG01.HSQ",     // 0x88
    "VG02.HSQ",     // 0x89
    "VG03.HSQ",     // 0x8a
    "VG04.HSQ",     // 0x8b
    "VG05.HSQ",     // 0x8c
    "VG06.HSQ",     // 0x8d
    "VG07.HSQ",     // 0x8e
    "VG08.HSQ",     // 0x8f
    "VG09.HSQ",     // 0x90
    "VG10.HSQ",     // 0x91
];

// = seg000:f0b9 open_spritesheet_si_into_esdi — bank index -> filename.
fn bank_filename(index: u16) -> Option<&'static str> {
    BANK_NAMES.get(index as usize).copied()
}

/// The DOS active-bank globals: which resource is selected for drawing, the
/// per-index loaded-sheet cache, and the last palette applied.
pub struct Banks {
    // = seg001:2784 _word_21C34_active_bank_id — index of the selected bank.
    active_bank_id: u16,
    // = the 0d844 far-pointer table: one cached sheet per bank index (None ==
    // null far ptr == not yet loaded). Sized to BANK_NAMES.
    cache: Vec<Option<SpriteSheet>>,
}

impl Banks {
    pub fn new() -> Self {
        Self {
            active_bank_id: NO_BANK,
            cache: (0..BANK_NAMES.len()).map(|_| None).collect(),
        }
    }
}

impl Default for Banks {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    // Port helper
    pub fn with_active_bank_sheet(&mut self, f: impl FnOnce(&mut GameState, &SpriteSheet)) {
        let slot = self.banks.active_bank_id as usize;
        let Some(sheet) = self.banks.cache.get_mut(slot).and_then(Option::take) else {
            return;
        };
        f(self, &sheet);
        self.banks.cache[slot] = Some(sheet);
    }

    // Port helper
    pub fn with_bank(&mut self, index: i16, f: impl FnOnce(&mut GameState)) {
        let prev = self.open_sprite_bank(index);
        f(self);
        self.open_sprite_bank(prev as i16);
    }

    // = seg000:c137 open_icones_spritesheet.
    pub fn open_icones_spritesheet(&mut self) -> u16 {
        self.open_sprite_bank(ICONES)
    }

    // = seg000:c13b open_onmap_spritesheet.
    pub fn open_onmap_spritesheet(&mut self) -> u16 {
        self.open_sprite_bank(ONMAP)
    }

    // = seg000:c13e open_spritesheet.
    pub fn open_sprite_bank(&mut self, index: i16) -> u16 {
        if index < 0 {
            return self.banks.active_bank_id;
        }
        let idx = index as u16;
        // = seg000:c145 xchg bx,[2784h] — install new id, keep old.
        let prev = self.banks.active_bank_id;
        self.banks.active_bank_id = idx;
        // = seg000:c149 cmp ax,bx; jz — already selected.
        if idx == prev {
            return prev;
        }
        // = seg000:c15b les di,[0d844h+idx*4]; or es,es; jz -> load. Miss when
        // the cache slot is empty.
        let slot = idx as usize;
        let need_load = self.banks.cache.get(slot).is_none_or(Option::is_none);
        if need_load {
            // = seg000:c177 open_spritesheet_si_into_esdi: read the file.
            let Some(name) = bank_filename(idx) else {
                return prev; // unmapped index: nothing to load
            };
            let data = self
                .dat_file
                .read(name)
                .unwrap_or_else(|e| panic!("open_spritesheet({idx:#x}): read {name}: {e}"));
            let sheet = SpriteSheet::from_slice(&data)
                .unwrap_or_else(|e| panic!("open_spritesheet({idx:#x}): parse {name}: {e}"));
            self.banks.cache[slot] = Some(sheet);
        }
        // = seg000:c172 / c186 apply_sprite_sheet_palette.
        self.apply_sprite_sheet_palette(idx);
        prev
    }

    // = seg000:c1aa apply_sprite_sheet_palette.
    fn apply_sprite_sheet_palette(&mut self, idx: u16) {
        // Disjoint field borrows: &self.banks.cache + &mut self.palette.
        let Some(Some(sheet)) = self.banks.cache.get(idx as usize) else {
            return;
        };
        let _ = sheet.apply_palette_update(&mut self.palette);
    }

    // = seg000:c1ba
    pub(crate) fn apply_palette_update(&mut self, bytes: &[u8]) -> u16 {
        self.palette.apply_palette_update(bytes).unwrap() as u16
    }

    // = seg000:c202 sprite_center_coords
    fn sprite_center_coords(&self, sprite_id: u16, center_x: &mut i16, center_y: &mut i16) {
        todo!();
        // let Some(sheet) = self.bank.as_ref() else {
        //     return;
        // };

        // let Some(sprite) = sheet.get_sprite(sprite_id) else {
        //     return;
        // };

        // let width = sprite.width();
        // let height = sprite.height();

        // *center_x = center_x.saturating_sub_unsigned(width / 2);
        // *center_y = center_y.saturating_sub_unsigned(height / 2);
    }

    // = seg000:c22f draw_sprite_clobbering_bx_dx.
    pub fn draw_active_bank_sprite(&mut self, sprite_id: u16, x: i16, y: i16) {
        let slot = self.banks.active_bank_id as usize;
        let physical_y = y + self.y_offset as i16;
        // Hold &self.banks.cache and &mut <one framebuffer field> at once: these
        // are disjoint GameState fields. Do NOT route through active_fb_mut() —
        // that reborrows all of `self` and would conflict with the cache borrow.
        let Some(Some(sheet)) = self.banks.cache.get(slot) else {
            return;
        };
        let fb = match self.active_fb {
            FbId::Screen => &mut self.screen,
            FbId::Fb1 => &mut self.framebuffer,
            FbId::Saved => &mut self.framebuffer_saved,
        };
        let _ = draw_sprite_from_sheet(sheet, sprite_id, x, physical_y, fb);
    }

    // = seg000:c2f2 open_resource_and_draw_sprite0.
    pub fn open_resource_and_draw_sprite0(&mut self, index: i16) {
        // = seg000:c2f4 xor ah,ah; call open_spritesheet.
        self.open_sprite_bank(index);
        // = seg000:c2f7 ax=0 (sprite 0); bx=0 (y); dx=0 (x); draw_sprite.
        self.draw_active_bank_sprite(0, 0, 0);
    }
}
