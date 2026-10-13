//! # Cranelift JIT Configuration
//!
//! Controls runtime compilation thresholds, optimization levels, and feature flags
//! for the RHP Just-In-Time (JIT) compilation pipeline.

/// Configuration parameters for the Cranelift JIT compilation engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitConfig {
    /// Whether JIT compilation is enabled.
    pub enabled: bool,
    /// Number of function invocations required before triggering JIT compilation.
    pub hot_function_threshold: usize,
    /// Number of loop backedge iterations required before triggering hot-loop JIT compilation.
    pub hot_loop_threshold: usize,
    /// Cranelift optimization level: 0 = none, 1 = speed, 2 = speed_and_size.
    pub opt_level: u8,
    /// Enable on-stack hot execution loop compilation.
    pub enable_loop_jit: bool,
    /// Enable whole-function compilation for long-lived and hot functions.
    pub enable_function_jit: bool,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hot_function_threshold: 16,
            hot_loop_threshold: 32,
            opt_level: 2,
            enable_loop_jit: true,
            enable_function_jit: true,
        }
    }
}

impl JitConfig {
    /// Create a new [`JitConfig`] with JIT enabled and default thresholds.
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Set whether JIT compilation is enabled.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the invocation threshold for compiling hot functions.
    pub fn with_hot_function_threshold(mut self, threshold: usize) -> Self {
        self.hot_function_threshold = threshold;
        self
    }

    /// Set the backedge iteration threshold for compiling hot loops.
    pub fn with_hot_loop_threshold(mut self, threshold: usize) -> Self {
        self.hot_loop_threshold = threshold;
        self
    }

    /// Set the Cranelift optimization level (0..2).
    pub fn with_opt_level(mut self, opt_level: u8) -> Self {
        self.opt_level = opt_level;
        self
    }

    /// Toggle hot execution loop compilation.
    pub fn with_enable_loop_jit(mut self, enable: bool) -> Self {
        self.enable_loop_jit = enable;
        self
    }

    /// Toggle hot function compilation.
    pub fn with_enable_function_jit(mut self, enable: bool) -> Self {
        self.enable_function_jit = enable;
        self
    }
}
