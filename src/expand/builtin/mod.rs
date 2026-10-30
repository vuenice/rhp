pub mod attributes;
pub mod directives;

pub use attributes::{FillableExpander, GuardedExpander, TableExpander};
pub use directives::{DatetimeExpander, ForExpander, ForeachExpander, IfExpander};

use crate::diagnostic::Diagnostic;
use crate::expand::registry::ExpanderRegistry;

/// Register all standard built-in attribute and directive expanders.
pub fn register_builtins(registry: &mut ExpanderRegistry) -> Result<(), Diagnostic> {
    // Attributes
    registry.register_attribute(Box::new(FillableExpander))?;
    registry.register_attribute(Box::new(GuardedExpander))?;
    registry.register_attribute(Box::new(TableExpander))?;

    // Declarative compile-time metadata & DI attributes
    for name in &[
        "Singleton", "Route", "Get", "Post", "Put", "Delete", "Patch",
        "Command", "Listener", "Policy", "Attribute",
    ] {
        registry.register_attribute(Box::new(attributes::DeclarativeExpander::new(name)))?;
    }

    // Directives
    registry.register_directive(Box::new(IfExpander))?;
    registry.register_directive(Box::new(ForExpander))?;
    registry.register_directive(Box::new(ForeachExpander))?;
    registry.register_directive(Box::new(DatetimeExpander))?;

    Ok(())
}
