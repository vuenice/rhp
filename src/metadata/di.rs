//! # Dependency Injection & Compile-Time Factory Engine
//!
//! Replaces runtime reflection with compile-time generated factory functions,
//! static container resolution, and build-time circular dependency detection.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use crate::lexer::span::Span;
use crate::metadata::table::ProgramMetadata;
use crate::type_system::types::RhpType;
use crate::vm::engine::VM;
use crate::vm::error::RuntimeError;
use crate::vm::value::PhpValue;

/// Error encountered during dependency injection compilation or container resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum DiError {
    /// Circular dependency detected at compile time.
    CircularDependency {
        cycle: Vec<String>,
    },
    /// An interface or uninstantiable class is required by a constructor, but no binding was provided.
    MissingBinding {
        consumer: String,
        dependency: String,
    },
    /// An unbound interface was requested where multiple or no concrete implementations exist.
    UnboundInterface {
        interface_name: String,
    },
    /// Service or class not registered in container and cannot be auto-wired.
    ServiceNotFound {
        service_name: String,
    },
    /// Error raised during object instantiation or constructor execution.
    InstantiationError {
        class_name: String,
        error: String,
    },
}

impl fmt::Display for DiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CircularDependency { cycle } => {
                write!(
                    f,
                    "Compile-time DI Error: Circular dependency detected: {}",
                    cycle.join(" -> ")
                )
            }
            Self::MissingBinding { consumer, dependency } => {
                write!(
                    f,
                    "Compile-time DI Error: Cannot wire class '{}': missing binding for contract '{}'",
                    consumer, dependency
                )
            }
            Self::UnboundInterface { interface_name } => {
                write!(
                    f,
                    "Compile-time DI Error: Interface '{}' must be explicitly bound via container.bind()",
                    interface_name
                )
            }
            Self::ServiceNotFound { service_name } => {
                write!(
                    f,
                    "DI Error: Service or class '{}' not registered in container",
                    service_name
                )
            }
            Self::InstantiationError { class_name, error } => {
                write!(
                    f,
                    "DI Error: Failed to instantiate class '{}': {}",
                    class_name, error
                )
            }
        }
    }
}

impl std::error::Error for DiError {}

impl From<RuntimeError> for DiError {
    fn from(err: RuntimeError) -> Self {
        Self::InstantiationError {
            class_name: "unknown".to_string(),
            error: err.to_string(),
        }
    }
}

/// Specification of a single constructor dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct DependencySpec {
    pub param_name: String,
    pub service_type: String,
    pub is_optional: bool,
}

/// Compile-time generated class factory recipe.
///
/// Analogous to Go's Wire or Java's Dagger generated code:
/// ```text
/// fn __make_OrderController(c: &mut Container, vm: &mut VM) -> Result<PhpValue, DiError> {
///     let dep0 = c.resolve("PaymentGateway", vm)?;
///     let dep1 = c.resolve("Logger", vm)?;
///     vm.instantiate_and_construct("OrderController", vec![dep0, dep1], span)
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedClassFactory {
    pub class_name: String,
    pub dependencies: Vec<DependencySpec>,
    pub is_singleton: bool,
}

impl GeneratedClassFactory {
    pub fn new(class_name: impl Into<String>, is_singleton: bool) -> Self {
        Self {
            class_name: class_name.into(),
            dependencies: Vec::new(),
            is_singleton,
        }
    }

    pub fn add_dependency(
        &mut self,
        param_name: impl Into<String>,
        service_type: impl Into<String>,
        is_optional: bool,
    ) {
        self.dependencies.push(DependencySpec {
            param_name: param_name.into(),
            service_type: service_type.into(),
            is_optional,
        });
    }

    /// Instantiate this class using dependencies resolved from the container.
    pub fn create_instance(&self, container: &mut Container, vm: &mut VM) -> Result<PhpValue, DiError> {
        let mut args = Vec::with_capacity(self.dependencies.len());
        for dep in &self.dependencies {
            match container.resolve(&dep.service_type, vm) {
                Ok(val) => args.push(val),
                Err(err) => {
                    if dep.is_optional {
                        args.push(PhpValue::Null);
                    } else {
                        return Err(err);
                    }
                }
            }
        }

        vm.instantiate_and_construct(&self.class_name, args, Span::default())
            .map_err(|e| DiError::InstantiationError {
                class_name: self.class_name.clone(),
                error: e.to_string(),
            })
    }
}

/// Registration mode of a service binding.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceBinding {
    /// Bound to another concrete class name.
    Alias {
        concrete: String,
        is_singleton: bool,
    },
    /// Auto-wired factory generated by the compiler.
    Factory(GeneratedClassFactory),
    /// Pre-instantiated singleton instance.
    Instance(PhpValue),
}

/// Inversion of Control (IoC) Dependency Injection Container.
///
/// Supports:
/// - Compile-time auto-wiring via generated factories
/// - Singleton caching
/// - Rocket-style parent process (pre-fork) and child worker (request-scoped) hierarchy
/// - Container swapping for zero-reflection test mocks (`app.swap(PaymentGateway, FakeGateway)`)
#[derive(Debug, Clone, Default)]
pub struct Container {
    /// Bindings table (interfaces/abstract classes to concrete implementations or factories).
    pub bindings: HashMap<String, ServiceBinding>,
    /// Pre-compiled class factories indexed by concrete class name.
    pub factories: HashMap<String, GeneratedClassFactory>,
    /// Cached singleton instances.
    pub singletons: HashMap<String, PhpValue>,
    /// Test swaps / mocks (`app.swap(Abstract, Fake)`).
    pub swaps: HashMap<String, PhpValue>,
    /// Optional parent container for hierarchical request scoping.
    pub parent: Option<Arc<Container>>,
}

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a request-scoped child container with a shared parent container.
    pub fn with_parent(parent: Arc<Container>) -> Self {
        Self {
            parent: Some(parent),
            ..Default::default()
        }
    }

    /// Create an isolated request scope inheriting parent bindings and singletons.
    pub fn create_request_scope(&self) -> Container {
        Container::with_parent(Arc::new(self.clone()))
    }

    /// Bind an interface or abstract name to a concrete implementation class.
    pub fn bind(&mut self, abstract_name: impl Into<String>, concrete_name: impl Into<String>) {
        let clean_abs = Self::clean_name(&abstract_name.into());
        let clean_conc = Self::clean_name(&concrete_name.into());
        self.bindings.insert(
            clean_abs,
            ServiceBinding::Alias {
                concrete: clean_conc,
                is_singleton: false,
            },
        );
    }

    /// Bind an interface or class as a singleton.
    pub fn singleton(&mut self, abstract_name: impl Into<String>, concrete_name: impl Into<String>) {
        let clean_abs = Self::clean_name(&abstract_name.into());
        let clean_conc = Self::clean_name(&concrete_name.into());
        self.bindings.insert(
            clean_abs,
            ServiceBinding::Alias {
                concrete: clean_conc,
                is_singleton: true,
            },
        );
    }

    /// Bind a pre-built instance as a singleton service.
    pub fn instance(&mut self, abstract_name: impl Into<String>, instance: PhpValue) {
        let clean = Self::clean_name(&abstract_name.into());
        self.singletons.insert(clean.clone(), instance.clone());
        self.bindings.insert(clean, ServiceBinding::Instance(instance));
    }

    /// Swap a binding with a fake/mock for testing (`app.swap(PaymentGateway, FakeGateway)`).
    pub fn swap(&mut self, abstract_name: impl Into<String>, fake: PhpValue) {
        let clean = Self::clean_name(&abstract_name.into());
        self.swaps.insert(clean, fake);
    }

    /// Check if a service binding or factory exists.
    pub fn has(&self, service_name: &str) -> bool {
        let clean = Self::clean_name(service_name);
        if self.swaps.contains_key(&clean)
            || self.singletons.contains_key(&clean)
            || self.bindings.contains_key(&clean)
            || self.factories.contains_key(&clean)
        {
            return true;
        }
        if let Some(ref parent) = self.parent {
            return parent.has(&clean);
        }
        false
    }

    /// Auto-wire and register generated factories for all concrete classes in [`ProgramMetadata`].
    /// Performs compile-time circular dependency detection!
    pub fn auto_wire(&mut self, metadata: &ProgramMetadata) -> Result<(), DiError> {
        // 1. Detect circular dependencies at compile time
        detect_circular_dependencies(metadata)?;

        // 2. Generate factories for all concrete classes
        let factories = generate_factories(metadata)?;
        for (name, factory) in factories {
            self.factories.insert(name.clone(), factory.clone());
            self.bindings
                .entry(name)
                .or_insert_with(|| ServiceBinding::Factory(factory));
        }

        Ok(())
    }

    /// Resolve an instance of `service_name` from the container.
    pub fn resolve(&mut self, service_name: &str, vm: &mut VM) -> Result<PhpValue, DiError> {
        let clean = Self::clean_name(service_name);

        // 1. Testing swaps always take precedence
        if let Some(fake) = self.get_swap(&clean) {
            return Ok(fake);
        }

        // 2. Existing cached singleton instance
        if let Some(inst) = self.get_singleton(&clean) {
            return Ok(inst);
        }

        // 3. Explicit binding lookup
        if let Some(binding) = self.get_binding(&clean) {
            match binding {
                ServiceBinding::Instance(inst) => return Ok(inst),
                ServiceBinding::Alias { concrete, is_singleton } => {
                    let inst = if concrete != clean {
                        self.resolve(&concrete, vm)?
                    } else if let Some(factory) = self.get_factory(&clean) {
                        factory.create_instance(self, vm)?
                    } else {
                        let class_info = vm
                            .module()
                            .get_class_by_name(&clean)
                            .map(|class| (class.modifiers.is_abstract, class.modifiers.is_singleton));

                        if let Some((is_abstract, _)) = class_info {
                            if !is_abstract {
                                vm.instantiate_and_construct(&clean, Vec::new(), Span::default())
                                    .map_err(|e| DiError::InstantiationError {
                                        class_name: clean.clone(),
                                        error: e.to_string(),
                                    })?
                            } else {
                                return Err(DiError::ServiceNotFound {
                                    service_name: clean,
                                });
                            }
                        } else {
                            return Err(DiError::ServiceNotFound {
                                service_name: clean,
                            });
                        }
                    };
                    if is_singleton {
                        self.singletons.insert(clean, inst.clone());
                    }
                    return Ok(inst);
                }
                ServiceBinding::Factory(factory) => {
                    let inst = factory.create_instance(self, vm)?;
                    if factory.is_singleton {
                        self.singletons.insert(clean, inst.clone());
                    }
                    return Ok(inst);
                }
            }
        }

        // 4. Generated factory lookup
        if let Some(factory) = self.get_factory(&clean) {
            let inst = factory.create_instance(self, vm)?;
            if factory.is_singleton {
                self.singletons.insert(clean, inst.clone());
            }
            return Ok(inst);
        }

        // 5. Fallback: try auto-instantiating default zero-argument class
        let class_info = vm
            .module()
            .get_class_by_name(&clean)
            .map(|class| (class.modifiers.is_abstract, class.modifiers.is_singleton));

        if let Some((is_abstract, is_singleton)) = class_info {
            if !is_abstract {
                let inst = vm
                    .instantiate_and_construct(&clean, Vec::new(), Span::default())
                    .map_err(|e| DiError::InstantiationError {
                        class_name: clean.clone(),
                        error: e.to_string(),
                    })?;
                if is_singleton {
                    self.singletons.insert(clean, inst.clone());
                }
                return Ok(inst);
            }
        }

        Err(DiError::ServiceNotFound {
            service_name: clean,
        })
    }

    fn get_swap(&self, name: &str) -> Option<PhpValue> {
        if let Some(fake) = self.swaps.get(name) {
            return Some(fake.clone());
        }
        self.parent.as_ref().and_then(|p| p.get_swap(name))
    }

    fn get_singleton(&self, name: &str) -> Option<PhpValue> {
        if let Some(inst) = self.singletons.get(name) {
            return Some(inst.clone());
        }
        self.parent.as_ref().and_then(|p| p.get_singleton(name))
    }

    fn get_binding(&self, name: &str) -> Option<ServiceBinding> {
        if let Some(b) = self.bindings.get(name) {
            return Some(b.clone());
        }
        self.parent.as_ref().and_then(|p| p.get_binding(name))
    }

    fn get_factory(&self, name: &str) -> Option<GeneratedClassFactory> {
        if let Some(f) = self.factories.get(name) {
            return Some(f.clone());
        }
        self.parent.as_ref().and_then(|p| p.get_factory(name))
    }

    fn clean_name(name: &str) -> String {
        name.strip_prefix('\\').unwrap_or(name).to_string()
    }
}

/// Detect circular dependencies across all classes in [`ProgramMetadata`] at compile time.
pub fn detect_circular_dependencies(metadata: &ProgramMetadata) -> Result<(), DiError> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for (name, class) in &metadata.classes {
        if class.is_abstract {
            continue;
        }
        let mut deps = Vec::new();
        if let Some(ref ctor) = class.constructor {
            for param in &ctor.params {
                if let RhpType::Class(ref dep_class) = param.rhp_type {
                    deps.push(dep_class.strip_prefix('\\').unwrap_or(dep_class).to_string());
                }
            }
        }
        graph.insert(name.clone(), deps);
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut rec_stack: Vec<String> = Vec::new();

    for node in graph.keys() {
        if !visited.contains(node) {
            check_cycle(node, &graph, &mut visited, &mut rec_stack)?;
        }
    }

    Ok(())
}

fn check_cycle(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    rec_stack: &mut Vec<String>,
) -> Result<(), DiError> {
    visited.insert(node.to_string());
    rec_stack.push(node.to_string());

    if let Some(neighbors) = graph.get(node) {
        for neighbor in neighbors {
            if !visited.contains(neighbor) {
                check_cycle(neighbor, graph, visited, rec_stack)?;
            } else if let Some(pos) = rec_stack.iter().position(|x| x == neighbor) {
                let mut cycle: Vec<String> = rec_stack[pos..].to_vec();
                cycle.push(neighbor.clone());
                return Err(DiError::CircularDependency { cycle });
            }
        }
    }

    rec_stack.pop();
    Ok(())
}

/// Generate compile-time class factories for all concrete classes in [`ProgramMetadata`].
pub fn generate_factories(
    metadata: &ProgramMetadata,
) -> Result<HashMap<String, GeneratedClassFactory>, DiError> {
    let mut factories = HashMap::new();

    for (class_name, class) in &metadata.classes {
        if class.is_abstract {
            continue;
        }

        let is_singleton = class.is_singleton || class.has_attribute("Singleton");
        let mut factory = GeneratedClassFactory::new(class_name, is_singleton);

        if let Some(ref ctor) = class.constructor {
            for param in &ctor.params {
                let service_type = match &param.rhp_type {
                    RhpType::Class(cls) => cls.strip_prefix('\\').unwrap_or(cls).to_string(),
                    RhpType::Nullable(inner) => match &**inner {
                        RhpType::Class(cls) => cls.strip_prefix('\\').unwrap_or(cls).to_string(),
                        _ => param.name.clone(),
                    },
                    _ => param.name.clone(),
                };

                let is_optional = param.has_default || matches!(param.rhp_type, RhpType::Nullable(_));
                factory.add_dependency(&param.name, service_type, is_optional);
            }
        }

        factories.insert(class_name.clone(), factory);
    }

    Ok(factories)
}
