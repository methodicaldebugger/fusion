//! Exact integer support for Fusion's fixed-width integer types.
//!
//! The interpreter keeps every integer that fits in `i64` in `Value::Number`.
//! Integers outside that range (for example `u64` values above `i64::MAX`, or
//! any `i128` / `u128` value that does not fit in 64 bits) are stored as
//! [`WideInt`], a sign-and-magnitude integer that can represent every value of
//! every fixed-width integer type, from `i128::MIN` up to `u128::MAX`.
//!
//! [`IntKind`] describes the fixed-width integer types themselves (bit width,
//! signedness and the inclusive range of representable values).

use std::cmp::Ordering;
use std::fmt;

/// Magnitude of `i128::MIN`, the largest magnitude a negative value may have.
const NEGATIVE_LIMIT: u128 = 1u128 << 127;

/// An exact integer in the range `-2^127 ..= 2^128 - 1`.
///
/// Invariant: zero is never negative, and a negative value never has a
/// magnitude above `2^127`. All constructors preserve this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WideInt {
    negative: bool,
    magnitude: u128,
}

impl WideInt {
    /// Builds a value from a sign and magnitude. Returns `None` when the value
    /// is below `-2^127` (the magnitude does not fit a negative number).
    pub fn new(negative: bool, magnitude: u128) -> Option<WideInt> {
        if magnitude == 0 {
            return Some(WideInt {
                negative: false,
                magnitude: 0,
            });
        }
        if negative && magnitude > NEGATIVE_LIMIT {
            return None;
        }
        Some(WideInt {
            negative,
            magnitude,
        })
    }

    pub fn from_i64(value: i64) -> WideInt {
        WideInt {
            negative: value < 0,
            magnitude: value.unsigned_abs() as u128,
        }
    }

    pub fn from_i128(value: i128) -> WideInt {
        WideInt {
            negative: value < 0,
            magnitude: value.unsigned_abs(),
        }
    }

    pub fn from_u128(value: u128) -> WideInt {
        WideInt {
            negative: false,
            magnitude: value,
        }
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    pub fn is_zero(&self) -> bool {
        self.magnitude == 0
    }

    pub fn magnitude(&self) -> u128 {
        self.magnitude
    }

    pub fn to_i128(self) -> Option<i128> {
        if self.negative {
            // The magnitude is at most 2^127; `as i128` wraps 2^127 to
            // i128::MIN, whose wrapping negation is itself, which is correct.
            Some((self.magnitude as i128).wrapping_neg())
        } else {
            i128::try_from(self.magnitude).ok()
        }
    }

    pub fn to_i64(self) -> Option<i64> {
        self.to_i128().and_then(|value| i64::try_from(value).ok())
    }

    pub fn to_u128(self) -> Option<u128> {
        if self.negative {
            None
        } else {
            Some(self.magnitude)
        }
    }

    pub fn to_f64(self) -> f64 {
        let magnitude = self.magnitude as f64;
        if self.negative {
            -magnitude
        } else {
            magnitude
        }
    }

    pub fn to_f32(self) -> f32 {
        let magnitude = self.magnitude as f32;
        if self.negative {
            -magnitude
        } else {
            magnitude
        }
    }

    /// Converts a float that holds a whole number. Returns `None` for NaN,
    /// infinities, fractional values and values outside `-2^127 ..= 2^128 - 1`.
    pub fn from_f64(value: f64) -> Option<WideInt> {
        if !value.is_finite() || value.fract() != 0.0 {
            return None;
        }
        if value >= 0.0 {
            // u128::MAX as f64 rounds up to exactly 2^128, which is not representable.
            if value >= u128::MAX as f64 {
                return None;
            }
            Some(WideInt::from_u128(value as u128))
        } else {
            // -2^127 is representable exactly as an f64.
            if value < -(NEGATIVE_LIMIT as f64) {
                return None;
            }
            Some(WideInt::from_i128(value as i128))
        }
    }

    fn negated_unchecked(self) -> WideInt {
        WideInt {
            negative: !self.negative && self.magnitude != 0,
            magnitude: self.magnitude,
        }
    }

    pub fn checked_neg(self) -> Option<WideInt> {
        WideInt::new(!self.negative, self.magnitude)
    }

    pub fn checked_add(self, other: WideInt) -> Option<WideInt> {
        if self.negative == other.negative {
            let magnitude = self.magnitude.checked_add(other.magnitude)?;
            WideInt::new(self.negative, magnitude)
        } else if self.magnitude >= other.magnitude {
            WideInt::new(self.negative, self.magnitude - other.magnitude)
        } else {
            WideInt::new(other.negative, other.magnitude - self.magnitude)
        }
    }

    pub fn checked_sub(self, other: WideInt) -> Option<WideInt> {
        self.checked_add(other.negated_unchecked())
    }

    pub fn checked_mul(self, other: WideInt) -> Option<WideInt> {
        let magnitude = self.magnitude.checked_mul(other.magnitude)?;
        WideInt::new(self.negative != other.negative, magnitude)
    }

    /// Division truncating toward zero, matching Fusion's `i64` division.
    /// Returns `None` on division by zero or when the quotient is out of range.
    pub fn checked_div(self, other: WideInt) -> Option<WideInt> {
        if other.magnitude == 0 {
            return None;
        }
        WideInt::new(
            self.negative != other.negative,
            self.magnitude / other.magnitude,
        )
    }

    /// Parses an optional sign followed by decimal digits.
    pub fn parse(text: &str) -> Option<WideInt> {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let magnitude = digits.parse::<u128>().ok()?;
        WideInt::new(negative, magnitude)
    }
}

impl Ord for WideInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.magnitude.cmp(&other.magnitude),
            (true, true) => other.magnitude.cmp(&self.magnitude),
        }
    }
}

impl PartialOrd for WideInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for WideInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            write!(f, "-")?;
        }
        write!(f, "{}", self.magnitude)
    }
}

/// The fixed-width integer types of Fusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntKind {
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

impl IntKind {
    pub fn from_name(name: &str) -> Option<IntKind> {
        match name {
            "i8" => Some(IntKind::I8),
            "i16" => Some(IntKind::I16),
            "i32" => Some(IntKind::I32),
            "i64" => Some(IntKind::I64),
            "i128" => Some(IntKind::I128),
            "isize" => Some(IntKind::Isize),
            "u8" => Some(IntKind::U8),
            "u16" => Some(IntKind::U16),
            "u32" => Some(IntKind::U32),
            "u64" => Some(IntKind::U64),
            "u128" => Some(IntKind::U128),
            "usize" => Some(IntKind::Usize),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IntKind::I8 => "i8",
            IntKind::I16 => "i16",
            IntKind::I32 => "i32",
            IntKind::I64 => "i64",
            IntKind::I128 => "i128",
            IntKind::Isize => "isize",
            IntKind::U8 => "u8",
            IntKind::U16 => "u16",
            IntKind::U32 => "u32",
            IntKind::U64 => "u64",
            IntKind::U128 => "u128",
            IntKind::Usize => "usize",
        }
    }

    /// Bit width of the type. `isize` / `usize` are 64 bits wide, which is the
    /// ABI width Fusion targets.
    pub fn bits(self) -> u32 {
        match self {
            IntKind::I8 | IntKind::U8 => 8,
            IntKind::I16 | IntKind::U16 => 16,
            IntKind::I32 | IntKind::U32 => 32,
            IntKind::I64 | IntKind::U64 | IntKind::Isize | IntKind::Usize => 64,
            IntKind::I128 | IntKind::U128 => 128,
        }
    }

    pub fn is_signed(self) -> bool {
        matches!(
            self,
            IntKind::I8
                | IntKind::I16
                | IntKind::I32
                | IntKind::I64
                | IntKind::I128
                | IntKind::Isize
        )
    }

    /// Smallest representable value.
    pub fn min(self) -> WideInt {
        if self.is_signed() {
            WideInt {
                negative: true,
                magnitude: 1u128 << (self.bits() - 1),
            }
        } else {
            WideInt {
                negative: false,
                magnitude: 0,
            }
        }
    }

    /// Largest representable value.
    pub fn max(self) -> WideInt {
        let magnitude = if self.is_signed() {
            (1u128 << (self.bits() - 1)) - 1
        } else if self.bits() == 128 {
            u128::MAX
        } else {
            (1u128 << self.bits()) - 1
        };
        WideInt {
            negative: false,
            magnitude,
        }
    }

    pub fn contains(self, value: WideInt) -> bool {
        value >= self.min() && value <= self.max()
    }
}
