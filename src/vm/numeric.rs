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

/// Store a decimal as 64-bit float.
pub fn narrow_decimal(mut value: Decimal) -> Result<PhpValue, ArithFail> {
    value.normalize();
    Ok(PhpValue::float(value.to_f64()))
}

pub fn decimal_from_literal(raw: &str) -> Option<Decimal> {
    let cleaned: String = raw.chars().filter(|c| *c != '_').collect();
    Decimal::from_str_exact(&cleaned)
}


pub fn php_to_decimal(value: &PhpValue) -> Option<Decimal> {
    match value {
        PhpValue::Float(f) => Decimal::from_f64(*f),
        PhpValue::Int(n) => Some(Decimal::from_i64(*n)),
        PhpValue::Bool(b) => Some(Decimal::from_i64(if *b { 1 } else { 0 })),
        PhpValue::String(s) => Decimal::from_str_exact(s.trim()),
        _ => None,
    }
}

pub fn is_float_like(value: &PhpValue) -> bool {
    matches!(value, PhpValue::Float(_))
}

/// Read an integer operand as `i64`. Integers that do not fit are overflow, not a type error.
pub fn int_operand(value: &PhpValue) -> Result<i64, ArithFail> {
    match value {
        PhpValue::Int(n) => Ok(*n),
        PhpValue::Bool(b) => Ok(if *b { 1 } else { 0 }),
        PhpValue::String(s) => s.trim().parse::<i64>().map_err(|_| ArithFail::NotNumeric),
        PhpValue::Float(_) => Err(ArithFail::NotNumeric),
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
    let a = match lhs {
        PhpValue::Float(f) => *f,
        PhpValue::Int(n) => *n as f64,
        PhpValue::Bool(b) => if *b { 1.0 } else { 0.0 },
        PhpValue::String(s) => s.trim().parse::<f64>().map_err(|_| ArithFail::NotNumeric)?,
        _ => return Err(ArithFail::NotNumeric),
    };
    let b = match rhs {
        PhpValue::Float(f) => *f,
        PhpValue::Int(n) => *n as f64,
        PhpValue::Bool(b) => if *b { 1.0 } else { 0.0 },
        PhpValue::String(s) => s.trim().parse::<f64>().map_err(|_| ArithFail::NotNumeric)?,
        _ => return Err(ArithFail::NotNumeric),
    };
    let result = match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => {
            if b == 0.0 {
                return Err(ArithFail::DivideByZero);
            }
            a / b
        }
        BinaryOp::Mod => {
            if b == 0.0 {
                return Err(ArithFail::DivideByZero);
            }
            a % b
        }
        BinaryOp::Pow => a.powf(b),
        _ => return Err(ArithFail::NotNumeric),
    };
    Ok(PhpValue::float(result))
}



/// Coerce a value into the stored width named by a boundary type.
pub fn fits_stored_width(value: &PhpValue, width: &str) -> Option<PhpValue> {
    let bits = value.signed_bits()?;
    match width {
        "i64" => i64::try_from(bits).ok().map(PhpValue::Int),
        _ => None,
    }
}
