//! x87 state and arithmetic on an `f64` model.
//!
//! Registers hold `f64`, not the 80-bit format of the hardware. This is exact
//! for the way the game uses the FPU:
//!
//! * With precision control set to double (the C runtime's default), every
//!   `+ - * / sqrt` result is rounded to 53 bits, which is `f64` arithmetic.
//! * With precision control set to single (what Direct3D selects), the result
//!   is rounded to 24 bits. Rounding the `f64` result again to 24 bits gives
//!   the same value, because 53 >= 2 * 24 + 2 makes the double rounding
//!   harmless. The exponent range stays that of the wider format, as on the
//!   hardware, so the rounding is done on the mantissa and not through `f32`.
//!
//! All four rounding modes are honoured exactly for `+ - * / sqrt`, stores and
//! integer conversions: the round-to-nearest result is corrected by one unit
//! in the last place using the sign of the exact rounding error.
//!
//! Not exact: extended precision (control word asking for 64 bits), integers
//! beyond 2^53 loaded with `fild`, the transcendental functions (last-digit
//! differences), results outside the `f64` exponent range, and `fprem` on
//! operands so far apart that the hardware would reduce in several steps.

/// Format of a memory operand of an x87 instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FMem {
    F32,
    F64,
    F80,
    I16,
    I32,
    I64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FBin {
    Add,
    Sub,
    Mul,
    Div,
    /// `atan2(a, b)`
    Atan2,
    /// `a * log2(b)`
    Yl2x,
    /// `a * 2^trunc(b)`
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FUn {
    Abs,
    Neg,
    Sqrt,
    Sin,
    Cos,
    Tan,
    /// Round to an integer in the current rounding mode.
    Round,
    /// `2^a - 1`
    Exp2M1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FConst {
    Zero,
    One,
    Pi,
    Log2E,
    Log10Of2,
    LnOf2,
}

impl FConst {
    pub fn value(self) -> f64 {
        match self {
            FConst::Zero => 0.0,
            FConst::One => 1.0,
            FConst::Pi => std::f64::consts::PI,
            FConst::Log2E => std::f64::consts::LOG2_E,
            FConst::Log10Of2 => std::f64::consts::LOG10_2,
            FConst::LnOf2 => std::f64::consts::LN_2,
        }
    }
}

pub const DEFAULT_CONTROL: u16 = 0x037F;

#[derive(Debug, Clone, PartialEq)]
pub struct Fpu {
    /// Physical registers; `st(i)` is `regs[(top + i) & 7]`.
    pub regs: [f64; 8],
    pub top: u8,
    /// Bit `n` set: physical register `n` holds a value.
    pub valid: u8,
    pub control: u16,
    /// Condition codes C0..C3.
    pub c: [bool; 4],
    /// Set when the model stopped being exact: arithmetic under extended
    /// precision or a directed rounding mode, or a stack fault. Diagnostics.
    pub inexact_model: bool,
}

impl Default for Fpu {
    fn default() -> Self {
        Fpu {
            regs: [0.0; 8],
            top: 0,
            valid: 0,
            control: DEFAULT_CONTROL,
            c: [false; 4],
            inexact_model: false,
        }
    }
}

impl Fpu {
    #[inline]
    fn slot(&self, i: u32) -> usize {
        (self.top as usize + i as usize) & 7
    }

    #[inline]
    pub fn st(&mut self, i: u32) -> f64 {
        let slot = self.slot(i);
        if self.valid & (1 << slot) == 0 {
            // Stack underflow: the hardware produces the default NaN.
            self.inexact_model = true;
            return f64::NAN;
        }
        self.regs[slot]
    }

    #[inline]
    pub fn set_st(&mut self, i: u32, value: f64) {
        let slot = self.slot(i);
        self.regs[slot] = value;
        self.valid |= 1 << slot;
    }

    #[inline]
    pub fn push(&mut self, value: f64) {
        self.top = (self.top + 7) & 7;
        let slot = self.top as usize;
        if self.valid & (1 << slot) != 0 {
            self.inexact_model = true; // stack overflow
        }
        self.regs[slot] = value;
        self.valid |= 1 << slot;
    }

    #[inline]
    pub fn pop(&mut self) {
        self.valid &= !(1 << self.top);
        self.top = (self.top + 1) & 7;
    }

    pub fn depth(&self) -> u32 {
        self.valid.count_ones()
    }

    #[inline]
    pub fn status(&self) -> u16 {
        (self.c[0] as u16) << 8
            | (self.c[1] as u16) << 9
            | (self.c[2] as u16) << 10
            | (self.top as u16) << 11
            | (self.c[3] as u16) << 14
    }

    pub fn init(&mut self) {
        *self = Fpu { inexact_model: self.inexact_model, ..Fpu::default() };
    }

    #[inline]
    pub fn rounding(&self) -> Rounding {
        Rounding::from_control(self.control)
    }

    /// Narrows a result, already rounded to 53 bits in the current mode, to
    /// the precision selected in the control word.
    #[inline]
    fn narrow(&mut self, value: f64) -> f64 {
        match (self.control >> 8) & 3 {
            0 => round_to_24_bits(value, self.rounding()),
            3 => {
                self.inexact_model = true;
                value
            }
            _ => value,
        }
    }

    /// Rounds the result of a library function. These are only as accurate
    /// as the host's maths library, so no directed rounding is attempted.
    fn narrow_approximate(&mut self, value: f64) -> f64 {
        match (self.control >> 8) & 3 {
            0 => round_to_24_bits(value, Rounding::Nearest),
            _ => value,
        }
    }

    #[inline]
    pub fn binary(&mut self, op: FBin, a: f64, b: f64) -> f64 {
        let mode = self.rounding();
        match op {
            FBin::Add => {
                let sum = add_rounded(a, b, mode);
                self.narrow(sum)
            }
            FBin::Sub => {
                let difference = add_rounded(a, -b, mode);
                self.narrow(difference)
            }
            // The error of the nearest result is only wanted for a directed
            // rounding, and costs a fused multiply-add: it is not worked
            // out when rounding to nearest, the mode programs run in.
            FBin::Mul => {
                let nearest = a * b;
                let product = if mode == Rounding::Nearest {
                    nearest
                } else {
                    direct(nearest, a.mul_add(b, -nearest), mode)
                };
                self.narrow(product)
            }
            FBin::Div => {
                let nearest = a / b;
                let quotient = if mode == Rounding::Nearest {
                    nearest
                } else {
                    // a - nearest * b has the sign of (a / b - nearest) times the sign of b.
                    let residual = (-nearest).mul_add(b, a);
                    let error = if b.is_sign_negative() { -residual } else { residual };
                    direct(nearest, error, mode)
                };
                self.narrow(quotient)
            }
            FBin::Atan2 => self.narrow_approximate(a.atan2(b)),
            FBin::Yl2x => self.narrow_approximate(a * b.log2()),
            FBin::Scale => {
                let scaled = scale_by_power_of_two(a, b.trunc().clamp(-5000.0, 5000.0) as i32);
                self.narrow_approximate(scaled)
            }
        }
    }

    pub fn unary(&mut self, op: FUn, a: f64) -> f64 {
        match op {
            // Sign operations are exact and never rounded.
            FUn::Abs => a.abs(),
            FUn::Neg => -a,
            FUn::Sqrt => {
                let (nearest, mode) = (a.sqrt(), self.rounding());
                let root = if mode == Rounding::Nearest {
                    nearest
                } else {
                    // a - nearest^2 has the sign of (sqrt(a) - nearest).
                    direct(nearest, (-nearest).mul_add(nearest, a), mode)
                };
                self.narrow(root)
            }
            FUn::Sin => self.narrow_approximate(a.sin()),
            FUn::Cos => self.narrow_approximate(a.cos()),
            FUn::Tan => self.narrow_approximate(a.tan()),
            FUn::Round => round_to_integer(a, self.control),
            FUn::Exp2M1 => self.narrow_approximate((a * std::f64::consts::LN_2).exp_m1()),
        }
    }

    /// `fcom`: C3 C2 C0 = 000 (a > b), 001 (a < b), 100 (equal), 111 (unordered).
    #[inline]
    pub fn compare(&mut self, a: f64, b: f64) {
        let (c3, c2, c0) = match a.partial_cmp(&b) {
            Some(std::cmp::Ordering::Greater) => (false, false, false),
            Some(std::cmp::Ordering::Less) => (false, false, true),
            Some(std::cmp::Ordering::Equal) => (true, false, false),
            None => (true, true, true),
        };
        self.c = [c0, false, c2, c3];
    }

    /// `fxam`: classifies `st(0)`.
    pub fn examine(&mut self) {
        let slot = self.slot(0);
        let value = self.regs[slot];
        let (c3, c2, c0) = if self.valid & (1 << slot) == 0 {
            (true, false, true)
        } else if value.is_nan() {
            (false, false, true)
        } else if value.is_infinite() {
            (false, true, true)
        } else if value == 0.0 {
            (true, false, false)
        } else if value.is_subnormal() {
            (true, true, false)
        } else {
            (false, true, false)
        };
        self.c = [c0, value.is_sign_negative(), c2, c3];
    }

    /// `fprem` (truncating) and `fprem1` (round to nearest): `st0 %= st1`.
    pub fn remainder(&mut self, nearest: bool) {
        let (a, b) = (self.st(0), self.st(1));
        let mut rem = a % b;
        let mut quotient = ((a - rem) / b).abs();
        if nearest && rem.is_finite() && b.is_finite() {
            let half = (b / 2.0).abs();
            let odd = quotient % 2.0 == 1.0;
            if rem.abs() > half || (rem.abs() == half && odd) {
                rem -= b.abs().copysign(rem);
                quotient += 1.0;
            }
        }
        let low = if quotient.is_finite() { (quotient.round() % 8.0) as u32 } else { 0 };
        // C0 C3 C1 hold quotient bits 2, 1, 0; C2 clear means "reduction complete".
        self.c = [low & 4 != 0, low & 1 != 0, false, low & 2 != 0];
        self.set_st(0, rem);
    }
}

/// The rounding-control field of the control word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    Nearest,
    Down,
    Up,
    Zero,
}

impl Rounding {
    pub fn from_control(control: u16) -> Rounding {
        match (control >> 10) & 3 {
            0 => Rounding::Nearest,
            1 => Rounding::Down,
            2 => Rounding::Up,
            _ => Rounding::Zero,
        }
    }
}

/// Turns a round-to-nearest result into the result of a directed rounding.
///
/// `error` only needs the sign of (exact result - `nearest`); the callers get
/// it from an error-free transformation, so the outcome is correctly rounded.
fn direct(nearest: f64, error: f64, mode: Rounding) -> f64 {
    if mode == Rounding::Nearest || error == 0.0 || !nearest.is_finite() || !error.is_finite() {
        return nearest;
    }
    let exact_is_above = error > 0.0;
    let step_up = match mode {
        Rounding::Up => exact_is_above,
        Rounding::Zero => exact_is_above && nearest < 0.0,
        _ => false,
    };
    let step_down = match mode {
        Rounding::Down => !exact_is_above,
        Rounding::Zero => !exact_is_above && nearest > 0.0,
        _ => false,
    };
    if step_up {
        nearest.next_up()
    } else if step_down {
        nearest.next_down()
    } else {
        nearest
    }
}

fn add_rounded(a: f64, b: f64, mode: Rounding) -> f64 {
    let sum = a + b;
    if mode == Rounding::Nearest {
        return sum;
    }
    if sum == 0.0 && mode == Rounding::Down && a.is_sign_negative() != b.is_sign_negative() {
        // An exact zero from operands of opposite signs is negative when
        // rounding down, positive in every other mode.
        return -0.0;
    }
    // Knuth's two-sum: `error` is exactly (a + b) - sum.
    let b_virtual = sum - a;
    let error = (a - (sum - b_virtual)) + (b - b_virtual);
    direct(sum, error, mode)
}

/// Rounds to 24 significant bits, keeping the `f64` exponent.
pub fn round_to_24_bits(value: f64, mode: Rounding) -> f64 {
    if !value.is_finite() || value == 0.0 || value.is_subnormal() {
        return value;
    }
    const DROPPED: u32 = 52 - 23;
    let bits = value.to_bits();
    let half = 1u64 << (DROPPED - 1);
    let fraction = bits & ((1 << DROPPED) - 1);
    let mut kept = bits >> DROPPED;
    let away_from_zero = match mode {
        Rounding::Nearest => fraction > half || (fraction == half && kept & 1 == 1),
        Rounding::Zero => false,
        Rounding::Down => fraction != 0 && value < 0.0,
        Rounding::Up => fraction != 0 && value > 0.0,
    };
    if away_from_zero {
        kept += 1; // a carry out of the mantissa correctly bumps the exponent
    }
    f64::from_bits(kept << DROPPED)
}

/// Converts to `f32` as `fst dword` does, in the given rounding mode.
pub fn to_f32(value: f64, mode: Rounding) -> f32 {
    let nearest = value as f32;
    if nearest.is_nan() || nearest as f64 == value {
        return nearest;
    }
    let exact_is_above = value > nearest as f64;
    match mode {
        Rounding::Nearest => nearest,
        Rounding::Up if exact_is_above => nearest.next_up(),
        Rounding::Down if !exact_is_above => nearest.next_down(),
        Rounding::Zero if exact_is_above && value < 0.0 => nearest.next_up(),
        Rounding::Zero if !exact_is_above && value > 0.0 => nearest.next_down(),
        _ => nearest,
    }
}

/// Rounds to an integral value in the mode selected by the control word.
pub fn round_to_integer(value: f64, control: u16) -> f64 {
    match (control >> 10) & 3 {
        0 => value.round_ties_even(),
        1 => value.floor(),
        2 => value.ceil(),
        _ => value.trunc(),
    }
}

/// Converts to a signed integer of `bits` width, as `fistp` does. Values that
/// do not fit give the "integer indefinite" pattern (only the sign bit set).
pub fn to_integer(value: f64, control: u16, bits: u32) -> u64 {
    let rounded = round_to_integer(value, control);
    let limit = 2f64.powi(bits as i32 - 1);
    let indefinite = 1u64 << (bits - 1);
    if rounded.is_nan() || rounded >= limit || rounded < -limit {
        return indefinite;
    }
    (rounded as i64 as u64) & (u64::MAX >> (64 - bits))
}

/// Decodes the 80-bit extended format, rounding to nearest `f64`.
pub fn f80_to_f64(mantissa: u64, sign_exponent: u16) -> f64 {
    let negative = sign_exponent & 0x8000 != 0;
    let exponent = (sign_exponent & 0x7FFF) as i32;
    let magnitude = if exponent == 0x7FFF {
        if mantissa << 1 == 0 { f64::INFINITY } else { f64::NAN }
    } else if mantissa == 0 {
        0.0
    } else {
        // value = mantissa * 2^(exponent - 16383 - 63); `as f64` rounds to nearest.
        scale_by_power_of_two(mantissa as f64, exponent.max(1) - 16383 - 63)
    };
    if negative { -magnitude } else { magnitude }
}

/// Encodes an `f64` in the 80-bit extended format (always exact).
pub fn f64_to_f80(value: f64) -> (u64, u16) {
    let bits = value.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exponent = ((bits >> 52) & 0x7FF) as i32;
    let fraction = bits & ((1 << 52) - 1);
    if exponent == 0x7FF {
        let payload = if fraction == 0 { 0 } else { fraction << 11 | 1 << 62 };
        return (1 << 63 | payload, sign | 0x7FFF);
    }
    if exponent == 0 && fraction == 0 {
        return (0, sign);
    }
    let (mut mantissa, mut unbiased) =
        if exponent == 0 { (fraction << 11, -1022) } else { ((fraction | 1 << 52) << 11, exponent - 1023) };
    let shift = mantissa.leading_zeros();
    mantissa <<= shift;
    unbiased -= shift as i32;
    (mantissa, sign | (unbiased + 16383) as u16)
}

/// `value * 2^power`, exact unless the result leaves the `f64` range.
fn scale_by_power_of_two(mut value: f64, mut power: i32) -> f64 {
    // In steps, so that no intermediate factor overflows to infinity (which
    // would turn a zero into a NaN) or underflows to zero.
    while power != 0 && value != 0.0 && value.is_finite() {
        let step = power.clamp(-1000, 1000);
        value *= 2f64.powi(step);
        power -= step;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_precision_rounding_matches_f32_inside_its_range() {
        for value in [1.0 / 3.0, 16777217.0, 0.1, -123456.789, 1e-30, 3.5e37, f64::from(f32::MAX)] {
            assert_eq!(round_to_24_bits(value, Rounding::Nearest), value as f32 as f64, "{value}");
        }
        // Outside the f32 range the exponent is kept instead of overflowing.
        let big = round_to_24_bits(1e300 / 3.0, Rounding::Nearest);
        assert!(big.is_finite() && (big - 1e300 / 3.0).abs() < 1e293);
        let tie = 1.0 + 2f64.powi(-24);
        assert_eq!(round_to_24_bits(tie, Rounding::Nearest), 1.0); // tie to even
        assert_eq!(round_to_24_bits(1.0 + 3.0 * 2f64.powi(-24), Rounding::Nearest), 1.0 + 2f64.powi(-22));
        assert_eq!(round_to_24_bits(tie, Rounding::Up), 1.0 + 2f64.powi(-23));
        assert_eq!(round_to_24_bits(-tie, Rounding::Up), -1.0);
        assert_eq!(round_to_24_bits(-tie, Rounding::Down), -1.0 - 2f64.powi(-23));
        assert_eq!(round_to_24_bits(-tie, Rounding::Zero), -1.0);
    }

    #[test]
    fn directed_rounding_is_exact() {
        let chop = Fpu { control: 0x0E7F, ..Fpu::default() };
        let up = Fpu { control: 0x0A7F, ..Fpu::default() };
        let down = Fpu { control: 0x067F, ..Fpu::default() };
        // The double nearest to 1/3 lies below 1/3.
        let third = 1.0f64 / 3.0;
        assert_eq!(chop.clone().binary(FBin::Div, 1.0, 3.0), third);
        assert_eq!(up.clone().binary(FBin::Div, 1.0, 3.0), third.next_up());
        assert_eq!(down.clone().binary(FBin::Div, -1.0, 3.0), (-third).next_down());
        // 0.1 + 0.2 rounds up to 0.30000000000000004 in nearest mode.
        assert_eq!(chop.clone().binary(FBin::Add, 0.1, 0.2), (0.1f64 + 0.2).next_down());
        assert_eq!(up.clone().binary(FBin::Add, 0.1, 0.2), 0.1 + 0.2);
        assert!(down.clone().binary(FBin::Sub, 2.5, 2.5).is_sign_negative());
        assert!(chop.clone().binary(FBin::Sub, 2.5, 2.5).is_sign_positive());
        // Exact results are never moved.
        assert_eq!(chop.clone().binary(FBin::Mul, 1.5, 2.0), 3.0);
        assert_eq!(up.clone().unary(FUn::Sqrt, 9.0), 3.0);
        assert_eq!(chop.clone().unary(FUn::Sqrt, 7.0), 7f64.sqrt().next_down());
        assert_eq!(to_f32(0.1, Rounding::Zero), 0.1f32.next_down());
        assert_eq!(to_f32(1e39, Rounding::Zero), f32::MAX);
        assert_eq!(to_f32(1e39, Rounding::Nearest), f32::INFINITY);
    }

    #[test]
    fn integer_conversion_follows_the_rounding_mode() {
        let nearest = 0x037F;
        let chop = 0x0F7F;
        assert_eq!(to_integer(2.5, nearest, 32), 2);
        assert_eq!(to_integer(3.5, nearest, 32), 4);
        assert_eq!(to_integer(-2.7, chop, 32), (-2i32) as u32 as u64);
        assert_eq!(to_integer(-2.7, 0x077F, 32), (-3i32) as u32 as u64);
        assert_eq!(to_integer(3e9, nearest, 32), 0x8000_0000);
        assert_eq!(to_integer(f64::NAN, nearest, 16), 0x8000);
        assert_eq!(to_integer(-1.0, nearest, 64), u64::MAX);
    }

    #[test]
    fn extended_format_round_trips() {
        for value in [0.0, -0.0, 1.0, -2.5, 1e-310, f64::MAX, f64::MIN_POSITIVE, f64::INFINITY] {
            let (mantissa, sign_exponent) = f64_to_f80(value);
            assert_eq!(f80_to_f64(mantissa, sign_exponent).to_bits(), value.to_bits(), "{value}");
        }
        assert_eq!(f64_to_f80(1.0), (1 << 63, 16383));
        assert!(f80_to_f64(f64_to_f80(f64::NAN).0, 0x7FFF).is_nan());
    }

    /// `fprem`/`fprem1` against integer arithmetic: operands are multiples of
    /// 2^-10, so the exact remainder and quotient are computable in `i64`.
    #[test]
    fn remainders_match_exact_integer_arithmetic() {
        let mut state = 0x243F_6A88_85A3_08D3u64;
        let mut next = |bound: i64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % (2 * bound as u64 + 1)) as i64 - bound
        };
        for case in 0..20_000 {
            let x = next(if case % 2 == 0 { 1 << 40 } else { 5000 });
            let y = match next(if case % 3 == 0 { 1 << 20 } else { 3000 }) {
                0 => 7,
                y => y,
            };
            for nearest in [false, true] {
                let mut quotient = (x / y).abs();
                let mut remainder = x % y;
                if nearest
                    && (2 * remainder.abs() > y.abs()
                        || (2 * remainder.abs() == y.abs() && quotient % 2 == 1))
                {
                    remainder -= y.abs() * remainder.signum();
                    quotient += 1;
                }
                let mut fpu = Fpu::default();
                fpu.push(y as f64 / 1024.0);
                fpu.push(x as f64 / 1024.0);
                fpu.remainder(nearest);
                assert_eq!(fpu.st(0), remainder as f64 / 1024.0, "{x} rem {y}, nearest {nearest}");
                let low = quotient as u32 & 7;
                assert_eq!(fpu.c, [low & 4 != 0, low & 1 != 0, false, low & 2 != 0], "{x} rem {y}");
            }
        }
    }

    #[test]
    fn stack_and_status_word() {
        let mut fpu = Fpu::default();
        fpu.push(1.0);
        fpu.push(2.0);
        assert_eq!((fpu.st(0), fpu.st(1), fpu.depth()), (2.0, 1.0, 2));
        assert_eq!(fpu.status() >> 11 & 7, 6);
        fpu.compare(1.0, 2.0);
        assert_eq!(fpu.status() & 0x4500, 0x0100);
        fpu.compare(f64::NAN, 2.0);
        assert_eq!(fpu.status() & 0x4500, 0x4500);
        fpu.pop();
        assert_eq!(fpu.st(0), 1.0);
        fpu.set_st(0, 7.5);
        fpu.push(2.0);
        fpu.remainder(false);
        assert_eq!(fpu.st(0), 2.0);
        fpu.set_st(0, 7.5);
        fpu.set_st(1, 2.0);
        fpu.remainder(false);
        assert_eq!(fpu.st(0), 1.5);
        assert_eq!(fpu.c, [false, true, false, true]); // quotient 3
    }
}
