//! # Arbitrary-Precision Decimal Mathematics
//!
//! Provides exact base-10 decimal arithmetic for financial and high-precision calculations,
//! avoiding binary IEEE-754 floating-point inaccuracies (e.g. `0.1 + 0.2 == 0.3` instead of `0.30000000000000004`).

use std::cmp::Ordering;
use std::fmt;

/// An arbitrary-precision decimal number with exact digits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimal {
    pub negative: bool,
    /// Digits before decimal point, most-significant first. Always at least `[0]`.
    pub integral: Vec<u8>,
    /// Digits after decimal point, most-significant first. Trailing zeros stripped on normalization.
    pub fractional: Vec<u8>,
}

impl Decimal {
    /// Zero representation.
    pub fn zero() -> Self {
        Self {
            negative: false,
            integral: vec![0],
            fractional: Vec::new(),
        }
    }

    /// Check if this decimal represents zero.
    pub fn is_zero(&self) -> bool {
        self.integral.iter().all(|&d| d == 0) && self.fractional.iter().all(|&d| d == 0)
    }

    /// Returns absolute value.
    pub fn abs(&self) -> Self {
        let mut res = self.clone();
        res.negative = false;
        res
    }

    /// Parse a decimal from string representation.
    pub fn from_str_exact(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }

        let (negative, rest) = if let Some(stripped) = trimmed.strip_prefix('-') {
            (true, stripped.trim_start())
        } else if let Some(stripped) = trimmed.strip_prefix('+') {
            (false, stripped.trim_start())
        } else {
            (false, trimmed)
        };

        if rest.is_empty() {
            return None;
        }

        // Handle scientific notation e.g. "1.5e-3"
        if let Some((mantissa, exp_str)) = rest.split_once(['e', 'E']) {
            let exp: i32 = exp_str.parse().ok()?;
            let mut base = Self::from_str_exact(mantissa)?;
            base.shift_decimal(exp);
            if negative {
                base.negative = !base.negative;
            }
            return Some(base);
        }

        let (int_part, frac_part) = match rest.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (rest, None),
        };

        let mut integral: Vec<u8> = Vec::new();
        for ch in int_part.chars() {
            if let Some(digit) = ch.to_digit(10) {
                integral.push(digit as u8);
            } else {
                return None;
            }
        }

        let mut fractional: Vec<u8> = Vec::new();
        if let Some(f_str) = frac_part {
            for ch in f_str.chars() {
                if let Some(digit) = ch.to_digit(10) {
                    fractional.push(digit as u8);
                } else {
                    return None;
                }
            }
        }

        let mut dec = Self {
            negative,
            integral,
            fractional,
        };
        dec.normalize();
        Some(dec)
    }

    pub fn from_f64(val: f64) -> Option<Self> {
        if val.is_nan() || val.is_infinite() {
            return None;
        }
        Self::from_str_exact(&val.to_string())
    }

    pub fn from_f32(val: f32) -> Option<Self> {
        if val.is_nan() || val.is_infinite() {
            return None;
        }
        Self::from_str_exact(&val.to_string())
    }

    pub fn from_i64(val: i64) -> Self {
        Self::from_str_exact(&val.to_string()).unwrap_or_else(Self::zero)
    }

    pub fn from_php_value(val: &crate::vm::value::PhpValue) -> Option<Self> {
        match val {
            crate::vm::value::PhpValue::Float(f) => Self::from_f32(*f),
            crate::vm::value::PhpValue::RustFloat { value, .. } => Self::from_f64(*value),
            crate::vm::value::PhpValue::Int(n) => Some(Self::from_i64(*n as i64)),
            crate::vm::value::PhpValue::RustInt { value, .. } => Self::from_str_exact(&value.to_string()),
            crate::vm::value::PhpValue::String(s) => Self::from_str_exact(s.as_str()),
            _ => None,
        }
    }

    /// Shift the decimal point by `shift` places (positive = multiply by 10, negative = divide by 10).
    pub fn shift_decimal(&mut self, shift: i32) {
        if self.is_zero() || shift == 0 {
            return;
        }

        if shift > 0 {
            let count = shift as usize;
            for _ in 0..count {
                if !self.fractional.is_empty() {
                    let d = self.fractional.remove(0);
                    self.integral.push(d);
                } else {
                    self.integral.push(0);
                }
            }
        } else {
            let count = (-shift) as usize;
            for _ in 0..count {
                let d = self.integral.pop().unwrap_or(0);
                self.fractional.insert(0, d);
            }
        }
        self.normalize();
    }

    /// Strip leading zeros from integral and trailing zeros from fractional.
    pub fn normalize(&mut self) {
        // Strip leading zeros from integral
        while self.integral.len() > 1 && self.integral[0] == 0 {
            self.integral.remove(0);
        }
        if self.integral.is_empty() {
            self.integral.push(0);
        }

        // Strip trailing zeros from fractional
        while let Some(&last) = self.fractional.last() {
            if last == 0 {
                self.fractional.pop();
            } else {
                break;
            }
        }

        // -0 is normalized to 0
        if self.is_zero() {
            self.negative = false;
        }
    }

    /// Converts to standard `f64`.
    pub fn to_f64(&self) -> f64 {
        self.to_string().parse::<f64>().unwrap_or(0.0)
    }

    /// Converts to standard `f32`.
    pub fn to_f32(&self) -> f32 {
        self.to_string().parse::<f32>().unwrap_or(0.0)
    }

    /// Compare absolute values.
    pub fn cmp_abs(&self, other: &Self) -> Ordering {
        if self.integral.len() != other.integral.len() {
            return self.integral.len().cmp(&other.integral.len());
        }
        for (a, b) in self.integral.iter().zip(other.integral.iter()) {
            if a != b {
                return a.cmp(b);
            }
        }
        let max_frac = self.fractional.len().max(other.fractional.len());
        for i in 0..max_frac {
            let a = self.fractional.get(i).copied().unwrap_or(0);
            let b = other.fractional.get(i).copied().unwrap_or(0);
            if a != b {
                return a.cmp(&b);
            }
        }
        Ordering::Equal
    }

    /// Decimal addition.
    pub fn add(&self, other: &Self) -> Self {
        if self.negative != other.negative {
            if self.negative {
                return other.sub(&self.abs());
            } else {
                return self.sub(&other.abs());
            }
        }

        let max_frac = self.fractional.len().max(other.fractional.len());
        let mut carry = 0u8;
        let mut result_frac = vec![0u8; max_frac];

        // Add fractional parts from right to left
        for i in (0..max_frac).rev() {
            let d1 = self.fractional.get(i).copied().unwrap_or(0);
            let d2 = other.fractional.get(i).copied().unwrap_or(0);
            let sum = d1 + d2 + carry;
            result_frac[i] = sum % 10;
            carry = sum / 10;
        }

        // Add integral parts from right to left
        let mut result_int = Vec::new();
        let mut i1 = self.integral.len() as isize - 1;
        let mut i2 = other.integral.len() as isize - 1;

        while i1 >= 0 || i2 >= 0 || carry > 0 {
            let d1 = if i1 >= 0 { self.integral[i1 as usize] } else { 0 };
            let d2 = if i2 >= 0 { other.integral[i2 as usize] } else { 0 };
            let sum = d1 + d2 + carry;
            result_int.push(sum % 10);
            carry = sum / 10;
            i1 -= 1;
            i2 -= 1;
        }

        result_int.reverse();
        let mut res = Self {
            negative: self.negative,
            integral: result_int,
            fractional: result_frac,
        };
        res.normalize();
        res
    }

    /// Decimal subtraction.
    pub fn sub(&self, other: &Self) -> Self {
        if other.negative {
            return self.add(&other.abs());
        }
        if self.negative {
            let mut sum = self.abs().add(other);
            sum.negative = true;
            sum.normalize();
            return sum;
        }

        // Both are positive
        match self.cmp_abs(other) {
            Ordering::Equal => Self::zero(),
            Ordering::Less => {
                let mut res = other.sub(self);
                res.negative = true;
                res
            }
            Ordering::Greater => {
                let max_frac = self.fractional.len().max(other.fractional.len());
                let mut borrow = 0i8;
                let mut result_frac = vec![0u8; max_frac];

                for i in (0..max_frac).rev() {
                    let d1 = self.fractional.get(i).copied().unwrap_or(0) as i8;
                    let d2 = other.fractional.get(i).copied().unwrap_or(0) as i8;
                    let mut diff = d1 - d2 - borrow;
                    if diff < 0 {
                        diff += 10;
                        borrow = 1;
                    } else {
                        borrow = 0;
                    }
                    result_frac[i] = diff as u8;
                }

                let mut result_int = Vec::new();
                let mut i1 = self.integral.len() as isize - 1;
                let mut i2 = other.integral.len() as isize - 1;

                while i1 >= 0 || i2 >= 0 {
                    let d1 = if i1 >= 0 { self.integral[i1 as usize] as i8 } else { 0 };
                    let d2 = if i2 >= 0 { other.integral[i2 as usize] as i8 } else { 0 };
                    let mut diff = d1 - d2 - borrow;
                    if diff < 0 {
                        diff += 10;
                        borrow = 1;
                    } else {
                        borrow = 0;
                    }
                    result_int.push(diff as u8);
                    i1 -= 1;
                    i2 -= 1;
                }

                result_int.reverse();
                let mut res = Self {
                    negative: false,
                    integral: result_int,
                    fractional: result_frac,
                };
                res.normalize();
                res
            }
        }
    }

    /// Decimal multiplication.
    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }

        let mut a_digits = self.integral.clone();
        a_digits.extend_from_slice(&self.fractional);

        let mut b_digits = other.integral.clone();
        b_digits.extend_from_slice(&other.fractional);

        let total_scale = self.fractional.len() + other.fractional.len();

        let mut prod = vec![0u32; a_digits.len() + b_digits.len()];
        for (i, &da) in a_digits.iter().rev().enumerate() {
            for (j, &db) in b_digits.iter().rev().enumerate() {
                prod[i + j] += (da as u32) * (db as u32);
            }
        }

        let mut carry = 0u32;
        let mut final_digits = Vec::with_capacity(prod.len() + 1);
        for val in prod {
            let sum = val + carry;
            final_digits.push((sum % 10) as u8);
            carry = sum / 10;
        }
        while carry > 0 {
            final_digits.push((carry % 10) as u8);
            carry /= 10;
        }

        final_digits.reverse();

        // Split at total_scale
        let (integral, fractional) = if final_digits.len() <= total_scale {
            let missing = total_scale - final_digits.len();
            let mut frac = vec![0u8; missing];
            frac.extend(final_digits);
            (vec![0], frac)
        } else {
            let split_idx = final_digits.len() - total_scale;
            (
                final_digits[..split_idx].to_vec(),
                final_digits[split_idx..].to_vec(),
            )
        };

        let mut res = Self {
            negative: self.negative ^ other.negative,
            integral,
            fractional,
        };
        res.normalize();
        res
    }

    /// Decimal division with precision scale.
    pub fn div(&self, other: &Self, scale: Option<usize>) -> Result<Self, ()> {
        if other.is_zero() {
            return Err(());
        }
        if self.is_zero() {
            return Ok(Self::zero());
        }

        let target_scale = scale.unwrap_or(16);

        // Convert operands to integer vectors
        let mut a_digits = self.integral.clone();
        a_digits.extend_from_slice(&self.fractional);

        let mut b_digits = other.integral.clone();
        b_digits.extend_from_slice(&other.fractional);

        // Normalize leading zeros in b_digits
        while b_digits.len() > 1 && b_digits[0] == 0 {
            b_digits.remove(0);
        }

        let scale_a = self.fractional.len();
        let scale_b = other.fractional.len();

        // To produce quotient with target_scale fractional digits:
        // shift_a = scale_b + target_scale - scale_a
        let shift = (scale_b + target_scale) as isize - scale_a as isize;
        if shift > 0 {
            a_digits.extend(vec![0u8; shift as usize]);
        } else if shift < 0 {
            b_digits.extend(vec![0u8; (-shift) as usize]);
        }

        // Long division of a_digits by b_digits
        let mut quotient: Vec<u8> = Vec::with_capacity(a_digits.len());
        let mut remainder: Vec<u8> = Vec::new();

        for d in a_digits {
            remainder.push(d);
            while remainder.len() > 1 && remainder[0] == 0 {
                remainder.remove(0);
            }

            // Find digit q in 0..=9 such that b_digits * q <= remainder
            let mut q = 0u8;
            for candidate in (1..=9).rev() {
                let prod = mul_digits_single(&b_digits, candidate);
                if cmp_digit_slices(&prod, &remainder) != Ordering::Greater {
                    q = candidate;
                    remainder = sub_digit_slices(&remainder, &prod);
                    break;
                }
            }
            quotient.push(q);
        }

        // Split quotient at target_scale
        let (integral, fractional) = if quotient.len() <= target_scale {
            let missing = target_scale - quotient.len();
            let mut frac = vec![0u8; missing];
            frac.extend(quotient);
            (vec![0], frac)
        } else {
            let split_idx = quotient.len() - target_scale;
            (
                quotient[..split_idx].to_vec(),
                quotient[split_idx..].to_vec(),
            )
        };

        let mut res = Self {
            negative: self.negative ^ other.negative,
            integral,
            fractional,
        };
        // If scale wasn't explicitly forced, normalize strips redundant trailing zeros
        if scale.is_none() {
            res.normalize();
        } else {
            // Keep fractional part padded/truncated to exact scale
            while res.fractional.len() < target_scale {
                res.fractional.push(0);
            }
            res.fractional.truncate(target_scale);
            while res.integral.len() > 1 && res.integral[0] == 0 {
                res.integral.remove(0);
            }
            if res.integral.is_empty() {
                res.integral.push(0);
            }
        }
        Ok(res)
    }
}

fn mul_digits_single(digits: &[u8], factor: u8) -> Vec<u8> {
    if factor == 0 {
        return vec![0];
    }
    let mut carry = 0u16;
    let mut res = Vec::with_capacity(digits.len() + 1);
    for &d in digits.iter().rev() {
        let p = (d as u16) * (factor as u16) + carry;
        res.push((p % 10) as u8);
        carry = p / 10;
    }
    if carry > 0 {
        res.push(carry as u8);
    }
    res.reverse();
    res
}

fn cmp_digit_slices(a: &[u8], b: &[u8]) -> Ordering {
    let a_clean = strip_leading_zeros(a);
    let b_clean = strip_leading_zeros(b);
    if a_clean.len() != b_clean.len() {
        return a_clean.len().cmp(&b_clean.len());
    }
    a_clean.cmp(b_clean)
}

fn strip_leading_zeros(s: &[u8]) -> &[u8] {
    let mut idx = 0;
    while idx + 1 < s.len() && s[idx] == 0 {
        idx += 1;
    }
    &s[idx..]
}

fn sub_digit_slices(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut res = Vec::with_capacity(a.len());
    let mut borrow = 0i16;
    let mut i1 = a.len() as isize - 1;
    let mut i2 = b.len() as isize - 1;

    while i1 >= 0 {
        let d1 = a[i1 as usize] as i16;
        let d2 = if i2 >= 0 { b[i2 as usize] as i16 } else { 0 };
        let mut diff = d1 - d2 - borrow;
        if diff < 0 {
            diff += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        res.push(diff as u8);
        i1 -= 1;
        i2 -= 1;
    }
    res.reverse();
    while res.len() > 1 && res[0] == 0 {
        res.remove(0);
    }
    res
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.negative != other.negative {
            if self.negative {
                return Ordering::Less;
            } else {
                return Ordering::Greater;
            }
        }
        if self.negative {
            other.cmp_abs(self)
        } else {
            self.cmp_abs(other)
        }
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            write!(f, "-")?;
        }
        for d in &self.integral {
            write!(f, "{}", d)?;
        }
        if !self.fractional.is_empty() {
            write!(f, ".")?;
            for d in &self.fractional {
                write!(f, "{}", d)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decimal_add() {
        let a = Decimal::from_str_exact("0.1").unwrap();
        let b = Decimal::from_str_exact("0.2").unwrap();
        let sum = a.add(&b);
        assert_eq!(sum.to_string(), "0.3");
    }

    #[test]
    fn test_decimal_sub() {
        let a = Decimal::from_str_exact("0.3").unwrap();
        let b = Decimal::from_str_exact("0.2").unwrap();
        let diff = a.sub(&b);
        assert_eq!(diff.to_string(), "0.1");
    }

    #[test]
    fn test_decimal_mul() {
        let a = Decimal::from_str_exact("0.1").unwrap();
        let b = Decimal::from_str_exact("0.2").unwrap();
        let prod = a.mul(&b);
        assert_eq!(prod.to_string(), "0.02");
    }

    #[test]
    fn test_decimal_div() {
        let a = Decimal::from_str_exact("0.3").unwrap();
        let b = Decimal::from_str_exact("0.1").unwrap();
        let quot = a.div(&b, None).unwrap();
        assert_eq!(quot.to_string(), "3");
    }
}
