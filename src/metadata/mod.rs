//! # Compile-Time Metadata & Dependency Injection Subsystem
//!
//! Provides the architectural alternative to PHP's runtime Reflection API:
//! - **Static Metadata Table**: Emits read-only static descriptors for every class, constructor,
//!   method, and attribute at build time with zero runtime reflection.
//! - **Dependency Injection**: Compiler-generated class factories with compile-time circular
//!   dependency detection and missing binding verification.
//! - **Method Injection & Handler Shims**: Inspects route handler parameters once at route
//!   registration/build time, generating fast direct call shims (Request, RouteBindable, scalars, container).
//! - **Route Parameter Binding**: Clean `RouteBindable` interface model resolution with strict
//!   type validation for URL scalar parameters.
//! - **Container Swapping for Testing**: Zero-reflection test mocking via `container.swap()` and `App::swap()`.
//! - **Build-Time Package Discovery**: Scans `#[Route]`, `#[Command]`, `#[Listener]`, and `#[Policy]`
//!   into indexed tables with no boot-time directory scanning.

pub mod di;
pub mod extractor;
pub mod registry;
pub mod shim;
pub mod table;

pub use di::{
    detect_circular_dependencies, generate_factories, Container, DependencySpec, DiError,
    GeneratedClassFactory, ServiceBinding,
};
pub use extractor::{extract_metadata, MetadataExtractor};
pub use registry::{
    AttributeRegistry, DiscoveredCommand, DiscoveredListener, DiscoveredPolicy, DiscoveredRoute,
};
pub use shim::{
    build_request_object, extract_url_param_names, DispatchError, HandlerShim, ParamSource,
};
pub use table::{
    AttributeArg, AttributeArgValue, ClassMetadata, CompiledAttribute, FunctionMetadata,
    MethodMetadata, ParamMetadata, ProgramMetadata, PropertyMetadata,
};

use crate::ast::Program;
use crate::type_system::types::TypeHierarchy;

/// Compile an AST [`Program`] into a complete static [`ProgramMetadata`] table.
pub fn compile_metadata(
    program: &Program<'_>,
    hierarchy: Option<&TypeHierarchy>,
) -> ProgramMetadata {
    extract_metadata(program, hierarchy)
}
