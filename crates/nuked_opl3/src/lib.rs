//! Pure-Rust port of Nuked-OPL3 1.8, a cycle-accurate Yamaha YMF262 (OPL3)
//! FM synthesizer emulator.
//!
//! Nuked OPL3
//! Copyright (C) 2013-2020 Nuke.YKT
//! <https://github.com/nukeykt/Nuked-OPL3>
//!
//! The emulation logic and tables in this file are Nuke.YKT's work; this
//! crate is a translation of their C source to Rust.
//!
//! Nuked OPL3 is free software: you can redistribute it and/or modify
//! it under the terms of the GNU Lesser General Public License as
//! published by the Free Software Foundation, either version 2.1
//! of the License, or (at your option) any later version.
//!
//! Nuked OPL3 is distributed in the hope that it will be useful,
//! but WITHOUT ANY WARRANTY; without even the implied warranty of
//! MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//! GNU Lesser General Public License for more details.
//!
//! You should have received a copy of the GNU Lesser General Public License
//! along with Nuked OPL3. If not, see <https://www.gnu.org/licenses/>.
//!
//! Thanks:
//! - MAME Development Team (Jarek Burczynski, Tatsuyuki Satoh):
//!   Feedback and Rhythm part calculation information.
//! - forums.submarine.org.uk (carbon14, opl3):
//!   Tremolo and phase generator calculation information.
//! - OPLx decapsulated (Matthew Gambrell, Olli Niemitalo): OPL2 ROMs.
//! - siliconpr0n.org (John McMaster, digshadow):
//!   YMF262 and VRC VII decaps and die shots.
//!
//! Every routine carries a `// = OPL3_...` comment naming the C function it
//! mirrors in `opl3.c`. The C code links slots and channels with raw
//! pointers; this port stores indices into the chip's `slot` and `channel`
//! arrays instead, and a [`ModSource`] where the C code pointed at one of a
//! few `i16` fields. The stereo-extension build option
//! (`OPL_ENABLE_STEREOEXT`) is not ported; it is off by default in C, which
//! also turns on the `OPL_QUIRK_CHANNELSAMPLEDELAY` behaviour ported here.

/// Size of the delayed register-write ring buffer (`OPL_WRITEBUF_SIZE`).
pub const OPL_WRITEBUF_SIZE: usize = 1024;
/// Minimum spacing, in chip samples, between two buffered writes
/// (`OPL_WRITEBUF_DELAY`).
pub const OPL_WRITEBUF_DELAY: u64 = 2;

/// Native sample rate of the YMF262.
pub const OPL_NATIVE_RATE: u32 = 49716;

const RSM_FRAC: u32 = 10;

/// Channel types.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChType {
    /// `ch_2op`
    Op2,
    /// `ch_4op`: the first channel of a 4-op pair.
    Op4,
    /// `ch_4op2`: the second channel of a 4-op pair.
    Op4Second,
    /// `ch_drum`
    Drum,
}

/// Envelope key types.
const EGK_NORM: u8 = 0x01;
const EGK_DRUM: u8 = 0x02;

/// Envelope generator stages (`envelope_gen_num`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EgGen {
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Where a slot's modulation input or a channel's output tap reads from.
/// Replaces the C `int16_t *` pointers into `zeromod`, `slot->out`, and
/// `slot->fbmod`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ModSource {
    /// `&chip->zeromod`: always reads as zero.
    Zero,
    /// `&chip->slot[n].out`
    SlotOut(usize),
    /// `&chip->slot[n].fbmod`
    SlotFbmod(usize),
}

/// logsin table
#[rustfmt::skip]
static LOGSINROM: [u16; 256] = [
    0x859, 0x6c3, 0x607, 0x58b, 0x52e, 0x4e4, 0x4a6, 0x471,
    0x443, 0x41a, 0x3f5, 0x3d3, 0x3b5, 0x398, 0x37e, 0x365,
    0x34e, 0x339, 0x324, 0x311, 0x2ff, 0x2ed, 0x2dc, 0x2cd,
    0x2bd, 0x2af, 0x2a0, 0x293, 0x286, 0x279, 0x26d, 0x261,
    0x256, 0x24b, 0x240, 0x236, 0x22c, 0x222, 0x218, 0x20f,
    0x206, 0x1fd, 0x1f5, 0x1ec, 0x1e4, 0x1dc, 0x1d4, 0x1cd,
    0x1c5, 0x1be, 0x1b7, 0x1b0, 0x1a9, 0x1a2, 0x19b, 0x195,
    0x18f, 0x188, 0x182, 0x17c, 0x177, 0x171, 0x16b, 0x166,
    0x160, 0x15b, 0x155, 0x150, 0x14b, 0x146, 0x141, 0x13c,
    0x137, 0x133, 0x12e, 0x129, 0x125, 0x121, 0x11c, 0x118,
    0x114, 0x10f, 0x10b, 0x107, 0x103, 0x0ff, 0x0fb, 0x0f8,
    0x0f4, 0x0f0, 0x0ec, 0x0e9, 0x0e5, 0x0e2, 0x0de, 0x0db,
    0x0d7, 0x0d4, 0x0d1, 0x0cd, 0x0ca, 0x0c7, 0x0c4, 0x0c1,
    0x0be, 0x0bb, 0x0b8, 0x0b5, 0x0b2, 0x0af, 0x0ac, 0x0a9,
    0x0a7, 0x0a4, 0x0a1, 0x09f, 0x09c, 0x099, 0x097, 0x094,
    0x092, 0x08f, 0x08d, 0x08a, 0x088, 0x086, 0x083, 0x081,
    0x07f, 0x07d, 0x07a, 0x078, 0x076, 0x074, 0x072, 0x070,
    0x06e, 0x06c, 0x06a, 0x068, 0x066, 0x064, 0x062, 0x060,
    0x05e, 0x05c, 0x05b, 0x059, 0x057, 0x055, 0x053, 0x052,
    0x050, 0x04e, 0x04d, 0x04b, 0x04a, 0x048, 0x046, 0x045,
    0x043, 0x042, 0x040, 0x03f, 0x03e, 0x03c, 0x03b, 0x039,
    0x038, 0x037, 0x035, 0x034, 0x033, 0x031, 0x030, 0x02f,
    0x02e, 0x02d, 0x02b, 0x02a, 0x029, 0x028, 0x027, 0x026,
    0x025, 0x024, 0x023, 0x022, 0x021, 0x020, 0x01f, 0x01e,
    0x01d, 0x01c, 0x01b, 0x01a, 0x019, 0x018, 0x017, 0x017,
    0x016, 0x015, 0x014, 0x014, 0x013, 0x012, 0x011, 0x011,
    0x010, 0x00f, 0x00f, 0x00e, 0x00d, 0x00d, 0x00c, 0x00c,
    0x00b, 0x00a, 0x00a, 0x009, 0x009, 0x008, 0x008, 0x007,
    0x007, 0x007, 0x006, 0x006, 0x005, 0x005, 0x005, 0x004,
    0x004, 0x004, 0x003, 0x003, 0x003, 0x002, 0x002, 0x002,
    0x002, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001,
    0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000,
];

/// exp table
#[rustfmt::skip]
static EXPROM: [u16; 256] = [
    0x7fa, 0x7f5, 0x7ef, 0x7ea, 0x7e4, 0x7df, 0x7da, 0x7d4,
    0x7cf, 0x7c9, 0x7c4, 0x7bf, 0x7b9, 0x7b4, 0x7ae, 0x7a9,
    0x7a4, 0x79f, 0x799, 0x794, 0x78f, 0x78a, 0x784, 0x77f,
    0x77a, 0x775, 0x770, 0x76a, 0x765, 0x760, 0x75b, 0x756,
    0x751, 0x74c, 0x747, 0x742, 0x73d, 0x738, 0x733, 0x72e,
    0x729, 0x724, 0x71f, 0x71a, 0x715, 0x710, 0x70b, 0x706,
    0x702, 0x6fd, 0x6f8, 0x6f3, 0x6ee, 0x6e9, 0x6e5, 0x6e0,
    0x6db, 0x6d6, 0x6d2, 0x6cd, 0x6c8, 0x6c4, 0x6bf, 0x6ba,
    0x6b5, 0x6b1, 0x6ac, 0x6a8, 0x6a3, 0x69e, 0x69a, 0x695,
    0x691, 0x68c, 0x688, 0x683, 0x67f, 0x67a, 0x676, 0x671,
    0x66d, 0x668, 0x664, 0x65f, 0x65b, 0x657, 0x652, 0x64e,
    0x649, 0x645, 0x641, 0x63c, 0x638, 0x634, 0x630, 0x62b,
    0x627, 0x623, 0x61e, 0x61a, 0x616, 0x612, 0x60e, 0x609,
    0x605, 0x601, 0x5fd, 0x5f9, 0x5f5, 0x5f0, 0x5ec, 0x5e8,
    0x5e4, 0x5e0, 0x5dc, 0x5d8, 0x5d4, 0x5d0, 0x5cc, 0x5c8,
    0x5c4, 0x5c0, 0x5bc, 0x5b8, 0x5b4, 0x5b0, 0x5ac, 0x5a8,
    0x5a4, 0x5a0, 0x59c, 0x599, 0x595, 0x591, 0x58d, 0x589,
    0x585, 0x581, 0x57e, 0x57a, 0x576, 0x572, 0x56f, 0x56b,
    0x567, 0x563, 0x560, 0x55c, 0x558, 0x554, 0x551, 0x54d,
    0x549, 0x546, 0x542, 0x53e, 0x53b, 0x537, 0x534, 0x530,
    0x52c, 0x529, 0x525, 0x522, 0x51e, 0x51b, 0x517, 0x514,
    0x510, 0x50c, 0x509, 0x506, 0x502, 0x4ff, 0x4fb, 0x4f8,
    0x4f4, 0x4f1, 0x4ed, 0x4ea, 0x4e7, 0x4e3, 0x4e0, 0x4dc,
    0x4d9, 0x4d6, 0x4d2, 0x4cf, 0x4cc, 0x4c8, 0x4c5, 0x4c2,
    0x4be, 0x4bb, 0x4b8, 0x4b5, 0x4b1, 0x4ae, 0x4ab, 0x4a8,
    0x4a4, 0x4a1, 0x49e, 0x49b, 0x498, 0x494, 0x491, 0x48e,
    0x48b, 0x488, 0x485, 0x482, 0x47e, 0x47b, 0x478, 0x475,
    0x472, 0x46f, 0x46c, 0x469, 0x466, 0x463, 0x460, 0x45d,
    0x45a, 0x457, 0x454, 0x451, 0x44e, 0x44b, 0x448, 0x445,
    0x442, 0x43f, 0x43c, 0x439, 0x436, 0x433, 0x430, 0x42d,
    0x42a, 0x428, 0x425, 0x422, 0x41f, 0x41c, 0x419, 0x416,
    0x414, 0x411, 0x40e, 0x40b, 0x408, 0x406, 0x403, 0x400,
];

/// freq mult table multiplied by 2
///
/// 1/2, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10, 12, 12, 15, 15
static MT: [u8; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

/// ksl table
static KSLROM: [u8; 16] = [
    0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64,
];

static KSLSHIFT: [u8; 4] = [8, 1, 2, 0];

/// envelope generator constants
static EG_INCSTEP: [[u8; 4]; 4] = [[0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 1, 0], [1, 1, 1, 0]];

/// address decoding
#[rustfmt::skip]
static AD_SLOT: [i8; 0x20] = [
    0, 1, 2, 3, 4, 5, -1, -1, 6, 7, 8, 9, 10, 11, -1, -1,
    12, 13, 14, 15, 16, 17, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
];

static CH_SLOT: [usize; 18] = [
    0, 1, 2, 6, 7, 8, 12, 13, 14, 18, 19, 20, 24, 25, 26, 30, 31, 32,
];

// = opl3_slot
#[derive(Clone, Copy, Debug)]
struct Slot {
    /// Index of the owning channel (`slot->channel`).
    channel: usize,
    out: i16,
    fbmod: i16,
    /// `slot->mod`
    mod_src: ModSource,
    prout: i16,
    eg_rout: u16,
    eg_out: u16,
    eg_gen: EgGen,
    eg_ksl: u8,
    /// `slot->trem`: `true` when it points at `chip->tremolo`, `false` for
    /// `zeromod`.
    trem: bool,
    reg_vib: u8,
    reg_type: u8,
    reg_ksr: u8,
    reg_mult: u8,
    reg_ksl: u8,
    reg_tl: u8,
    reg_ar: u8,
    reg_dr: u8,
    reg_sl: u8,
    reg_rr: u8,
    reg_wf: u8,
    key: u8,
    pg_reset: bool,
    pg_phase: u32,
    pg_phase_out: u16,
    slot_num: usize,
}

impl Slot {
    const fn new(slot_num: usize) -> Self {
        Slot {
            channel: 0,
            out: 0,
            fbmod: 0,
            mod_src: ModSource::Zero,
            prout: 0,
            eg_rout: 0x1ff,
            eg_out: 0x1ff,
            eg_gen: EgGen::Release,
            eg_ksl: 0,
            trem: false,
            reg_vib: 0,
            reg_type: 0,
            reg_ksr: 0,
            reg_mult: 0,
            reg_ksl: 0,
            reg_tl: 0,
            reg_ar: 0,
            reg_dr: 0,
            reg_sl: 0,
            reg_rr: 0,
            reg_wf: 0,
            key: 0,
            pg_reset: false,
            pg_phase: 0,
            pg_phase_out: 0,
            slot_num,
        }
    }
}

// = opl3_channel
#[derive(Clone, Copy, Debug)]
struct Channel {
    /// Slot indices (`channel->slotz`).
    slots: [usize; 2],
    /// Index of the 4-op partner channel (`channel->pair`). Channels 6..=8
    /// and 15..=17 have no partner; the C code leaves the pointer NULL and
    /// never reads it, so the port stores the channel's own index there.
    pair: usize,
    /// `channel->out`
    out: [ModSource; 4],
    chtype: ChType,
    f_num: u16,
    block: u8,
    fb: u8,
    con: u8,
    alg: u8,
    ksv: u8,
    cha: u16,
    chb: u16,
    chc: u16,
    chd: u16,
    ch_num: usize,
}

impl Channel {
    const fn new(ch_num: usize) -> Self {
        Channel {
            slots: [0, 0],
            pair: ch_num,
            out: [ModSource::Zero; 4],
            chtype: ChType::Op2,
            f_num: 0,
            block: 0,
            fb: 0,
            con: 0,
            alg: 0,
            ksv: 0,
            cha: 0,
            chb: 0,
            chc: 0,
            chd: 0,
            ch_num,
        }
    }
}

// = opl3_writebuf
#[derive(Clone, Copy, Debug, Default)]
struct WriteBuf {
    time: u64,
    reg: u16,
    data: u8,
}

// = opl3_chip
/// One emulated YMF262. Create it with [`Opl3Chip::new`], drive it with
/// [`Opl3Chip::write_reg`] / [`Opl3Chip::write_reg_buffered`], and pull
/// samples with the `generate_*` methods.
#[derive(Clone, Debug)]
pub struct Opl3Chip {
    channel: [Channel; 18],
    slot: [Slot; 36],
    timer: u16,
    eg_timer: u64,
    eg_timerrem: u8,
    eg_state: u8,
    eg_add: u8,
    eg_timer_lo: u8,
    newm: u8,
    nts: u8,
    rhy: u8,
    vibpos: u8,
    vibshift: u8,
    tremolo: u8,
    tremolopos: u8,
    tremoloshift: u8,
    noise: u32,
    mixbuff: [i32; 4],
    rm_hh_bit2: u8,
    rm_hh_bit3: u8,
    rm_hh_bit7: u8,
    rm_hh_bit8: u8,
    rm_tc_bit3: u8,
    rm_tc_bit5: u8,

    /* OPL3L */
    rateratio: i32,
    samplecnt: i32,
    oldsamples: [i16; 4],
    samples: [i16; 4],

    writebuf_samplecnt: u64,
    writebuf_cur: usize,
    writebuf_last: usize,
    writebuf_lasttime: u64,
    writebuf: [WriteBuf; OPL_WRITEBUF_SIZE],
}

/*
    Envelope generator
*/

// = OPL3_EnvelopeCalcExp
fn envelope_calc_exp(level: u32) -> i16 {
    let level = level.min(0x1fff);
    (((EXPROM[(level & 0xff) as usize] as i32) << 1) >> (level >> 8)) as i16
}

// = OPL3_EnvelopeCalcSin0
fn envelope_calc_sin0(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let neg: u16 = if phase & 0x200 != 0 { 0xffff } else { 0 };
    let out = if phase & 0x100 != 0 {
        LOGSINROM[((phase & 0xff) ^ 0xff) as usize]
    } else {
        LOGSINROM[(phase & 0xff) as usize]
    };
    (envelope_calc_exp(out as u32 + ((envelope as u32) << 3)) as u16 ^ neg) as i16
}

// = OPL3_EnvelopeCalcSin1
fn envelope_calc_sin1(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let out = if phase & 0x200 != 0 {
        0x1000
    } else if phase & 0x100 != 0 {
        LOGSINROM[((phase & 0xff) ^ 0xff) as usize]
    } else {
        LOGSINROM[(phase & 0xff) as usize]
    };
    envelope_calc_exp(out as u32 + ((envelope as u32) << 3))
}

// = OPL3_EnvelopeCalcSin2
fn envelope_calc_sin2(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let out = if phase & 0x100 != 0 {
        LOGSINROM[((phase & 0xff) ^ 0xff) as usize]
    } else {
        LOGSINROM[(phase & 0xff) as usize]
    };
    envelope_calc_exp(out as u32 + ((envelope as u32) << 3))
}

// = OPL3_EnvelopeCalcSin3
fn envelope_calc_sin3(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let out = if phase & 0x100 != 0 {
        0x1000
    } else {
        LOGSINROM[(phase & 0xff) as usize]
    };
    envelope_calc_exp(out as u32 + ((envelope as u32) << 3))
}

// = OPL3_EnvelopeCalcSin4
fn envelope_calc_sin4(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let neg: u16 = if phase & 0x300 == 0x100 { 0xffff } else { 0 };
    let out = if phase & 0x200 != 0 {
        0x1000
    } else if phase & 0x80 != 0 {
        LOGSINROM[(((phase ^ 0xff) << 1) & 0xff) as usize]
    } else {
        LOGSINROM[((phase << 1) & 0xff) as usize]
    };
    (envelope_calc_exp(out as u32 + ((envelope as u32) << 3)) as u16 ^ neg) as i16
}

// = OPL3_EnvelopeCalcSin5
fn envelope_calc_sin5(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let out = if phase & 0x200 != 0 {
        0x1000
    } else if phase & 0x80 != 0 {
        LOGSINROM[(((phase ^ 0xff) << 1) & 0xff) as usize]
    } else {
        LOGSINROM[((phase << 1) & 0xff) as usize]
    };
    envelope_calc_exp(out as u32 + ((envelope as u32) << 3))
}

// = OPL3_EnvelopeCalcSin6
fn envelope_calc_sin6(phase: u16, envelope: u16) -> i16 {
    let phase = phase & 0x3ff;
    let neg: u16 = if phase & 0x200 != 0 { 0xffff } else { 0 };
    (envelope_calc_exp((envelope as u32) << 3) as u16 ^ neg) as i16
}

// = OPL3_EnvelopeCalcSin7
fn envelope_calc_sin7(phase: u16, envelope: u16) -> i16 {
    let mut phase = phase & 0x3ff;
    let mut neg: u16 = 0;
    if phase & 0x200 != 0 {
        neg = 0xffff;
        phase = (phase & 0x1ff) ^ 0x1ff;
    }
    let out = phase << 3;
    (envelope_calc_exp(out as u32 + ((envelope as u32) << 3)) as u16 ^ neg) as i16
}

// = envelope_sin
static ENVELOPE_SIN: [fn(u16, u16) -> i16; 8] = [
    envelope_calc_sin0,
    envelope_calc_sin1,
    envelope_calc_sin2,
    envelope_calc_sin3,
    envelope_calc_sin4,
    envelope_calc_sin5,
    envelope_calc_sin6,
    envelope_calc_sin7,
];

// = OPL3_ClipSample
fn clip_sample(sample: i32) -> i16 {
    sample.clamp(-32768, 32767) as i16
}

impl Opl3Chip {
    // = OPL3_Reset — build a chip in its just-reset state for the given
    // output sample rate.
    pub fn new(samplerate: u32) -> Self {
        let mut chip = Opl3Chip {
            channel: [Channel::new(0); 18],
            slot: [Slot::new(0); 36],
            timer: 0,
            eg_timer: 0,
            eg_timerrem: 0,
            eg_state: 0,
            eg_add: 0,
            eg_timer_lo: 0,
            newm: 0,
            nts: 0,
            rhy: 0,
            vibpos: 0,
            vibshift: 0,
            tremolo: 0,
            tremolopos: 0,
            tremoloshift: 0,
            noise: 0,
            mixbuff: [0; 4],
            rm_hh_bit2: 0,
            rm_hh_bit3: 0,
            rm_hh_bit7: 0,
            rm_hh_bit8: 0,
            rm_tc_bit3: 0,
            rm_tc_bit5: 0,
            rateratio: 0,
            samplecnt: 0,
            oldsamples: [0; 4],
            samples: [0; 4],
            writebuf_samplecnt: 0,
            writebuf_cur: 0,
            writebuf_last: 0,
            writebuf_lasttime: 0,
            writebuf: [WriteBuf::default(); OPL_WRITEBUF_SIZE],
        };
        for slotnum in 0..36 {
            chip.slot[slotnum] = Slot::new(slotnum);
        }
        for (channum, &local_ch_slot) in CH_SLOT.iter().enumerate() {
            let mut channel = Channel::new(channum);
            channel.slots = [local_ch_slot, local_ch_slot + 3];
            chip.slot[local_ch_slot].channel = channum;
            chip.slot[local_ch_slot + 3].channel = channum;
            if (channum % 9) < 3 {
                channel.pair = channum + 3;
            } else if (channum % 9) < 6 {
                channel.pair = channum - 3;
            }
            channel.cha = 0xffff;
            channel.chb = 0xffff;
            chip.channel[channum] = channel;
            chip.channel_setup_alg(channum);
        }
        chip.noise = 1;
        chip.rateratio = ((samplerate << RSM_FRAC) / OPL_NATIVE_RATE) as i32;
        chip.tremoloshift = 4;
        chip.vibshift = 1;
        chip
    }

    // = OPL3_Reset
    pub fn reset(&mut self, samplerate: u32) {
        *self = Self::new(samplerate);
    }

    /// Read the `i16` a [`ModSource`] points at.
    #[inline]
    fn read(&self, src: ModSource) -> i16 {
        match src {
            ModSource::Zero => 0,
            ModSource::SlotOut(i) => self.slot[i].out,
            ModSource::SlotFbmod(i) => self.slot[i].fbmod,
        }
    }

    // = OPL3_EnvelopeUpdateKSL
    fn envelope_update_ksl(&mut self, s: usize) {
        let channel = &self.channel[self.slot[s].channel];
        let ksl = ((KSLROM[(channel.f_num >> 6) as usize] as i32) << 2)
            - ((0x08 - channel.block as i32) << 5);
        self.slot[s].eg_ksl = ksl.max(0) as u8;
    }

    // = OPL3_EnvelopeCalc
    fn envelope_calc(&mut self, s: usize) {
        let tremolo = self.tremolo;
        let eg_add = self.eg_add;
        let eg_state = self.eg_state;
        let eg_timer_lo = self.eg_timer_lo;
        let ksv = self.channel[self.slot[s].channel].ksv;
        let slot = &mut self.slot[s];

        let trem = if slot.trem { tremolo as u16 } else { 0 };
        slot.eg_out = slot.eg_rout
            + ((slot.reg_tl as u16) << 2)
            + ((slot.eg_ksl as u16) >> KSLSHIFT[slot.reg_ksl as usize])
            + trem;
        let mut reg_rate: u8 = 0;
        let mut reset = false;
        if slot.key != 0 && slot.eg_gen == EgGen::Release {
            reset = true;
            reg_rate = slot.reg_ar;
        } else {
            match slot.eg_gen {
                EgGen::Attack => reg_rate = slot.reg_ar,
                EgGen::Decay => reg_rate = slot.reg_dr,
                EgGen::Sustain => {
                    if slot.reg_type == 0 {
                        reg_rate = slot.reg_rr;
                    }
                }
                EgGen::Release => reg_rate = slot.reg_rr,
            }
        }
        slot.pg_reset = reset;
        let ks = ksv >> ((slot.reg_ksr ^ 1) << 1);
        let nonzero = reg_rate != 0;
        let rate = ks + (reg_rate << 2);
        let mut rate_hi = rate >> 2;
        let rate_lo = rate & 0x03;
        if rate_hi & 0x10 != 0 {
            rate_hi = 0x0f;
        }
        let eg_shift = rate_hi + eg_add;
        let mut shift: u8 = 0;
        if nonzero {
            if rate_hi < 12 {
                if eg_state != 0 {
                    match eg_shift {
                        12 => shift = 1,
                        13 => shift = (rate_lo >> 1) & 0x01,
                        14 => shift = rate_lo & 0x01,
                        _ => {}
                    }
                }
            } else {
                shift = (rate_hi & 0x03) + EG_INCSTEP[rate_lo as usize][eg_timer_lo as usize];
                if shift & 0x04 != 0 {
                    shift = 0x03;
                }
                if shift == 0 {
                    shift = eg_state;
                }
            }
        }
        let mut eg_rout = slot.eg_rout;
        let mut eg_inc: i16 = 0;
        let mut eg_off = false;
        /* Instant attack */
        if reset && rate_hi == 0x0f {
            eg_rout = 0x00;
        }
        /* Envelope off */
        if (slot.eg_rout & 0x1f8) == 0x1f8 {
            eg_off = true;
        }
        if slot.eg_gen != EgGen::Attack && !reset && eg_off {
            eg_rout = 0x1ff;
        }
        match slot.eg_gen {
            EgGen::Attack => {
                if slot.eg_rout == 0 {
                    slot.eg_gen = EgGen::Decay;
                } else if slot.key != 0 && shift > 0 && rate_hi != 0x0f {
                    eg_inc = ((!(slot.eg_rout as i32)) >> (4 - shift)) as i16;
                }
            }
            EgGen::Decay => {
                if (slot.eg_rout >> 4) == slot.reg_sl as u16 {
                    slot.eg_gen = EgGen::Sustain;
                } else if !eg_off && !reset && shift > 0 {
                    eg_inc = 1 << (shift - 1);
                }
            }
            EgGen::Sustain | EgGen::Release => {
                if !eg_off && !reset && shift > 0 {
                    eg_inc = 1 << (shift - 1);
                }
            }
        }
        slot.eg_rout = ((eg_rout as i32 + eg_inc as i32) & 0x1ff) as u16;
        /* Key off */
        if reset {
            slot.eg_gen = EgGen::Attack;
        }
        if slot.key == 0 {
            slot.eg_gen = EgGen::Release;
        }
    }

    // = OPL3_EnvelopeKeyOn
    fn envelope_key_on(&mut self, s: usize, kind: u8) {
        self.slot[s].key |= kind;
    }

    // = OPL3_EnvelopeKeyOff
    fn envelope_key_off(&mut self, s: usize, kind: u8) {
        self.slot[s].key &= !kind;
    }

    /*
        Phase Generator
    */

    // = OPL3_PhaseGenerate
    fn phase_generate(&mut self, s: usize) {
        let channel = self.channel[self.slot[s].channel];
        let mut f_num = channel.f_num;
        if self.slot[s].reg_vib != 0 {
            let mut range = ((f_num >> 7) & 7) as i8;
            let vibpos = self.vibpos;

            if vibpos & 3 == 0 {
                range = 0;
            } else if vibpos & 1 != 0 {
                range >>= 1;
            }
            range >>= self.vibshift;

            if vibpos & 4 != 0 {
                range = -range;
            }
            f_num = f_num.wrapping_add(range as u16);
        }
        let basefreq = ((f_num as u32) << channel.block) >> 1;
        let phase = (self.slot[s].pg_phase >> 9) as u16;
        if self.slot[s].pg_reset {
            self.slot[s].pg_phase = 0;
        }
        self.slot[s].pg_phase = self.slot[s]
            .pg_phase
            .wrapping_add((basefreq * MT[self.slot[s].reg_mult as usize] as u32) >> 1);
        /* Rhythm mode */
        let noise = self.noise;
        self.slot[s].pg_phase_out = phase;
        let slot_num = self.slot[s].slot_num;
        if slot_num == 13 {
            /* hh */
            self.rm_hh_bit2 = ((phase >> 2) & 1) as u8;
            self.rm_hh_bit3 = ((phase >> 3) & 1) as u8;
            self.rm_hh_bit7 = ((phase >> 7) & 1) as u8;
            self.rm_hh_bit8 = ((phase >> 8) & 1) as u8;
        }
        if slot_num == 17 && (self.rhy & 0x20) != 0 {
            /* tc */
            self.rm_tc_bit3 = ((phase >> 3) & 1) as u8;
            self.rm_tc_bit5 = ((phase >> 5) & 1) as u8;
        }
        if self.rhy & 0x20 != 0 {
            let rm_xor = (self.rm_hh_bit2 ^ self.rm_hh_bit7)
                | (self.rm_hh_bit3 ^ self.rm_tc_bit5)
                | (self.rm_tc_bit3 ^ self.rm_tc_bit5);
            match slot_num {
                13 => {
                    /* hh */
                    let mut out = (rm_xor as u16) << 9;
                    if (rm_xor ^ (noise & 1) as u8) != 0 {
                        out |= 0xd0;
                    } else {
                        out |= 0x34;
                    }
                    self.slot[s].pg_phase_out = out;
                }
                16 => {
                    /* sd */
                    self.slot[s].pg_phase_out = ((self.rm_hh_bit8 as u16) << 9)
                        | (((self.rm_hh_bit8 ^ (noise & 1) as u8) as u16) << 8);
                }
                17 => {
                    /* tc */
                    self.slot[s].pg_phase_out = ((rm_xor as u16) << 9) | 0x80;
                }
                _ => {}
            }
        }
        let n_bit = ((noise >> 14) ^ noise) & 0x01;
        self.noise = (noise >> 1) | (n_bit << 22);
    }

    /*
        Slot
    */

    // = OPL3_SlotWrite20
    fn slot_write_20(&mut self, s: usize, data: u8) {
        let slot = &mut self.slot[s];
        slot.trem = (data >> 7) & 0x01 != 0;
        slot.reg_vib = (data >> 6) & 0x01;
        slot.reg_type = (data >> 5) & 0x01;
        slot.reg_ksr = (data >> 4) & 0x01;
        slot.reg_mult = data & 0x0f;
    }

    // = OPL3_SlotWrite40
    fn slot_write_40(&mut self, s: usize, data: u8) {
        self.slot[s].reg_ksl = (data >> 6) & 0x03;
        self.slot[s].reg_tl = data & 0x3f;
        self.envelope_update_ksl(s);
    }

    // = OPL3_SlotWrite60
    fn slot_write_60(&mut self, s: usize, data: u8) {
        self.slot[s].reg_ar = (data >> 4) & 0x0f;
        self.slot[s].reg_dr = data & 0x0f;
    }

    // = OPL3_SlotWrite80
    fn slot_write_80(&mut self, s: usize, data: u8) {
        let slot = &mut self.slot[s];
        slot.reg_sl = (data >> 4) & 0x0f;
        if slot.reg_sl == 0x0f {
            slot.reg_sl = 0x1f;
        }
        slot.reg_rr = data & 0x0f;
    }

    // = OPL3_SlotWriteE0
    fn slot_write_e0(&mut self, s: usize, data: u8) {
        self.slot[s].reg_wf = data & 0x07;
        if self.newm == 0x00 {
            self.slot[s].reg_wf &= 0x03;
        }
    }

    // = OPL3_SlotGenerate
    fn slot_generate(&mut self, s: usize) {
        let modulation = self.read(self.slot[s].mod_src);
        let slot = &mut self.slot[s];
        slot.out = ENVELOPE_SIN[slot.reg_wf as usize](
            slot.pg_phase_out.wrapping_add(modulation as u16),
            slot.eg_out,
        );
    }

    // = OPL3_SlotCalcFB
    fn slot_calc_fb(&mut self, s: usize) {
        let fb = self.channel[self.slot[s].channel].fb;
        let slot = &mut self.slot[s];
        if fb != 0x00 {
            slot.fbmod = ((slot.prout as i32 + slot.out as i32) >> (0x09 - fb)) as i16;
        } else {
            slot.fbmod = 0;
        }
        slot.prout = slot.out;
    }

    /*
        Channel
    */

    // = OPL3_ChannelUpdateRhythm
    fn channel_update_rhythm(&mut self, data: u8) {
        self.rhy = data & 0x3f;
        if self.rhy & 0x20 != 0 {
            let ch6 = self.channel[6].slots;
            let ch7 = self.channel[7].slots;
            let ch8 = self.channel[8].slots;
            self.channel[6].out = [
                ModSource::SlotOut(ch6[1]),
                ModSource::SlotOut(ch6[1]),
                ModSource::Zero,
                ModSource::Zero,
            ];
            self.channel[7].out = [
                ModSource::SlotOut(ch7[0]),
                ModSource::SlotOut(ch7[0]),
                ModSource::SlotOut(ch7[1]),
                ModSource::SlotOut(ch7[1]),
            ];
            self.channel[8].out = [
                ModSource::SlotOut(ch8[0]),
                ModSource::SlotOut(ch8[0]),
                ModSource::SlotOut(ch8[1]),
                ModSource::SlotOut(ch8[1]),
            ];
            for chnum in 6..9 {
                self.channel[chnum].chtype = ChType::Drum;
            }
            self.channel_setup_alg(6);
            self.channel_setup_alg(7);
            self.channel_setup_alg(8);
            /* hh */
            if self.rhy & 0x01 != 0 {
                self.envelope_key_on(ch7[0], EGK_DRUM);
            } else {
                self.envelope_key_off(ch7[0], EGK_DRUM);
            }
            /* tc */
            if self.rhy & 0x02 != 0 {
                self.envelope_key_on(ch8[1], EGK_DRUM);
            } else {
                self.envelope_key_off(ch8[1], EGK_DRUM);
            }
            /* tom */
            if self.rhy & 0x04 != 0 {
                self.envelope_key_on(ch8[0], EGK_DRUM);
            } else {
                self.envelope_key_off(ch8[0], EGK_DRUM);
            }
            /* sd */
            if self.rhy & 0x08 != 0 {
                self.envelope_key_on(ch7[1], EGK_DRUM);
            } else {
                self.envelope_key_off(ch7[1], EGK_DRUM);
            }
            /* bd */
            if self.rhy & 0x10 != 0 {
                self.envelope_key_on(ch6[0], EGK_DRUM);
                self.envelope_key_on(ch6[1], EGK_DRUM);
            } else {
                self.envelope_key_off(ch6[0], EGK_DRUM);
                self.envelope_key_off(ch6[1], EGK_DRUM);
            }
        } else {
            for chnum in 6..9 {
                self.channel[chnum].chtype = ChType::Op2;
                self.channel_setup_alg(chnum);
                let slots = self.channel[chnum].slots;
                self.envelope_key_off(slots[0], EGK_DRUM);
                self.envelope_key_off(slots[1], EGK_DRUM);
            }
        }
    }

    // = OPL3_ChannelWriteA0
    fn channel_write_a0(&mut self, c: usize, data: u8) {
        if self.newm != 0 && self.channel[c].chtype == ChType::Op4Second {
            return;
        }
        let nts = self.nts;
        let channel = &mut self.channel[c];
        channel.f_num = (channel.f_num & 0x300) | data as u16;
        channel.ksv = (channel.block << 1) | ((channel.f_num >> (0x09 - nts)) & 0x01) as u8;
        let slots = channel.slots;
        self.envelope_update_ksl(slots[0]);
        self.envelope_update_ksl(slots[1]);
        if self.newm != 0 && self.channel[c].chtype == ChType::Op4 {
            let pair = self.channel[c].pair;
            self.channel[pair].f_num = self.channel[c].f_num;
            self.channel[pair].ksv = self.channel[c].ksv;
            let pair_slots = self.channel[pair].slots;
            self.envelope_update_ksl(pair_slots[0]);
            self.envelope_update_ksl(pair_slots[1]);
        }
    }

    // = OPL3_ChannelWriteB0
    fn channel_write_b0(&mut self, c: usize, data: u8) {
        if self.newm != 0 && self.channel[c].chtype == ChType::Op4Second {
            return;
        }
        let nts = self.nts;
        let channel = &mut self.channel[c];
        channel.f_num = (channel.f_num & 0xff) | (((data & 0x03) as u16) << 8);
        channel.block = (data >> 2) & 0x07;
        channel.ksv = (channel.block << 1) | ((channel.f_num >> (0x09 - nts)) & 0x01) as u8;
        let slots = channel.slots;
        self.envelope_update_ksl(slots[0]);
        self.envelope_update_ksl(slots[1]);
        if self.newm != 0 && self.channel[c].chtype == ChType::Op4 {
            let pair = self.channel[c].pair;
            self.channel[pair].f_num = self.channel[c].f_num;
            self.channel[pair].block = self.channel[c].block;
            self.channel[pair].ksv = self.channel[c].ksv;
            let pair_slots = self.channel[pair].slots;
            self.envelope_update_ksl(pair_slots[0]);
            self.envelope_update_ksl(pair_slots[1]);
        }
    }

    // = OPL3_ChannelSetupAlg
    fn channel_setup_alg(&mut self, c: usize) {
        let [s0, s1] = self.channel[c].slots;
        let alg = self.channel[c].alg;
        if self.channel[c].chtype == ChType::Drum {
            if self.channel[c].ch_num == 7 || self.channel[c].ch_num == 8 {
                self.slot[s0].mod_src = ModSource::Zero;
                self.slot[s1].mod_src = ModSource::Zero;
                return;
            }
            match alg & 0x01 {
                0x00 => {
                    self.slot[s0].mod_src = ModSource::SlotFbmod(s0);
                    self.slot[s1].mod_src = ModSource::SlotOut(s0);
                }
                _ => {
                    self.slot[s0].mod_src = ModSource::SlotFbmod(s0);
                    self.slot[s1].mod_src = ModSource::Zero;
                }
            }
            return;
        }
        if alg & 0x08 != 0 {
            return;
        }
        if alg & 0x04 != 0 {
            let pair = self.channel[c].pair;
            let [p0, p1] = self.channel[pair].slots;
            self.channel[pair].out = [ModSource::Zero; 4];
            match alg & 0x03 {
                0x00 => {
                    self.slot[p0].mod_src = ModSource::SlotFbmod(p0);
                    self.slot[p1].mod_src = ModSource::SlotOut(p0);
                    self.slot[s0].mod_src = ModSource::SlotOut(p1);
                    self.slot[s1].mod_src = ModSource::SlotOut(s0);
                    self.channel[c].out = [
                        ModSource::SlotOut(s1),
                        ModSource::Zero,
                        ModSource::Zero,
                        ModSource::Zero,
                    ];
                }
                0x01 => {
                    self.slot[p0].mod_src = ModSource::SlotFbmod(p0);
                    self.slot[p1].mod_src = ModSource::SlotOut(p0);
                    self.slot[s0].mod_src = ModSource::Zero;
                    self.slot[s1].mod_src = ModSource::SlotOut(s0);
                    self.channel[c].out = [
                        ModSource::SlotOut(p1),
                        ModSource::SlotOut(s1),
                        ModSource::Zero,
                        ModSource::Zero,
                    ];
                }
                0x02 => {
                    self.slot[p0].mod_src = ModSource::SlotFbmod(p0);
                    self.slot[p1].mod_src = ModSource::Zero;
                    self.slot[s0].mod_src = ModSource::SlotOut(p1);
                    self.slot[s1].mod_src = ModSource::SlotOut(s0);
                    self.channel[c].out = [
                        ModSource::SlotOut(p0),
                        ModSource::SlotOut(s1),
                        ModSource::Zero,
                        ModSource::Zero,
                    ];
                }
                _ => {
                    self.slot[p0].mod_src = ModSource::SlotFbmod(p0);
                    self.slot[p1].mod_src = ModSource::Zero;
                    self.slot[s0].mod_src = ModSource::SlotOut(p1);
                    self.slot[s1].mod_src = ModSource::Zero;
                    self.channel[c].out = [
                        ModSource::SlotOut(p0),
                        ModSource::SlotOut(s0),
                        ModSource::SlotOut(s1),
                        ModSource::Zero,
                    ];
                }
            }
        } else {
            match alg & 0x01 {
                0x00 => {
                    self.slot[s0].mod_src = ModSource::SlotFbmod(s0);
                    self.slot[s1].mod_src = ModSource::SlotOut(s0);
                    self.channel[c].out = [
                        ModSource::SlotOut(s1),
                        ModSource::Zero,
                        ModSource::Zero,
                        ModSource::Zero,
                    ];
                }
                _ => {
                    self.slot[s0].mod_src = ModSource::SlotFbmod(s0);
                    self.slot[s1].mod_src = ModSource::Zero;
                    self.channel[c].out = [
                        ModSource::SlotOut(s0),
                        ModSource::SlotOut(s1),
                        ModSource::Zero,
                        ModSource::Zero,
                    ];
                }
            }
        }
    }

    // = OPL3_ChannelUpdateAlg
    fn channel_update_alg(&mut self, c: usize) {
        let con = self.channel[c].con;
        let pair = self.channel[c].pair;
        self.channel[c].alg = con;
        if self.newm != 0 {
            if self.channel[c].chtype == ChType::Op4 {
                self.channel[pair].alg = 0x04 | (con << 1) | self.channel[pair].con;
                self.channel[c].alg = 0x08;
                self.channel_setup_alg(pair);
            } else if self.channel[c].chtype == ChType::Op4Second {
                self.channel[c].alg = 0x04 | (self.channel[pair].con << 1) | con;
                self.channel[pair].alg = 0x08;
                self.channel_setup_alg(c);
            } else {
                self.channel_setup_alg(c);
            }
        } else {
            self.channel_setup_alg(c);
        }
    }

    // = OPL3_ChannelWriteC0
    fn channel_write_c0(&mut self, c: usize, data: u8) {
        self.channel[c].fb = (data & 0x0e) >> 1;
        self.channel[c].con = data & 0x01;
        self.channel_update_alg(c);
        let newm = self.newm;
        let channel = &mut self.channel[c];
        if newm != 0 {
            channel.cha = if (data >> 4) & 0x01 != 0 { !0 } else { 0 };
            channel.chb = if (data >> 5) & 0x01 != 0 { !0 } else { 0 };
            channel.chc = if (data >> 6) & 0x01 != 0 { !0 } else { 0 };
            channel.chd = if (data >> 7) & 0x01 != 0 { !0 } else { 0 };
        } else {
            channel.cha = !0;
            channel.chb = !0;
            // TODO: Verify on real chip if DAC2 output is disabled in compat mode
            channel.chc = 0;
            channel.chd = 0;
        }
    }

    // = OPL3_ChannelKeyOn
    fn channel_key_on(&mut self, c: usize) {
        let [s0, s1] = self.channel[c].slots;
        if self.newm != 0 {
            match self.channel[c].chtype {
                ChType::Op4 => {
                    let [p0, p1] = self.channel[self.channel[c].pair].slots;
                    self.envelope_key_on(s0, EGK_NORM);
                    self.envelope_key_on(s1, EGK_NORM);
                    self.envelope_key_on(p0, EGK_NORM);
                    self.envelope_key_on(p1, EGK_NORM);
                }
                ChType::Op2 | ChType::Drum => {
                    self.envelope_key_on(s0, EGK_NORM);
                    self.envelope_key_on(s1, EGK_NORM);
                }
                ChType::Op4Second => {}
            }
        } else {
            self.envelope_key_on(s0, EGK_NORM);
            self.envelope_key_on(s1, EGK_NORM);
        }
    }

    // = OPL3_ChannelKeyOff
    fn channel_key_off(&mut self, c: usize) {
        let [s0, s1] = self.channel[c].slots;
        if self.newm != 0 {
            match self.channel[c].chtype {
                ChType::Op4 => {
                    let [p0, p1] = self.channel[self.channel[c].pair].slots;
                    self.envelope_key_off(s0, EGK_NORM);
                    self.envelope_key_off(s1, EGK_NORM);
                    self.envelope_key_off(p0, EGK_NORM);
                    self.envelope_key_off(p1, EGK_NORM);
                }
                ChType::Op2 | ChType::Drum => {
                    self.envelope_key_off(s0, EGK_NORM);
                    self.envelope_key_off(s1, EGK_NORM);
                }
                ChType::Op4Second => {}
            }
        } else {
            self.envelope_key_off(s0, EGK_NORM);
            self.envelope_key_off(s1, EGK_NORM);
        }
    }

    // = OPL3_ChannelSet4Op
    fn channel_set_4op(&mut self, data: u8) {
        for bit in 0..6usize {
            let mut chnum = bit;
            if bit >= 3 {
                chnum += 9 - 3;
            }
            if (data >> bit) & 0x01 != 0 {
                self.channel[chnum].chtype = ChType::Op4;
                self.channel[chnum + 3].chtype = ChType::Op4Second;
                self.channel_update_alg(chnum);
            } else {
                self.channel[chnum].chtype = ChType::Op2;
                self.channel[chnum + 3].chtype = ChType::Op2;
                self.channel_update_alg(chnum);
                self.channel_update_alg(chnum + 3);
            }
        }
    }

    // = OPL3_ProcessSlot
    fn process_slot(&mut self, s: usize) {
        self.slot_calc_fb(s);
        self.envelope_calc(s);
        self.phase_generate(s);
        self.slot_generate(s);
    }

    /// Sum a channel's four output taps into the 16-bit accumulator the
    /// mixer masks (the `accm` local in `OPL3_Generate4Ch`).
    #[inline]
    fn channel_accm(&self, c: usize) -> i16 {
        let out = self.channel[c].out;
        (self.read(out[0]) as i32
            + self.read(out[1]) as i32
            + self.read(out[2]) as i32
            + self.read(out[3]) as i32) as i16
    }

    // = OPL3_Generate4Ch — run the chip for one native (49716 Hz) sample and
    // return the four DAC outputs: `[left_a, right_a, left_b, right_b]`.
    pub fn generate_4ch(&mut self) -> [i16; 4] {
        let mut buf4 = [0i16; 4];

        buf4[1] = clip_sample(self.mixbuff[1]);
        buf4[3] = clip_sample(self.mixbuff[3]);

        // OPL_QUIRK_CHANNELSAMPLEDELAY: some FM channels are output one sample
        // later on the left side than the right.
        for ii in 0..15 {
            self.process_slot(ii);
        }

        let mut mix = [0i32; 2];
        for ii in 0..18 {
            let accm = self.channel_accm(ii);
            mix[0] += (accm as u16 & self.channel[ii].cha) as i16 as i32;
            mix[1] += (accm as u16 & self.channel[ii].chc) as i16 as i32;
        }
        self.mixbuff[0] = mix[0];
        self.mixbuff[2] = mix[1];

        for ii in 15..18 {
            self.process_slot(ii);
        }

        buf4[0] = clip_sample(self.mixbuff[0]);
        buf4[2] = clip_sample(self.mixbuff[2]);

        for ii in 18..33 {
            self.process_slot(ii);
        }

        let mut mix = [0i32; 2];
        for ii in 0..18 {
            let accm = self.channel_accm(ii);
            mix[0] += (accm as u16 & self.channel[ii].chb) as i16 as i32;
            mix[1] += (accm as u16 & self.channel[ii].chd) as i16 as i32;
        }
        self.mixbuff[1] = mix[0];
        self.mixbuff[3] = mix[1];

        for ii in 33..36 {
            self.process_slot(ii);
        }

        if (self.timer & 0x3f) == 0x3f {
            self.tremolopos = (self.tremolopos + 1) % 210;
        }
        if self.tremolopos < 105 {
            self.tremolo = self.tremolopos >> self.tremoloshift;
        } else {
            self.tremolo = (210 - self.tremolopos) >> self.tremoloshift;
        }

        if (self.timer & 0x3ff) == 0x3ff {
            self.vibpos = (self.vibpos + 1) & 7;
        }

        self.timer = self.timer.wrapping_add(1);

        if self.eg_state != 0 {
            let mut shift: u8 = 0;
            while shift < 13 && ((self.eg_timer >> shift) & 1) == 0 {
                shift += 1;
            }
            if shift > 12 {
                self.eg_add = 0;
            } else {
                self.eg_add = shift + 1;
            }
            self.eg_timer_lo = (self.eg_timer & 0x3) as u8;
        }

        if self.eg_timerrem != 0 || self.eg_state != 0 {
            if self.eg_timer == 0xf_ffff_ffff {
                self.eg_timer = 0;
                self.eg_timerrem = 1;
            } else {
                self.eg_timer += 1;
                self.eg_timerrem = 0;
            }
        }

        self.eg_state ^= 1;

        while self.writebuf[self.writebuf_cur].time <= self.writebuf_samplecnt {
            let entry = self.writebuf[self.writebuf_cur];
            if entry.reg & 0x200 == 0 {
                break;
            }
            let reg = entry.reg & 0x1ff;
            self.writebuf[self.writebuf_cur].reg = reg;
            self.write_reg(reg, entry.data);
            self.writebuf_cur = (self.writebuf_cur + 1) % OPL_WRITEBUF_SIZE;
        }
        self.writebuf_samplecnt += 1;

        buf4
    }

    // = OPL3_Generate — one native-rate stereo sample `[left, right]`.
    pub fn generate(&mut self) -> [i16; 2] {
        let samples = self.generate_4ch();
        [samples[0], samples[1]]
    }

    // = OPL3_Generate4ChResampled — one four-channel sample at the output
    // rate given to `new`, linearly interpolated from the native rate.
    pub fn generate_4ch_resampled(&mut self) -> [i16; 4] {
        while self.samplecnt >= self.rateratio {
            self.oldsamples = self.samples;
            self.samples = self.generate_4ch();
            self.samplecnt -= self.rateratio;
        }
        let mut buf4 = [0i16; 4];
        for (out, (&old, &new)) in buf4
            .iter_mut()
            .zip(self.oldsamples.iter().zip(&self.samples))
        {
            *out = ((old as i32 * (self.rateratio - self.samplecnt) + new as i32 * self.samplecnt)
                / self.rateratio) as i16;
        }
        self.samplecnt += 1 << RSM_FRAC;
        buf4
    }

    // = OPL3_GenerateResampled — one stereo sample `[left, right]` at the
    // output rate.
    pub fn generate_resampled(&mut self) -> [i16; 2] {
        let samples = self.generate_4ch_resampled();
        [samples[0], samples[1]]
    }

    // = OPL3_WriteReg — write a register immediately. Bit 8 of `reg`
    // selects the second register bank.
    pub fn write_reg(&mut self, reg: u16, v: u8) {
        let high = ((reg >> 8) & 0x01) as usize;
        let regm = (reg & 0xff) as u8;
        let slot_of = |regm: u8| -> Option<usize> {
            let s = AD_SLOT[(regm & 0x1f) as usize];
            (s >= 0).then(|| 18 * high + s as usize)
        };
        match regm & 0xf0 {
            0x00 => {
                if high != 0 {
                    match regm & 0x0f {
                        0x04 => self.channel_set_4op(v),
                        0x05 => self.newm = v & 0x01,
                        _ => {}
                    }
                } else if regm & 0x0f == 0x08 {
                    self.nts = (v >> 6) & 0x01;
                }
            }
            0x20 | 0x30 => {
                if let Some(s) = slot_of(regm) {
                    self.slot_write_20(s, v);
                }
            }
            0x40 | 0x50 => {
                if let Some(s) = slot_of(regm) {
                    self.slot_write_40(s, v);
                }
            }
            0x60 | 0x70 => {
                if let Some(s) = slot_of(regm) {
                    self.slot_write_60(s, v);
                }
            }
            0x80 | 0x90 => {
                if let Some(s) = slot_of(regm) {
                    self.slot_write_80(s, v);
                }
            }
            0xe0 | 0xf0 => {
                if let Some(s) = slot_of(regm) {
                    self.slot_write_e0(s, v);
                }
            }
            0xa0 if (regm & 0x0f) < 9 => {
                self.channel_write_a0(9 * high + (regm & 0x0f) as usize, v);
            }
            0xb0 => {
                if regm == 0xbd && high == 0 {
                    self.tremoloshift = (((v >> 7) ^ 1) << 1) + 2;
                    self.vibshift = ((v >> 6) & 0x01) ^ 1;
                    self.channel_update_rhythm(v);
                } else if (regm & 0x0f) < 9 {
                    let c = 9 * high + (regm & 0x0f) as usize;
                    self.channel_write_b0(c, v);
                    if v & 0x20 != 0 {
                        self.channel_key_on(c);
                    } else {
                        self.channel_key_off(c);
                    }
                }
            }
            0xc0 if (regm & 0x0f) < 9 => {
                self.channel_write_c0(9 * high + (regm & 0x0f) as usize, v);
            }
            _ => {}
        }
    }

    // = OPL3_WriteRegBuffered — queue a register write so it lands at least
    // `OPL_WRITEBUF_DELAY` native samples after the previous buffered write.
    pub fn write_reg_buffered(&mut self, reg: u16, v: u8) {
        let writebuf_last = self.writebuf_last;
        let entry = self.writebuf[writebuf_last];

        if entry.reg & 0x200 != 0 {
            self.write_reg(entry.reg & 0x1ff, entry.data);

            self.writebuf_cur = (writebuf_last + 1) % OPL_WRITEBUF_SIZE;
            self.writebuf_samplecnt = entry.time;
        }

        let mut time1 = self.writebuf_lasttime + OPL_WRITEBUF_DELAY;
        let time2 = self.writebuf_samplecnt;

        if time1 < time2 {
            time1 = time2;
        }

        self.writebuf[writebuf_last] = WriteBuf {
            time: time1,
            reg: reg | 0x200,
            data: v,
        };
        self.writebuf_lasttime = time1;
        self.writebuf_last = (writebuf_last + 1) % OPL_WRITEBUF_SIZE;
    }

    // = OPL3_Generate4ChStream — fill two interleaved stereo buffers (DAC
    // pair A into `sndptr1`, pair B into `sndptr2`) at the output rate.
    pub fn generate_4ch_stream(&mut self, sndptr1: &mut [i16], sndptr2: &mut [i16]) {
        for (a, b) in sndptr1.chunks_exact_mut(2).zip(sndptr2.chunks_exact_mut(2)) {
            let samples = self.generate_4ch_resampled();
            a[0] = samples[0];
            a[1] = samples[1];
            b[0] = samples[2];
            b[1] = samples[3];
        }
    }

    // = OPL3_GenerateStream — fill an interleaved stereo buffer at the
    // output rate. A trailing odd element is left untouched.
    pub fn generate_stream(&mut self, sndptr: &mut [i16]) {
        for frame in sndptr.chunks_exact_mut(2) {
            let samples = self.generate_resampled();
            frame[0] = samples[0];
            frame[1] = samples[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny PCG-style generator so the schedule below is reproducible.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
    }

    /// Drive the chip with a pseudo-random write schedule (register scribbles
    /// across both banks plus regular key-on events) and FNV-hash every
    /// output sample. The expected hashes were produced by running the same
    /// schedule through the C Nuked-OPL3 1.8 build in opl3-rs 0.2.x, so a
    /// match here means the port is still bit-exact with the original.
    fn schedule_hash(rate: u32, seed: u64, buffered: bool, chunks: usize) -> (usize, u64) {
        let mut chip = Opl3Chip::new(rate);
        let mut rng = Lcg(seed);
        let mut total = 0usize;
        let mut hash: u64 = 0xcbf29ce484222325;
        let mut writes: Vec<(u16, u8)> = vec![(0x105, 1), (0x104, 0x03), (0xbd, 0xe0)];
        for i in 0..chunks {
            let n = 1 + (rng.next() % 6) as usize;
            for _ in 0..n {
                let reg = (rng.next() % 0x200) as u16;
                let v = (rng.next() & 0xff) as u8;
                writes.push((reg, v));
            }
            if i % 7 == 0 {
                let ch = (rng.next() % 18) as u16;
                let bank = if ch >= 9 { 0x100 } else { 0 };
                let ch = ch % 9;
                writes.push((bank | (0xa0 + ch), (rng.next() & 0xff) as u8));
                writes.push((bank | (0xb0 + ch), 0x20 | (rng.next() & 0x1f) as u8));
            }
            for &(reg, v) in &writes {
                if buffered {
                    chip.write_reg_buffered(reg, v);
                } else {
                    chip.write_reg(reg, v);
                }
            }
            writes.clear();
            let frames = 1 + (rng.next() % 600) as usize;
            let mut buf = vec![0i16; frames * 2];
            chip.generate_stream(&mut buf);
            for s in &buf {
                hash ^= *s as u16 as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            total += frames;
        }
        (total, hash)
    }

    #[test]
    fn matches_c_reference_native_rate() {
        assert_eq!(
            schedule_hash(49716, 1, false, 60),
            (19023, 0x99c697fd0147fad6)
        );
        assert_eq!(
            schedule_hash(49716, 1, true, 60),
            (19023, 0x257b8d1f429d8e60)
        );
    }

    #[test]
    fn matches_c_reference_resampled() {
        assert_eq!(
            schedule_hash(44100, 1, false, 60),
            (19023, 0x14643eb1ad6d4bdb)
        );
        assert_eq!(
            schedule_hash(44100, 1, true, 60),
            (19023, 0xafc183dbe4d9bac7)
        );
    }

    #[test]
    fn silent_after_reset() {
        let mut chip = Opl3Chip::new(44100);
        let mut buf = [0x7fffi16; 64];
        chip.generate_stream(&mut buf);
        assert!(buf.iter().all(|&s| s == 0));
    }
}
