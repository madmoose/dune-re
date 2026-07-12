use std::ops::{Add, Div, Mul};

#[derive(Debug, Copy, Clone, Default)]
pub struct FixedU16F16(pub u32);

impl FixedU16F16 {
    pub const HALF: FixedU16F16 = FixedU16F16(0x0000_8000);

    pub const fn from_u16_u16(int: u16, frac: u16) -> FixedU16F16 {
        FixedU16F16(((int as u32) << 16) | (frac as u32))
    }

    pub const fn new_integer(value: u16) -> FixedU16F16 {
        FixedU16F16((value as u32) << 16)
    }

    pub const fn int_part(self) -> u16 {
        (self.0 >> 16) as u16
    }

    pub const fn fract(self) -> FixedU16F16 {
        Self(self.0 & 0x0000_ffff)
    }

    pub const fn floor(self) -> FixedU16F16 {
        Self(self.0 & 0xffff_0000)
    }
}

impl Add for FixedU16F16 {
    type Output = FixedU16F16;

    fn add(self, other: FixedU16F16) -> FixedU16F16 {
        FixedU16F16(self.0.wrapping_add(other.0))
    }
}

impl Mul<u16> for FixedU16F16 {
    type Output = FixedU16F16;

    fn mul(self, rhs: u16) -> FixedU16F16 {
        FixedU16F16(self.0.wrapping_mul(rhs as u32))
    }
}

impl Mul<FixedU16F16> for u32 {
    type Output = FixedU16F16;

    fn mul(self, rhs: FixedU16F16) -> FixedU16F16 {
        FixedU16F16(self.wrapping_mul(rhs.0))
    }
}

impl Div<u32> for FixedU16F16 {
    type Output = FixedU16F16;

    fn div(self, rhs: u32) -> FixedU16F16 {
        FixedU16F16(self.0 / rhs)
    }
}
