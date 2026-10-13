//! # JIT Native Slot and Runtime ABI
//!
//! Provides memory-aligned native slot representations for register file transfer
//! between the RHP bytecode VM and Cranelift native machine code.

use crate::vm::PhpValue;

pub const TAG_NULL: i64 = 0;
pub const TAG_BOOL: i64 = 1;
pub const TAG_INT: i64 = 2;
pub const TAG_FLOAT: i64 = 3;
pub const TAG_OTHER: i64 = 4;

/// A contiguous 24-byte representation of a virtual register slot passed into
/// Cranelift-generated machine code.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeSlot {
    pub int_val: i64,
    pub float_val: f64,
    pub tag: i64,
}

impl Default for NativeSlot {
    fn default() -> Self {
        Self {
            int_val: 0,
            float_val: 0.0,
            tag: TAG_NULL,
        }
    }
}

impl NativeSlot {
    #[inline]
    pub fn new_null() -> Self {
        Self {
            int_val: 0,
            float_val: 0.0,
            tag: TAG_NULL,
        }
    }

    #[inline]
    pub fn new_bool(b: bool) -> Self {
        Self {
            int_val: if b { 1 } else { 0 },
            float_val: 0.0,
            tag: TAG_BOOL,
        }
    }

    #[inline]
    pub fn new_int(n: i64) -> Self {
        Self {
            int_val: n,
            float_val: n as f64,
            tag: TAG_INT,
        }
    }

    #[inline]
    pub fn new_float(f: f64) -> Self {
        Self {
            int_val: f as i64,
            float_val: f,
            tag: TAG_FLOAT,
        }
    }

    /// Convert a dynamic [`PhpValue`] into a [`NativeSlot`].
    pub fn from_php_value(val: &PhpValue) -> Self {
        match val {
            PhpValue::Null => Self::new_null(),
            PhpValue::Bool(b) => Self::new_bool(*b),
            PhpValue::Int(n) => Self::new_int(*n as i64),
            PhpValue::RustInt { value, .. } => Self::new_int(*value as i64),
            PhpValue::Float(f) => Self::new_float(*f as f64),
            PhpValue::RustFloat { value, .. } => Self::new_float(*value),
            _ => Self {
                int_val: 0,
                float_val: 0.0,
                tag: TAG_OTHER,
            },
        }
    }

    /// Convert this [`NativeSlot`] back to a [`PhpValue`], preserving original complex
    /// value types if the slot holds non-primitive data.
    pub fn to_php_value(&self, original: Option<&PhpValue>) -> PhpValue {
        match self.tag {
            TAG_NULL => PhpValue::Null,
            TAG_BOOL => PhpValue::Bool(self.int_val != 0),
            TAG_INT => {
                if let Some(PhpValue::RustInt { kind, .. }) = original {
                    PhpValue::RustInt {
                        kind: *kind,
                        value: self.int_val as i128,
                    }
                } else {
                    PhpValue::Int(self.int_val as i32)
                }
            }
            TAG_FLOAT => {
                if let Some(PhpValue::RustFloat { kind, .. }) = original {
                    PhpValue::RustFloat {
                        kind: *kind,
                        value: self.float_val,
                    }
                } else {
                    PhpValue::Float(self.float_val as f32)
                }
            }
            TAG_OTHER => original.cloned().unwrap_or(PhpValue::Null),
            _ => PhpValue::Null,
        }
    }
}

/// Contiguous register buffer for fast JIT frame unboxing and write-back.
#[derive(Debug, Clone, Default)]
pub struct NativeFrame {
    pub slots: Vec<NativeSlot>,
}

impl NativeFrame {
    pub fn new(capacity: usize) -> Self {
        Self {
            slots: vec![NativeSlot::default(); capacity],
        }
    }

    /// Load VM register window into native slots.
    pub fn load_from_registers(&mut self, regs: &[PhpValue]) {
        self.slots.clear();
        self.slots.reserve(regs.len());
        for val in regs {
            self.slots.push(NativeSlot::from_php_value(val));
        }
    }

    /// Write updated native slots back into the VM register window.
    pub fn write_back_to_registers(&self, regs: &mut [PhpValue]) {
        let len = self.slots.len().min(regs.len());
        for i in 0..len {
            // Only overwrite if the native slot was modified or was primitive
            if self.slots[i].tag != TAG_OTHER {
                regs[i] = self.slots[i].to_php_value(Some(&regs[i]));
            }
        }
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut NativeSlot {
        self.slots.as_mut_ptr()
    }
}
