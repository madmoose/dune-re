pub trait Device {
    fn write_opl(&mut self, reg: u16, val: u8);
}

const MOD_OP_REG: [u8; 9] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x10, 0x11, 0x12];
const CAR_OP_REG: [u8; 9] = [0x03, 0x04, 0x05, 0x0B, 0x0C, 0x0D, 0x13, 0x14, 0x15];

const FREQ_TABLE: [u16; 12] = [
    0x157, 0x16C, 0x181, 0x198, 0x1B1, 0x1CB, 0x1E6, 0x203, 0x222, 0x243, 0x266, 0x28A,
];

const SEMITONE_SCALE: [u8; 13] = [
    0x13, 0x15, 0x15, 0x17, 0x19, 0x1A, 0x1B, 0x1D, 0x1F, 0x21, 0x23, 0x24, 0x25,
];

const PITCH_FRAC: [u8; 10] = [0, 5, 10, 15, 20, 0, 6, 12, 18, 24];

#[derive(Clone, Copy)]
pub struct Channel {
    pub timer: u16,
    pub read_ptr: usize,
    pub init_ptr: u16,
    pub program: u8,
    pub note: u8,
    bend_mode: u8,
    transpose: i8,
    glide_countdown: u8,
    glide_steps: u8,
    glide_accum: u8,
    glide_step: i8,
    mod_vel_sens: u8,
    car_vel_sens_factor: u8,
    op_base_mod_level: u8,
    op_base_car_level: u8,
    op_cur_mod_level: u8,
    op_cur_car_level: u8,
    car_vel_sens_base: u8,
    fb_conn_dyn: u8,
    mod_vol_sens: u8,
    car_vol_sens_0c7: u8,
    car_vol_sens: u8,
    fb_conn: u8,
    freq: u16,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            timer: 0,
            read_ptr: 0,
            init_ptr: 0,
            program: 0,
            note: 0,
            bend_mode: 0,
            transpose: 0,
            glide_countdown: 0,
            glide_steps: 0,
            glide_accum: 0,
            glide_step: 0,
            mod_vel_sens: 0,
            car_vel_sens_factor: 0,
            op_base_mod_level: 0,
            op_base_car_level: 0,
            op_cur_mod_level: 0,
            op_cur_car_level: 0,
            car_vel_sens_base: 0,
            fb_conn_dyn: 0,
            mod_vol_sens: 0,
            car_vol_sens_0c7: 0,
            car_vol_sens: 0,
            fb_conn: 0,
            freq: 0x0157,
        }
    }
}

pub struct HeradADL {
    pub device: Box<dyn Device>,
    status: u8,
    song_play_count: u8,
    vol_current: u8,
    vol_target: u8,
    vol_master: u8,
    fade_pattern: u16,
    subtick_accum: u16,
    measure: u16,
    tick: u16,
    loop_countdown: u16,
    instruments_offset: u16,
    loop_start: u16,
    loop_end: u16,
    loop_count: u16,
    speed: u16,
    channels: [Channel; 9],
    loop_snapshot: [(u16, usize); 9],
}

impl HeradADL {
    pub fn new(device: Box<dyn Device>) -> Self {
        Self {
            device,
            status: 0,
            song_play_count: 0,
            vol_current: 0,
            vol_target: 0,
            vol_master: 0,
            fade_pattern: 0x0001,
            subtick_accum: 0,
            measure: 0,
            tick: 0,
            loop_countdown: 0,
            instruments_offset: 0,
            loop_start: 0,
            loop_end: 0,
            loop_count: 0,
            speed: 0,
            channels: [Channel::default(); 9],
            loop_snapshot: [(0, 0); 9],
        }
    }

    pub fn init(&mut self) {
        self.opl_register_write(0x01, 0x20);
        self.opl_register_write(0xBD, 0x00);
        self.opl_register_write(0x08, 0x40);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.silence_all_channels();
        self.status = 0;
    }

    pub fn silence_all_channels(&mut self) {
        for i in (0..9).rev() {
            self.opl_note_off(i);
        }
    }

    pub fn open(&mut self, song_data: &[u8], song_play_count: u8) {
        if song_data.len() < 2 + (21 * 2) + 8 {
            return;
        }

        self.song_play_count = song_play_count;

        let mut r = std::io::Cursor::new(song_data);
        use bytes_ext::ReadBytesExt;

        let header_size = r.read_le_u16().unwrap_or(0);
        self.instruments_offset = header_size;

        let mut track_offsets = [0u16; 9];
        for slot in &mut track_offsets {
            let off = r.read_le_u16().unwrap_or(0);
            *slot = if off != 0 { off + 2 } else { 0 };
        }

        for _ in 0..12 {
            let _ = r.read_le_u16();
        }

        self.loop_start = r.read_le_u16().unwrap_or(0);
        self.loop_end = r.read_le_u16().unwrap_or(0);
        self.loop_count = r.read_le_u16().unwrap_or(0);
        self.speed = r.read_le_u16().unwrap_or(0);

        self.mute_all_operators();
        self.build_channel_table(track_offsets);
        self.rewind_all_channels(song_data);

        self.vol_current = self.vol_master;
        self.vol_target = self.vol_current;
        self.subtick_accum = 0;
        self.loop_countdown = 0;

        self.process_tick(song_data);
        self.status = 0x80;
    }

    fn mute_all_operators(&mut self) {
        for &offset in MOD_OP_REG.iter().chain(CAR_OP_REG.iter()) {
            self.opl_register_write(0x80 + offset as u16, 0xFF);
        }
    }

    fn build_channel_table(&mut self, track_offsets: [u16; 9]) {
        for (i, &offset) in track_offsets.iter().enumerate() {
            self.channels[i].init_ptr = offset;
            self.channels[i].program = 0xFF;
            self.channels[i].note = 0;
        }
    }

    fn rewind_all_channels(&mut self, song_data: &[u8]) {
        self.measure = 1;
        self.tick = 0x60;
        for i in 0..9 {
            let init_ptr = self.channels[i].init_ptr;
            self.channels[i].read_ptr = init_ptr as usize;
            self.channels[i].timer = 0xFFFF;
            if init_ptr != 0 {
                self.read_wait_value(song_data, i);
                self.channels[i].timer = self.channels[i].timer.wrapping_add(1);
            }
        }
    }

    pub fn channels(&self) -> &[Channel; 9] {
        &self.channels
    }

    pub fn tick_handler(&mut self, song_data: &[u8]) -> (u8, u16, u16) {
        if (self.status & 0x80) == 0 {
            return (self.status, self.measure, self.tick);
        }
        let (new_accum, overflow) = self.subtick_accum.overflowing_sub(0x100);
        self.subtick_accum = new_accum;
        if overflow {
            self.process_tick(song_data);
        }
        self.fade_pattern = self.fade_pattern.rotate_left(1);
        if (self.fade_pattern & 1) != 0 {
            self.fade_step(song_data);
        }
        (self.status, self.measure, self.tick)
    }

    fn process_tick(&mut self, song_data: &[u8]) {
        self.subtick_accum = self.subtick_accum.wrapping_add(self.speed);
        self.loop_point_check(song_data);
        for i in 0..9 {
            self.channels[i].timer = self.channels[i].timer.wrapping_sub(1);
            if self.channels[i].timer == 0 {
                if self.channels[i].read_ptr != 0 {
                    self.process_events(song_data, i);
                }
            } else if self.channels[i].glide_countdown != 0 && self.channels[i].read_ptr != 0 {
                self.channels[i].glide_countdown -= 1;
                let step = self.channels[i].glide_step;
                self.channels[i].glide_accum =
                    self.channels[i].glide_accum.wrapping_add(step as u8);
                self.pitch_bend(i, self.channels[i].glide_accum);
            }
        }
        self.tick -= 1;
        if self.tick == 0 {
            self.tick = 0x60;
            self.measure += 1;
        }
    }

    fn loop_point_check(&mut self, _song_data: &[u8]) {
        if self.loop_countdown == 0 {
            if self.loop_start != 0 && self.measure == self.loop_start && self.tick == 0x60 {
                for i in 0..9 {
                    self.loop_snapshot[i] = (self.channels[i].timer, self.channels[i].read_ptr);
                }
                self.loop_countdown = self.loop_count.saturating_sub(1);
            }
        } else if self.loop_end != 0 && self.measure == self.loop_end {
            self.loop_countdown -= 1;
            for i in 0..9 {
                let (timer, read_ptr) = self.loop_snapshot[i];
                self.channels[i].timer = timer;
                self.channels[i].read_ptr = read_ptr;
            }
            self.measure = self.loop_start;
        }
    }

    fn process_events(&mut self, song_data: &[u8], chan: usize) {
        loop {
            let ptr = self.channels[chan].read_ptr;
            if ptr + 1 >= song_data.len() {
                self.channels[chan].timer = 0xFFFF;
                return;
            }
            let event = song_data[ptr];
            let param = song_data[ptr + 1];
            self.channels[chan].read_ptr += 2;
            let op = (event >> 4) & 0x07;
            match op {
                0 => self.op_note_off(song_data, chan, param),
                1 => self.op_note_on(song_data, chan, param),
                2 | 3 => self.op_wait(song_data, chan),
                4 => self.op_program_change(song_data, chan, param),
                5 => self.op_volume_modulation(song_data, chan, param),
                6 => self.op_pitch_bend(song_data, chan, param),
                7 => self.op_end_of_track(song_data, chan),
                _ => {}
            }
            if self.channels[chan].timer != 0 {
                break;
            }
        }
    }

    fn op_note_off(&mut self, song_data: &[u8], chan: usize, note: u8) {
        self.channels[chan].read_ptr += 1;
        self.read_wait_value(song_data, chan);

        let transposed_note = note.wrapping_add(self.channels[chan].transpose as u8);
        if self.channels[chan].note == transposed_note {
            self.channels[chan].note = 0;
            self.opl_note_off(chan);
        }
    }

    fn op_note_on(&mut self, song_data: &[u8], chan: usize, note: u8) {
        let ptr = self.channels[chan].read_ptr;
        let velocity = song_data[ptr];
        self.channels[chan].read_ptr += 1;
        self.read_wait_value(song_data, chan);

        self.apply_velocity(chan, velocity);
        if self.channels[chan].note != 0 {
            self.opl_frequency_write(chan, 0);
        }
        let transposed_note = note.wrapping_add(self.channels[chan].transpose as u8);
        self.channels[chan].note = transposed_note;
        self.channels[chan].glide_countdown = self.channels[chan].glide_steps;
        self.channels[chan].glide_accum = 0x40;
        self.opl_note_on_internal(chan, transposed_note);
    }

    fn op_wait(&mut self, song_data: &[u8], chan: usize) {
        self.read_wait_value(song_data, chan);
    }

    fn opl_note_on_internal(&mut self, chan: usize, note: u8) {
        let mut ax = (note as u16).wrapping_sub(0x18);
        if ax >= 0x60 {
            ax = 0;
        }
        let octave = (ax / 12) as u8;
        let semitone = (ax % 12) as usize;
        let mut freq = FREQ_TABLE[semitone];
        freq |= (octave as u16) << 10;
        self.channels[chan].freq = freq;
        freq |= 0x2000;
        self.opl_frequency_write(chan, freq);
    }

    fn op_program_change(&mut self, song_data: &[u8], chan: usize, patch: u8) {
        self.read_wait_value(song_data, chan);
        if self.channels[chan].program == patch {
            return;
        }
        self.channels[chan].program = patch;
        let inst_ptr = self.instruments_offset as usize + (patch as usize * 40);
        if inst_ptr + 40 > song_data.len() {
            return;
        }
        let inst = &song_data[inst_ptr..inst_ptr + 40];

        self.channels[chan].bend_mode = inst[0x21];
        self.channels[chan].transpose = inst[0x22] as i8;

        let ksl_mod = (inst[0x02] & 0x03) << 6;
        let ksl_car = (inst[0x0F] & 0x03) << 6;

        // DNADL bakes vol_current into base levels at program change
        self.channels[chan].op_base_mod_level =
            (inst[0x0A].saturating_add(self.vol_current).min(0x3F)) | ksl_mod;
        self.channels[chan].op_base_car_level =
            (inst[0x17].saturating_add(self.vol_current).min(0x3F)) | ksl_car;
        self.channels[chan].op_cur_mod_level = self.channels[chan].op_base_mod_level;
        self.channels[chan].op_cur_car_level = self.channels[chan].op_base_car_level;

        self.channels[chan].mod_vel_sens = inst[0x1E];
        self.channels[chan].car_vel_sens_factor = inst[0x1F];

        self.channels[chan].mod_vol_sens = inst[0x26];
        self.channels[chan].car_vol_sens_0c7 = inst[0x27];

        let mut al = inst[0x0E];
        let mut ax = (al as u16) << 8;
        ax >>= 1;
        al = ax as u8;
        let mut ah = inst[0x04];
        al = !al;
        ax = (ah as u16) << 8 | (al as u16);
        ax <<= 1;
        ah = (ax >> 8) as u8;

        let ah_fb = ah;
        let al_20 = inst[0x20];
        self.channels[chan].car_vel_sens_base = al_20;
        self.channels[chan].fb_conn_dyn = ah_fb;

        let al_1b = inst[0x1B];
        self.channels[chan].car_vol_sens = al_1b;
        self.channels[chan].fb_conn = ah_fb;

        let ax_glide = (inst[0x24] as u16) << 8 | (inst[0x23] as u16);
        self.channels[chan].glide_step = (ax_glide >> 8) as i8;
        self.channels[chan].glide_countdown = 0;
        self.channels[chan].glide_steps = (ax_glide & 0xFF) as u8;

        self.instrument_write(chan, inst);
    }

    fn instrument_write(&mut self, chan: usize, inst: &[u8]) {
        let mod_off = MOD_OP_REG[chan];
        let car_off = CAR_OP_REG[chan];

        let mut al = inst[0x0E];
        let mut ax = (al as u16) << 8;
        ax >>= 1;
        al = ax as u8;
        let mut ah = inst[0x04];
        al = !al;
        ax = (ah as u16) << 8 | (al as u16);
        ax <<= 1;
        ah = (ax >> 8) as u8;
        ah &= 0x0F;

        self.opl_register_write(0xC0 + chan as u16, ah);

        let pack_0x20 = |mult: u8, bits_idx: &[usize]| {
            let mut val = 0u8;
            for &idx in bits_idx.iter().rev() {
                val = (val << 1) | (inst[idx] & 1);
            }
            (val << 4) | (mult & 0x0F)
        };

        self.opl_register_write(0xE0 + mod_off as u16, inst[0x1C] & 3);
        self.opl_register_write(0x40 + mod_off as u16, self.channels[chan].op_base_mod_level);
        self.opl_register_write(0x60 + mod_off as u16, (inst[5] << 4) | (inst[8] & 0x0F));
        self.opl_register_write(0x80 + mod_off as u16, (inst[6] << 4) | (inst[9] & 0x0F));
        self.opl_register_write(
            0x20 + mod_off as u16,
            pack_0x20(inst[3], &[0x0D, 7, 0x0C, 0x0B]),
        );

        self.opl_register_write(0xE0 + car_off as u16, inst[0x1D] & 3);
        self.opl_register_write(0x40 + car_off as u16, self.channels[chan].op_base_car_level);
        self.opl_register_write(0x60 + car_off as u16, (inst[18] << 4) | (inst[21] & 0x0F));
        self.opl_register_write(0x80 + car_off as u16, (inst[19] << 4) | (inst[22] & 0x0F));
        self.opl_register_write(
            0x20 + car_off as u16,
            pack_0x20(inst[16], &[26, 20, 25, 24]),
        );
    }

    fn op_volume_modulation(&mut self, song_data: &[u8], chan: usize, volume: u8) {
        self.read_wait_value(song_data, chan);

        let vol_orig_ah = volume;
        let vol_scaled_al = 0x80u8.wrapping_sub(volume);

        let sens_mod = self.channels[chan].mod_vol_sens;
        if sens_mod != 0 {
            let mut cl = sens_mod as i8;
            let mut al = vol_orig_ah;
            if cl < 0 {
                cl = -cl;
                al = vol_scaled_al;
            }
            let shift = (4 - cl).max(0) as u32;
            let atten = al >> shift;
            let ah = self.channels[chan].op_cur_mod_level;
            let mut res = ah & 0x3F;
            res = res.saturating_sub(atten);
            self.opl_register_write(0x40 + MOD_OP_REG[chan] as u16, (ah & 0xC0) | res);
        }

        let sens_car = self.channels[chan].car_vol_sens_0c7;
        if sens_car != 0 {
            let mut cl = sens_car as i8;
            let mut al = vol_orig_ah;
            if cl < 0 {
                cl = -cl;
                al = vol_scaled_al;
            }
            let shift = (4 - cl).max(0) as u32;
            let atten = al >> shift;
            let ah = self.channels[chan].op_cur_car_level;
            let mut res = ah & 0x3F;
            res = res.saturating_sub(atten);
            self.opl_register_write(0x40 + CAR_OP_REG[chan] as u16, (ah & 0xC0) | res);
        }

        let sens_c0 = self.channels[chan].car_vol_sens;
        if sens_c0 != 0 {
            let mut cl = sens_c0 as i8;
            let mut al = vol_orig_ah;
            if cl < 0 {
                cl = -cl;
                al = vol_scaled_al;
            }
            let shift = (6 - cl).max(0) as u32;
            let mut res = al >> shift;
            res &= 0xFE;
            res = res.wrapping_add(self.channels[chan].fb_conn);
            if res > 0x0F {
                res = (res & 0x0F) | 0x0E;
            }
            self.opl_register_write(0xC0 + chan as u16, res);
        }
    }

    fn op_pitch_bend(&mut self, song_data: &[u8], chan: usize, bend: u8) {
        self.read_wait_value(song_data, chan);
        self.pitch_bend(chan, bend);
    }

    fn op_end_of_track(&mut self, song_data: &[u8], chan: usize) {
        self.channels[chan].timer = 0xFFFF;
        self.channels[chan].read_ptr -= 4;

        if chan == 0 {
            if self.song_play_count != 0 {
                self.song_play_count -= 1;
                if self.song_play_count == 0 {
                    for ch in &mut self.channels {
                        ch.timer = 0xFFFF;
                    }
                    self.reset();
                    return;
                }
            }
            self.rewind_all_channels(song_data);
            self.loop_point_check(song_data);
            self.channels[0].timer = self.channels[0].timer.wrapping_sub(1);
        }
    }

    fn apply_velocity(&mut self, chan: usize, velocity: u8) {
        let vol_orig_ah = velocity;
        let vol_scaled_ah = 0x80u8.wrapping_sub(velocity);
        let ax_val = (vol_scaled_ah as u16) << 8 | vol_orig_ah as u16;

        let sens_mod = self.channels[chan].mod_vel_sens;
        if sens_mod != 0 {
            let mut cl = sens_mod as i8;
            let mut al = (ax_val >> 8) as u8;
            if cl < 0 {
                cl = -cl;
                al = ax_val as u8;
            }
            let shift = (4 - cl).max(0) as u32;
            let atten = al >> shift;
            let ah = self.channels[chan].op_base_mod_level;
            let mut res = ah & 0x3F;
            res = res.saturating_add(atten).min(0x3F);
            self.channels[chan].op_cur_mod_level = (ah & 0xC0) | res;
            self.opl_register_write(
                0x40 + MOD_OP_REG[chan] as u16,
                self.channels[chan].op_cur_mod_level,
            );
        } else {
            self.channels[chan].op_cur_mod_level = self.channels[chan].op_base_mod_level;
        }

        let sens_car = self.channels[chan].car_vel_sens_factor;
        if sens_car != 0 {
            let mut cl = sens_car as i8;
            let mut al = (ax_val >> 8) as u8;
            if cl < 0 {
                cl = -cl;
                al = ax_val as u8;
            }
            let shift = (4 - cl).max(0) as u32;
            let atten = al >> shift;
            let ah = self.channels[chan].op_base_car_level;
            let mut res = ah & 0x3F;
            res = res.saturating_add(atten).min(0x3F);
            self.channels[chan].op_cur_car_level = (ah & 0xC0) | res;
            self.opl_register_write(
                0x40 + CAR_OP_REG[chan] as u16,
                self.channels[chan].op_cur_car_level,
            );
        } else {
            self.channels[chan].op_cur_car_level = self.channels[chan].op_base_car_level;
        }

        let sens_c0 = self.channels[chan].car_vel_sens_base;
        if sens_c0 != 0 {
            let mut cl = sens_c0 as i8;
            let mut al = (ax_val >> 8) as u8;
            if cl < 0 {
                cl = -cl;
                al = ax_val as u8;
            }
            let shift = (6 - cl).max(0) as u32;
            let mut res = al >> shift;
            res &= 0xFE;
            res = res.wrapping_add(self.channels[chan].fb_conn_dyn);
            if res > 0x0F {
                res = (res & 0x0F) | 0x0E;
            }
            self.channels[chan].fb_conn = res;
            self.opl_register_write(0xC0 + chan as u16, res);
        } else {
            self.channels[chan].fb_conn = self.channels[chan].fb_conn_dyn;
        }
    }

    fn pitch_bend(&mut self, chan: usize, bend: u8) {
        let note = self.channels[chan].note;
        if note == 0 {
            return;
        }
        let mut ax = note as u16;
        ax = ax.saturating_sub(0x18);
        let octave = (ax / 12) as u8;
        let semitone = (ax % 12) as usize;
        let bend_val = bend as i16 - 0x40;
        let mut freq: u16;
        let mut semitone = semitone as i16;
        let mut octave = octave as i16;

        if self.channels[chan].bend_mode == 0 {
            if bend_val < 0 {
                let magnitude = (0x40i16 - bend as i16) as u16;
                let ror5 = (magnitude >> 5) | ((magnitude << 11) & 0xFFFF);
                let semitone_shift = (ror5 & 0xFF) as i16;
                let frac_mul = (ror5 >> 8) as u8;
                semitone -= semitone_shift;
                if semitone < 0 {
                    semitone += 12;
                    octave -= 1;
                    if octave < 0 {
                        semitone = 0;
                        octave = 0;
                    }
                }
                let scale = SEMITONE_SCALE[semitone as usize];
                let offset = (scale as u16 * frac_mul as u16) >> 8;
                freq = FREQ_TABLE[semitone as usize];
                freq = freq.saturating_sub(offset);
            } else {
                let magnitude = (bend as i16 - 0x40 + 1) as u16;
                let ror5 = (magnitude >> 5) | ((magnitude << 11) & 0xFFFF);
                let semitone_shift = (ror5 & 0xFF) as i16;
                let frac_mul = (ror5 >> 8) as u8;
                semitone += semitone_shift;
                if semitone >= 12 {
                    semitone -= 12;
                    octave += 1;
                }
                let scale = SEMITONE_SCALE[(semitone as usize + 1).min(12)];
                let offset = (scale as u16 * frac_mul as u16) >> 8;
                freq = FREQ_TABLE[semitone as usize];
                freq = freq.saturating_add(offset);
            }
        } else {
            let abs_bend = bend_val.unsigned_abs() as i16;
            let semitone_shift = abs_bend / 5;
            let frac_idx = (abs_bend % 5) as usize;
            if bend_val < 0 {
                semitone -= semitone_shift;
                if semitone < 0 {
                    semitone += 12;
                    octave -= 1;
                    if octave < 0 {
                        semitone = 0;
                        octave = 0;
                    }
                }
            } else {
                semitone += semitone_shift;
                if semitone >= 12 {
                    semitone -= 12;
                    octave += 1;
                }
            }
            let frac_offset = PITCH_FRAC[if semitone < 6 { frac_idx } else { frac_idx + 5 }] as u16;
            freq = FREQ_TABLE[semitone as usize];
            if bend_val < 0 {
                freq = freq.saturating_sub(frac_offset);
            } else {
                freq = freq.saturating_add(frac_offset);
            }
        }
        let mut ah = (freq >> 8) as u8;
        ah |= (octave as u8) << 2;
        self.channels[chan].freq = (ah as u16) << 8 | (freq & 0xFF);
        ah |= 0x20;
        self.opl_frequency_write(chan, (ah as u16) << 8 | (freq & 0xFF));
    }

    fn volume_to_attenuation(&self, volume: u8) -> u8 {
        let attenuation_level = ((255 - volume) >> 2) as u16;
        ((attenuation_level * attenuation_level) / 63) as u8
    }

    pub fn set_volume(&mut self, volume: u8) {
        let atten = self.volume_to_attenuation(volume);
        self.vol_master = atten;
        self.vol_target = atten;
        self.fade_pattern = 0xFFFF;
    }

    pub fn set_music_during_voices(&mut self, ramp_ticks: u16, volume: u8) {
        let atten = self.volume_to_attenuation(volume);
        self.vol_target = atten;
        self.fade_pattern = if ramp_ticks < 0x60 {
            0xFFFF
        } else if ramp_ticks < 0xC0 {
            0xAAAA
        } else if ramp_ticks < 0x180 {
            0x8888
        } else if ramp_ticks < 0x300 {
            0x8080
        } else {
            0x8000
        };
        if (self.status & 0x80) != 0 {
            self.status |= 0x40;
        }
    }

    pub fn opl_note_off(&mut self, chan: usize) {
        let freq = self.channels[chan].freq;
        self.opl_frequency_write(chan, freq);
    }

    fn opl_frequency_write(&mut self, chan: usize, freq: u16) {
        self.opl_register_write(0xA0 + chan as u16, (freq & 0xFF) as u8);
        self.opl_register_write(0xB0 + chan as u16, (freq >> 8) as u8);
    }

    fn opl_register_write(&mut self, reg: u16, val: u8) {
        self.device.write_opl(reg, val);
    }

    fn read_wait_value(&mut self, song_data: &[u8], chan: usize) -> u16 {
        let mut ptr = self.channels[chan].read_ptr;
        if ptr >= song_data.len() {
            return 0;
        }
        let mut b = song_data[ptr];
        ptr += 1;
        let wait = if (b & 0x80) == 0 {
            b as u16
        } else {
            let mut val = (b & 0x7F) as u16;
            let mut count = 0;
            loop {
                if ptr >= song_data.len() {
                    break;
                }
                b = song_data[ptr];
                ptr += 1;
                val = (val << 7) | (b & 0x7F) as u16;
                count += 1;
                if (b & 0x80) == 0 || count >= 2 {
                    break;
                }
            }
            if count >= 2 && (b & 0x80) != 0 {
                0xFFFF
            } else {
                val
            }
        };
        self.channels[chan].read_ptr = ptr;
        self.channels[chan].timer = wait;
        wait
    }

    fn fade_step(&mut self, song_data: &[u8]) {
        let cur = self.vol_current;
        let target = self.vol_target;
        if cur == target {
            self.fade_pattern = 1;
            self.status &= !0x40;
            return;
        }

        if cur > target {
            // Fade IN: decrease attenuation
            for i in 0..9 {
                let program = self.channels[i].program;
                if program == 0xFF {
                    continue;
                }

                let inst_ptr = self.instruments_offset as usize + (program as usize * 40);
                if inst_ptr + 40 > song_data.len() {
                    continue;
                }
                let inst = &song_data[inst_ptr..inst_ptr + 40];

                // Modulator
                let base_mod = self.channels[i].op_base_mod_level & 0x3F;
                if base_mod > inst[0x0A] {
                    self.channels[i].op_base_mod_level -= 1;
                    if (self.channels[i].op_cur_mod_level & 0x3F) > 0 {
                        self.channels[i].op_cur_mod_level -= 1;
                        self.opl_register_write(
                            0x40 + MOD_OP_REG[i] as u16,
                            self.channels[i].op_cur_mod_level,
                        );
                    }
                }

                // Carrier
                let base_car = self.channels[i].op_base_car_level & 0x3F;
                if base_car > inst[0x17] {
                    self.channels[i].op_base_car_level -= 1;
                    if (self.channels[i].op_cur_car_level & 0x3F) > 0 {
                        self.channels[i].op_cur_car_level -= 1;
                        self.opl_register_write(
                            0x40 + CAR_OP_REG[i] as u16,
                            self.channels[i].op_cur_car_level,
                        );
                    }
                }
            }
            self.vol_current -= 1;
        } else {
            // Fade OUT: increase attenuation
            for i in 0..9 {
                // Modulator
                if (self.channels[i].op_base_mod_level & 0x3F) < 0x3F {
                    self.channels[i].op_base_mod_level += 1;
                }
                if (self.channels[i].op_cur_mod_level & 0x3F) < 0x3F {
                    self.channels[i].op_cur_mod_level += 1;
                    self.opl_register_write(
                        0x40 + MOD_OP_REG[i] as u16,
                        self.channels[i].op_cur_mod_level,
                    );
                }

                // Carrier
                if (self.channels[i].op_base_car_level & 0x3F) < 0x3F {
                    self.channels[i].op_base_car_level += 1;
                }
                if (self.channels[i].op_cur_car_level & 0x3F) < 0x3F {
                    self.channels[i].op_cur_car_level += 1;
                    self.opl_register_write(
                        0x40 + CAR_OP_REG[i] as u16,
                        self.channels[i].op_cur_car_level,
                    );
                }
            }
            self.vol_current += 1;
            if self.vol_current >= 0x3F {
                self.silence_all_channels();
                self.status = 0;
            }
        }
    }
}
