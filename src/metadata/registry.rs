//! # Build-Time Attribute & Symbol Registry
//!
//! Scans language attributes at build time into pre-indexed registries.
//! Replaces runtime reflection scanning and boot-time file inspection with
//! static, zero-cost metadata tables.

use std::collections::{HashMap, HashSet};

use crate::metadata::table::{CompiledAttribute, ProgramMetadata};

/// Discovered HTTP route definition collected at build time.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredRoute {
    pub path: String,
    pub methods: Vec<String>,
    pub controller_class: String,
    pub method_name: String,
    pub route_name: Option<String>,
    pub middleware: Vec<String>,
}

/// Discovered CLI command collected at build time.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredCommand {
    pub signature: String,
    pub class_name: String,
    pub method_name: Option<String>,
    pub description: Option<String>,
}

/// Discovered event listener collected at build time.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredListener {
    pub event_name: String,
    pub listener_class: String,
    pub method_name: String,
}

/// Discovered authorization policy collected at build time.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredPolicy {
    pub model_class: String,
    pub policy_class: String,
}

/// Pre-indexed static registry of build-time discovered symbols.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttributeRegistry {
    /// Discovered HTTP routes from `#[Route]`, `#[Get]`, `#[Post]`, etc.
    pub routes: Vec<DiscoveredRoute>,
    /// Discovered CLI commands from `#[Command]`.
    pub commands: Vec<DiscoveredCommand>,
    /// Discovered event listeners from `#[Listener]`.
    pub listeners: Vec<DiscoveredListener>,
    /// Discovered policies from `#[Policy]`.
    pub policies: Vec<DiscoveredPolicy>,
    /// Classes declared with `#[Singleton]` or keyword `singleton`.
    pub singletons: HashSet<String>,
    /// Interfaces or classes marked with `#[mockable]` for compile-time mock generation.
    pub mockables: HashSet<String>,
    /// Registered service providers declared in packages or application manifests.
    pub service_providers: Vec<String>,
    /// String-to-class-id mapping for explicit dynamic resolution (`app($name)`).
    pub class_ids: HashMap<String, usize>,
}

impl AttributeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan a populated [`ProgramMetadata`] table and extract all attributes into indexed registries.
    pub fn scan_from_metadata(&mut self, metadata: &ProgramMetadata) {
        let mut class_id = 0;
        for (class_name, class) in &metadata.classes {
            self.class_ids.insert(class_name.clone(), class_id);
            class_id += 1;

            if class.is_singleton || class.has_attribute("Singleton") {
                self.singletons.insert(class_name.clone());
            }

            if class.has_attribute("mockable") || class.has_attribute("Mockable") {
                self.mockables.insert(class_name.clone());
            }

            // Class-level policy: `#[Policy(User::class)]`
            if let Some(policy_attr) = class.get_attribute("Policy") {
                if let Some(model) = policy_attr.first_str() {
                    self.policies.push(DiscoveredPolicy {
                        model_class: model.to_string(),
                        policy_class: class_name.clone(),
                    });
                }
            }

            // Class-level route prefix: `#[Route('/api/v1')]`
            let class_route_prefix = class
                .get_attribute("Route")
                .and_then(|a| a.first_str())
                .unwrap_or("");

            // Class-level command: `#[Command('app:sync')]`
            if let Some(cmd_attr) = class.get_attribute("Command") {
                if let Some(sig) = cmd_attr.first_str() {
                    let desc = cmd_attr.get_arg("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                    self.commands.push(DiscoveredCommand {
                        signature: sig.to_string(),
                        class_name: class_name.clone(),
                        method_name: None,
                        description: desc,
                    });
                }
            }

            // Scan method-level attributes
            for (method_name, method) in &class.methods {
                // Route attributes: `#[Route]`, `#[Get]`, `#[Post]`, `#[Put]`, `#[Delete]`, `#[Patch]`
                for attr in &method.attributes {
                    if let Some(route) = Self::parse_route_attribute(
                        class_name,
                        method_name,
                        class_route_prefix,
                        attr,
                    ) {
                        self.routes.push(route);
                    }

                    // Command attribute on method: `#[Command('users:prune')]`
                    if attr.name.eq_ignore_ascii_case("Command") {
                        if let Some(sig) = attr.first_str() {
                            let desc = attr.get_arg("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                            self.commands.push(DiscoveredCommand {
                                signature: sig.to_string(),
                                class_name: class_name.clone(),
                                method_name: Some(method_name.clone()),
                                description: desc,
                            });
                        }
                    }

                    // Listener attribute: `#[Listener('UserRegistered')]`
                    if attr.name.eq_ignore_ascii_case("Listener") {
                        if let Some(event) = attr.first_str() {
                            self.listeners.push(DiscoveredListener {
                                event_name: event.to_string(),
                                listener_class: class_name.clone(),
                                method_name: method_name.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    fn parse_route_attribute(
        class_name: &str,
        method_name: &str,
        prefix: &str,
        attr: &CompiledAttribute,
    ) -> Option<DiscoveredRoute> {
        let name_upper = attr.name.to_ascii_uppercase();
        let (path_raw, methods) = match name_upper.as_str() {
            "ROUTE" => {
                let p = attr.first_str().unwrap_or("/");
                let m = if let Some(methods_arg) = attr.get_arg("methods") {
                    methods_arg.to_string_list()
                } else if let Some(method_arg) = attr.get_arg("method") {
                    method_arg.to_string_list()
                } else {
                    vec!["GET".to_string()]
                };
                (p, m)
            }
            "GET" => (attr.first_str().unwrap_or("/"), vec!["GET".to_string()]),
            "POST" => (attr.first_str().unwrap_or("/"), vec!["POST".to_string()]),
            "PUT" => (attr.first_str().unwrap_or("/"), vec!["PUT".to_string()]),
            "DELETE" => (attr.first_str().unwrap_or("/"), vec!["DELETE".to_string()]),
            "PATCH" => (attr.first_str().unwrap_or("/"), vec!["PATCH".to_string()]),
            _ => return None,
        };

        let full_path = if prefix.is_empty() {
            path_raw.to_string()
        } else if prefix.ends_with('/') && path_raw.starts_with('/') {
            format!("{}{}", &prefix[..prefix.len() - 1], path_raw)
        } else if !prefix.ends_with('/') && !path_raw.starts_with('/') {
            format!("{}/{}", prefix, path_raw)
        } else {
            format!("{}{}", prefix, path_raw)
        };

        let route_name = attr.get_arg("name").and_then(|v| v.as_str()).map(|s| s.to_string());
        let middleware = attr.get_arg("middleware").map(|v| v.to_string_list()).unwrap_or_default();

        Some(DiscoveredRoute {
            path: full_path,
            methods,
            controller_class: class_name.to_string(),
            method_name: method_name.to_string(),
            route_name,
            middleware,
        })
    }

    /// Register a service provider class declared in `rhp.toml`.
    pub fn add_service_provider(&mut self, provider_class: impl Into<String>) {
        self.service_providers.push(provider_class.into());
    }

    /// Check if a class is registered as a singleton.
    pub fn is_singleton(&self, class_name: &str) -> bool {
        let clean = class_name.strip_prefix('\\').unwrap_or(class_name);
        self.singletons.contains(clean) || self.singletons.contains(class_name)
    }

    /// Check if an interface or class is marked `#[mockable]`.
    pub fn is_mockable(&self, name: &str) -> bool {
        let clean = name.strip_prefix('\\').unwrap_or(name);
        self.mockables.contains(clean) || self.mockables.contains(name)
    }
}
