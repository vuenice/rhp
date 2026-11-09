//! Fixed-width numeric operations.
//!
//! Integer arithmetic always produces an `i64`. A result that does not fit is an error.
//! Float arithmetic runs in decimal, then stores the narrowest exact form: `f32`, `f64`, or `Decimal`.

use crate::ast::expr::BinaryOp;
use crate::vm::decimal::Decimal;
use crate::vm::value::PhpValue;

pub const DECIMAL_MAX_DIGITS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithFail {
    Overflow,
    DecimalOverflow,
    DivideByZero,
    NotNumeric,
}

/// Store a decimal as exact `f32`, exact `f64`, or `Decimal`.
pub fn narrow_decimal(mut value: Decimal) -> Result<PhpValue, ArithFail> {
    value.normalize();
    if value.digit_count() > DECIMAL_MAX_DIGITS {
        return Err(ArithFail::DecimalOverflow);
    }
    if let Some(f) = exact_f32(&value) {
        return Ok(PhpValue::Float(f));
    }
    if let Some(f) = exact_f64(&value) {
        return Ok(PhpValue::Float64(f));
    }
    Ok(PhpValue::Decimal(Box::new(value)))
}

pub fn decimal_from_literal(raw: &str) -> Option<Decimal> {
    let cleaned: String = raw.chars().filter(|c| *c != '_').collect();
    Decimal::from_str_exact(&cleaned)
}

fn exact_f32(value: &Decimal) -> Option<f32> {
    let parsed: f32 = value.to_string().parse().ok()?;
    let back = f32_bits_to_decimal(parsed)?;
    if &back == value {
        Some(parsed)
    } else {
        None
    }
}

fn exact_f64(value: &Decimal) -> Option<f64> {
    let parsed: f64 = value.to_string().parse().ok()?;
    let back = f64_bits_to_decimal(parsed)?;
    if &back == value {
        Some(parsed)
    } else {
        None
    }
}

fn f32_bits_to_decimal(val: f32) -> Option<Decimal> {
    if !val.is_finite() {
        return None;
    }
    let bits = val.to_bits();
    let negative = bits & 0x8000_0000 != 0;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x7f_ffff;
    if exp == 0 && frac == 0 {
        return Some(Decimal::zero());
    }
    let (sig, pow) = if exp == 0 {
        (frac as u128, -126 - 23)
    } else {
        ((frac | 0x80_0000) as u128, exp - 127 - 23)
    };
    Some(significand_pow2(negative, sig, pow))
}

fn f64_bits_to_decimal(val: f64) -> Option<Decimal> {
    if !val.is_finite() {
        return None;
    }
    let bits = val.to_bits();
    let negative = bits & 0x8000_0000_0000_0000 != 0;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    if exp == 0 && frac == 0 {
        return Some(Decimal::zero());
    }
    let (sig, pow) = if exp == 0 {
        (frac as u128, -1022 - 52)
    } else {
        ((frac | (1u64 << 52)) as u128, exp - 1023 - 52)
    };
    Some(significand_pow2(negative, sig, pow))
}

fn significand_pow2(negative: bool, significand: u128, pow2: i32) -> Decimal {
    let mut value = Decimal::from_str_exact(&significand.to_string()).unwrap_or_else(Decimal::zero);
    value.negative = negative && !value.is_zero();
    let two = Decimal::from_i64(2);
    if pow2 >= 0 {
        for _ in 0..pow2 {
            value = value.mul(&two);
        }
    } else {
        for _ in 0..(-pow2) {
            value = value.div(&two, None).unwrap_or_else(|_| Decimal::zero());
        }
    }
    value.normalize();
    value
}

pub fn php_to_decimal(value: &PhpValue) -> Option<Decimal> {
    match value {
        PhpValue::Decimal(d) => Some((**d).clone()),
        PhpValue::Float(f) => Decimal::from_f32(*f),
        PhpValue::Float64(f) => Decimal::from_f64(*f),
        PhpValue::Int(n) => Some(Decimal::from_i64(*n)),
        PhpValue::Bool(b) => Some(Decimal::from_i64(if *b { 1 } else { 0 })),
        PhpValue::String(s) => Decimal::from_str_exact(s.trim()),
        _ => None,
    }
}

pub fn is_float_like(value: &PhpValue) -> bool {
    matches!(
        value,
        PhpValue::Float(_) | PhpValue::Float64(_) | PhpValue::Decimal(_)
    )
}

/// Read an integer operand as `i64`. Integers that do not fit are overflow, not a type error.
pub fn int_operand(value: &PhpValue) -> Result<i64, ArithFail> {
    match value {
        PhpValue::Int(n) => Ok(*n),
        PhpValue::Bool(b) => Ok(if *b { 1 } else { 0 }),
        PhpValue::String(s) => s.trim().parse::<i64>().map_err(|_| ArithFail::NotNumeric),
        PhpValue::Float(_) | PhpValue::Float64(_) | PhpValue::Decimal(_) => Err(ArithFail::NotNumeric),
        _ => Err(ArithFail::NotNumeric),
    }
}

pub fn checked_int_op(op: BinaryOp, a: i64, b: i64) -> Result<i64, ArithFail> {
    let result = match op {
        BinaryOp::Add => a.checked_add(b),
        BinaryOp::Sub => a.checked_sub(b),
        BinaryOp::Mul => a.checked_mul(b),
        BinaryOp::Div => {
            if b == 0 {
                return Err(ArithFail::DivideByZero);
            }
            if a % b != 0 {
                return Err(ArithFail::NotNumeric);
            }
            a.checked_div(b)
        }
        BinaryOp::Mod => {
            if b == 0 {
                return Err(ArithFail::DivideByZero);
            }
            Some(a % b)
        }
        BinaryOp::Pow => {
            if b < 0 {
                return Err(ArithFail::NotNumeric);
            }
            a.checked_pow(b as u32)
        }
        _ => None,
    };
    result.ok_or(ArithFail::Overflow)
}

pub fn checked_int_add(value: i64, delta: i64) -> Result<i64, ArithFail> {
    value.checked_add(delta).ok_or(ArithFail::Overflow)
}

pub fn checked_int_neg(value: i64) -> Result<i64, ArithFail> {
    value.checked_neg().ok_or(ArithFail::Overflow)
}

pub fn float_binary(op: BinaryOp, lhs: &PhpValue, rhs: &PhpValue) -> Result<PhpValue, ArithFail> {
    let a = php_to_decimal(lhs).ok_or(ArithFail::NotNumeric)?;
    let b = php_to_decimal(rhs).ok_or(ArithFail::NotNumeric)?;
    let result = match op {
        BinaryOp::Add => a.add(&b),
        BinaryOp::Sub => a.sub(&b),
        BinaryOp::Mul => a.mul(&b),
        BinaryOp::Div => {
            if b.is_zero() {
                return Err(ArithFail::DivideByZero);
            }
            let scale = a.fractional.len().max(b.fractional.len()).saturating_add(16);
            a.div(&b, Some(scale)).map_err(|_| ArithFail::DivideByZero)?
        }
        BinaryOp::Mod => {
            if b.is_zero() {
                return Err(ArithFail::DivideByZero);
            }
            decimal_mod(&a, &b)?
        }
        BinaryOp::Pow => decimal_pow(&a, &b)?,
        _ => return Err(ArithFail::NotNumeric),
    };
    narrow_decimal(result)
}

fn decimal_mod(a: &Decimal, b: &Decimal) -> Result<Decimal, ArithFail> {
    let quot = a.div(b, Some(0)).map_err(|_| ArithFail::DivideByZero)?;
    let whole = Decimal::from_str_exact(&truncate_toward_zero(&quot.to_string()))
        .unwrap_or_else(Decimal::zero);
    Ok(a.sub(&whole.mul(b)))
}

fn truncate_toward_zero(text: &str) -> String {
    match text.split_once('.') {
        Some((int_part, _)) => int_part.to_string(),
        None => text.to_string(),
    }
}

fn decimal_pow(base: &Decimal, exp: &Decimal) -> Result<Decimal, ArithFail> {
    if exp.fractional.is_empty() {
        let exp_text = exp.to_string();
        if let Ok(n) = exp_text.parse::<i32>() {
            if n == 0 {
                return Ok(Decimal::from_i64(1));
            }
            if n > 0 && n <= 64 {
                let mut acc = Decimal::from_i64(1);
                for _ in 0..n {
                    acc = acc.mul(base);
                    if acc.digit_count() > DECIMAL_MAX_DIGITS {
                        return Err(ArithFail::DecimalOverflow);
                    }
                }
                return Ok(acc);
            }
        }
    }
    let bf = base.to_f64();
    let ef = exp.to_f64();
    let powered = bf.powf(ef);
    Decimal::from_f64(powered).ok_or(ArithFail::NotNumeric)
}

/// Coerce a value into the stored width named by a boundary type.
pub fn fits_stored_width(value: &PhpValue, width: &str) -> Option<PhpValue> {
    let bits = value.signed_bits()?;
    match width {
        "i64" => i64::try_from(bits).ok().map(PhpValue::Int),
        _ => None,
    }
}
