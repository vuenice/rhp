use std::collections::BTreeMap;
use std::path::Path;

use crate::diagnostic::{Diagnostic, DiagnosticLevel, DiagnosticSpan, Position};
use crate::expand::traits::Expander;

/// Dual-namespace registry for compile-time AST expanders.
///
/// Keeps attribute expanders (`#[...]`) strictly segregated from
/// template directive expanders (`@...`), enforcing deterministic BTreeMap ordering.
pub struct ExpanderRegistry {
    attributes: BTreeMap<String, Box<dyn Expander>>,
    directives: BTreeMap<String, Box<dyn Expander>>,
}

impl Default for ExpanderRegistry {
    fn default() -> Self {
        Self::default_with_builtins()
    }
}

impl ExpanderRegistry {
    /// Create an empty registry with no expanders.
    pub fn new() -> Self {
        Self {
            attributes: BTreeMap::new(),
            directives: BTreeMap::new(),
        }
    }

    /// Create a registry populated with all built-in RHP attributes and template directives.
    pub fn default_with_builtins() -> Self {
        let mut registry = Self::new();
        crate::expand::builtin::register_builtins(&mut registry)
            .expect("Built-in expander registration must not fail");
        registry
    }

    /// Register a class/member attribute expander (e.g. `fillable`, `guarded`, `table`).
    ///
    /// Duplicate registration within the attribute namespace returns an `RHP0507` error.
    pub fn register_attribute(&mut self, expander: Box<dyn Expander>) -> Result<(), Diagnostic> {
        let name = expander.name().to_string();
        if self.attributes.contains_key(&name) {
            return Err(Diagnostic {
                level: DiagnosticLevel::Error,
                code: "RHP0507".to_string(),
                message: format!("duplicate attribute expander registration: `#[{}]` is already registered", name),
                span: DiagnosticSpan {
                    file: "<registry>".to_string(),
                    start: Position::new(1, 1, 0),
                    end: Position::new(1, 1, 0),
                },
                found: Some(name.clone()),
                expected: None,
                labels: Vec::new(),
                snippet: Vec::new(),
                suggestions: Vec::new(),
                explain: "rhp explain RHP0507".to_string(),
                rendered: None,
                caused_by: None,
            });
        }
        self.attributes.insert(name, expander);
        Ok(())
    }

    /// Register a template directive expander (e.g. `if`, `for`, `foreach`, `datetime`).
    ///
    /// Duplicate registration within the directive namespace returns an `RHP0507` error.
    pub fn register_directive(&mut self, expander: Box<dyn Expander>) -> Result<(), Diagnostic> {
        let name = expander.name().to_string();
        if self.directives.contains_key(&name) {
            return Err(Diagnostic {
                level: DiagnosticLevel::Error,
                code: "RHP0507".to_string(),
                message: format!("duplicate directive expander registration: `@{}` is already registered", name),
                span: DiagnosticSpan {
                    file: "<registry>".to_string(),
                    start: Position::new(1, 1, 0),
                    end: Position::new(1, 1, 0),
                },
                found: Some(name.clone()),
                expected: None,
                labels: Vec::new(),
                snippet: Vec::new(),
                suggestions: Vec::new(),
                explain: "rhp explain RHP0507".to_string(),
                rendered: None,
                caused_by: None,
            });
        }
        self.directives.insert(name, expander);
        Ok(())
    }

    /// Retrieve an attribute expander by name.
    pub fn get_attribute(&self, name: &str) -> Option<&dyn Expander> {
        self.attributes.get(name).map(|b| b.as_ref())
    }

    /// Retrieve a template directive expander by name.
    pub fn get_directive(&self, name: &str) -> Option<&dyn Expander> {
        self.directives.get(name).map(|b| b.as_ref())
    }

    /// Check if an attribute expander is registered.
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.contains_key(name)
    }

    /// Check if a template directive expander is registered.
    pub fn has_directive(&self, name: &str) -> bool {
        self.directives.contains_key(name)
    }

    /// Iteration over registered attribute expanders in deterministic alphabetical order.
    pub fn iter_attributes(&self) -> impl Iterator<Item = (&str, &dyn Expander)> {
        self.attributes.iter().map(|(k, v)| (k.as_str(), v.as_ref()))
    }

    /// Iteration over registered directive expanders in deterministic alphabetical order.
    pub fn iter_directives(&self) -> impl Iterator<Item = (&str, &dyn Expander)> {
        self.directives.iter().map(|(k, v)| (k.as_str(), v.as_ref()))
    }

    /// Config and package discovery hook.
    ///
    /// Milestone 1 registers built-ins and checks for configuration presence.
    /// Milestone 2 expands this hook to discover and compile userland RHP macro packages
    /// specified in `[compiler.plugins]` or `[expand]` sections of `rhp.toml`.
    pub fn populate_from_config(&mut self, config_path: Option<&Path>) -> Result<(), Diagnostic> {
        // Built-ins are already registered.
        // If a config file path is given, check for syntax/structure to ensure the hook contract.
        if let Some(path) = config_path {
            if path.exists() {
                // Milestone 2 discovery stub: validates file accessibility without executing unverified macros.
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(toml_val) = content.parse::<toml::Value>() {
                        if let Some(_expand_table) = toml_val.get("expand").or_else(|| toml_val.get("compiler")) {
                            // Slot for Milestone 2 comptime RHP macro discovery
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
