use crate::game_state::GameState;

impl GameState {
    // = seg000:e3b7 rand_masked. 16-bit LCG (seed*0xe56d+1 at [0d824h]); the
    // returned value is `((product>>16)&0xff)<<8 | (seed>>8)` ANDed with the
    // mask — a *mask*, not a modulo, so rand_masked(6) yields {0,2,4,6}.
    pub fn rand_masked(&mut self, mask: u16) -> u16 {
        let product = (self.rand_seed as u32).wrapping_mul(0xe56d);
        let seed_new = ((product & 0xffff) as u16).wrapping_add(1);
        self.rand_seed = seed_new;
        let lo = seed_new >> 8;
        let hi = ((product >> 16) & 0xff) as u16;
        ((hi << 8) | lo) & mask
    }

    // = seg000:e3cc rand. 16-bit LCG with a separate seed at [0d826h] and
    // multiplier 0xcbd1; returns `((product>>16)&0xff)<<8 | (seed>>8)`. Drives
    // the rand_bits churn the game_loop performs once per pass.
    pub fn rand(&mut self) -> u16 {
        let product = (self.rand_bits_seed as u32).wrapping_mul(0xcbd1);
        let seed_new = ((product & 0xffff) as u16).wrapping_add(1);
        self.rand_bits_seed = seed_new;
        let lo = seed_new >> 8;
        let hi = ((product >> 16) & 0xff) as u16;
        (hi << 8) | lo
    }

    // = seg000:e3df rand_iterated — a uniform random draw in 0..=max: build the
    // smallest all-ones mask covering `max`, then iterate the 0xcbd1 LCG on its
    // own seed (= _unk_2CCD8_bios_timer_count_3, distinct from rand's and
    // rand_masked's) until the masked draw lands within range. Drives the CD-
    // playlist shuffle (music_cd_playlist_shuffle) among others.
    pub(crate) fn rand_iterated(&mut self, max: u16) -> u16 {
        // = seg000:e3e1..e3e5 max == 0 returns 0 immediately (ax is already 0).
        if max == 0 {
            return 0;
        }
        // = seg000:e3e7..e3f0 the mask: 0xffff shifted left once per bit of max, inverted.
        let mut mask = 0xffffu16;
        let mut ax = max;
        while ax != 0 {
            mask <<= 1;
            ax >>= 1;
        }
        mask = !mask;
        // = seg000:e3f2 loc_0e3f2 the retry loop: redraw while the masked value exceeds max.
        loop {
            let product = (self.rand_iterated_seed as u32).wrapping_mul(0xcbd1);
            let seed_new = ((product & 0xffff) as u16).wrapping_add(1);
            self.rand_iterated_seed = seed_new;
            let lo = seed_new >> 8;
            let hi = ((product >> 16) & 0xff) as u16;
            let val = ((hi << 8) | lo) & mask;
            // = seg000:e404 cmp ax,bx; ja loc_0e3f2.
            if val <= max {
                return val;
            }
        }
    }
}
