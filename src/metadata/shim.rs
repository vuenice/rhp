//! # Method Injection & Compile-Time Handler Shims
//!
//! Replaces per-request runtime reflection with pre-compiled handler shims.
//! When routes are registered (or during build time), the compiler creates a shim
//! that resolves parameter sources:
//! 1. `Request` (or FormRequest subtype) from HTTP request
//! 2. Types implementing `RouteBindable` from URL route segments
//! 3. Scalars (`Int`, `String`, `Float`, `Bool`) from URL route parameters with validation
//! 4. Any other service from the DI container

use std::collections::HashMap;
use std::fmt;

use crate::lexer::span::Span;
use crate::metadata::di::{Container, DiError};
use crate::metadata::table::ProgramMetadata;
use crate::type_system::types::{RhpType, TypeHierarchy};
use crate::vm::engine::VM;
use crate::vm::value::{PhpValue, VmObject};
use crate::vm::worker::WorkerRequest;

/// Error during handler shim compilation or request dispatching.
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchError {
    /// Validation error for a route scalar parameter (e.g. non-integer passed to `Int`).
    ValidationError {
        param_name: String,
        expected: String,
        actual: String,
    },
    /// Route model binding failed (`resolveRouteBinding` returned null).
    NotFound {
        model: String,
        param_name: String,
        value: String,
    },
    /// Required route parameter was missing from URL.
    MissingRouteParam {
        param_name: String,
    },
    /// DI container resolution error.
    Di(DiError),
    /// Method execution error.
    ExecutionError(String),
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationError { param_name, expected, actual } => {
                write!(
                    f,
                    "422 Unprocessable Entity: Route parameter '{}' expected {}, got '{}'",
                    param_name, expected, actual
                )
            }
            Self::NotFound { model, param_name, value } => {
                write!(
                    f,
                    "404 Not Found: Model '{}' not found for route parameter '{}' = '{}'",
                    model, param_name, value
                )
            }
            Self::MissingRouteParam { param_name } => {
                write!(f, "400 Bad Request: Missing route parameter '{}'", param_name)
            }
            Self::Di(e) => write!(f, "500 Internal Error: {}", e),
            Self::ExecutionError(msg) => write!(f, "500 Execution Error: {}", msg),
        }
    }
}

impl std::error::Error for DispatchError {}

impl From<DiError> for DispatchError {
    fn from(e: DiError) -> Self {
        Self::Di(e)
    }
}

/// Compile-time source for a controller method parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamSource {
    /// Injected from incoming HTTP request.
    Request {
        is_form_request: bool,
        form_request_class: Option<String>,
    },
    /// Route model binding: resolves via `Model::resolveRouteBinding($url_param)`.
    RouteModel {
        param_name: String,
        model_class: String,
    },
    /// Route scalar parameter with strict type validation.
    RouteParam {
        param_name: String,
        target_type: RhpType,
    },
    /// Service resolved from DI Container.
    Container {
        service_type: String,
    },
}

/// Fast pre-compiled dispatch shim for a controller method.
#[derive(Debug, Clone, PartialEq)]
pub struct HandlerShim {
    pub controller_class: String,
    pub method_name: String,
    pub route_path: String,
    pub param_sources: Vec<ParamSource>,
}

impl HandlerShim {
    /// Compile a handler shim for a controller method against a route path.
    /// Analyzes parameter sources once at route registration time, never per request!
    pub fn compile(
        controller_class: &str,
        method_name: &str,
        route_path: &str,
        metadata: &ProgramMetadata,
        hierarchy: Option<&TypeHierarchy>,
    ) -> Result<Self, String> {
        let clean_ctrl = controller_class.strip_prefix('\\').unwrap_or(controller_class);
        let class_meta = metadata
            .get_class(clean_ctrl)
            .ok_or_else(|| format!("Controller class '{}' not found in metadata", clean_ctrl))?;

        let method_meta = class_meta
            .methods
            .get(method_name)
            .ok_or_else(|| format!("Method '{}::{}' not found in metadata", clean_ctrl, method_name))?;

        let url_params = extract_url_param_names(route_path);
        let mut param_sources = Vec::with_capacity(method_meta.params.len());

        for param in &method_meta.params {
            let clean_param_name = param.name.strip_prefix('$').unwrap_or(&param.name).to_string();

            // 1. Is this Request or FormRequest?
            let type_name = match &param.rhp_type {
                RhpType::Class(cls) => Some(cls.strip_prefix('\\').unwrap_or(cls)),
                _ => None,
            };

            if let Some(t_name) = type_name {
                if t_name == "Request" || t_name == "RawRequest" {
                    param_sources.push(ParamSource::Request {
                        is_form_request: false,
                        form_request_class: None,
                    });
                    continue;
                }

                if t_name.ends_with("Request") && t_name != "Request" {
                    param_sources.push(ParamSource::Request {
                        is_form_request: true,
                        form_request_class: Some(t_name.to_string()),
                    });
                    continue;
                }

                // 2. Does this type implement RouteBindable (or declare resolveRouteBinding)?
                let is_route_bindable = is_type_route_bindable(t_name, metadata, hierarchy);
                if is_route_bindable && url_params.contains(&clean_param_name) {
                    param_sources.push(ParamSource::RouteModel {
                        param_name: clean_param_name,
                        model_class: t_name.to_string(),
                    });
                    continue;
                }
            }

            // 3. Is this a scalar parameter matching a URL segment?
            if url_params.contains(&clean_param_name) && is_scalar_type(&param.rhp_type) {
                param_sources.push(ParamSource::RouteParam {
                    param_name: clean_param_name,
                    target_type: param.rhp_type.clone(),
                });
                continue;
            }

            // 4. Anything else: ask the DI Container
            let service_type = match &param.rhp_type {
                RhpType::Class(cls) => cls.strip_prefix('\\').unwrap_or(cls).to_string(),
                _ => clean_param_name,
            };

            param_sources.push(ParamSource::Container { service_type });
        }

        Ok(Self {
            controller_class: clean_ctrl.to_string(),
            method_name: method_name.to_string(),
            route_path: route_path.to_string(),
            param_sources,
        })
    }

    /// Dispatch an incoming HTTP request directly through this shim with ZERO runtime reflection.
    pub fn dispatch(
        &self,
        vm: &mut VM,
        container: &mut Container,
        request: &WorkerRequest,
        route_params: &HashMap<String, String>,
    ) -> Result<PhpValue, DispatchError> {
        // 1. Resolve controller instance from container
        let controller = container
            .resolve(&self.controller_class, vm)
            .map_err(DispatchError::Di)?;

        // 2. Prepare method arguments directly according to pre-compiled sources
        let mut args = Vec::with_capacity(self.param_sources.len());

        for source in &self.param_sources {
            match source {
                ParamSource::Request { is_form_request, form_request_class } => {
                    let req_val = build_request_object(request);
                    if *is_form_request {
                        if let Some(cls) = form_request_class.as_ref() {
                            let form_req = vm
                                .instantiate_and_construct(cls, vec![req_val.clone()], Span::default())
                                .unwrap_or(req_val);
                            // Run validation method if defined on FormRequest
                            let _ = vm.invoke_method_sync(form_req.clone(), "validate", Vec::new(), Span::default());
                            args.push(form_req);
                            continue;
                        }
                    }
                    args.push(req_val);
                }
                ParamSource::RouteModel { param_name, model_class } => {
                    let raw_val = route_params
                        .get(param_name)
                        .ok_or_else(|| DispatchError::MissingRouteParam {
                            param_name: param_name.clone(),
                        })?;

                    // Direct static call to Model::resolveRouteBinding($value)
                    let model_inst = vm
                        .invoke_static_method_sync(
                            model_class,
                            "resolveRouteBinding",
                            vec![PhpValue::string(raw_val.clone())],
                            Span::default(),
                        )
                        .map_err(|e| DispatchError::ExecutionError(e.to_string()))?;

                    if model_inst.is_null() {
                        return Err(DispatchError::NotFound {
                            model: model_class.clone(),
                            param_name: param_name.clone(),
                            value: raw_val.clone(),
                        });
                    }

                    args.push(model_inst);
                }
                ParamSource::RouteParam { param_name, target_type } => {
                    let raw_val = route_params
                        .get(param_name)
                        .ok_or_else(|| DispatchError::MissingRouteParam {
                            param_name: param_name.clone(),
                        })?;

                    let parsed_val = parse_scalar_param(param_name, raw_val, target_type)?;
                    args.push(parsed_val);
                }
                ParamSource::Container { service_type } => {
                    let service = container
                        .resolve(service_type, vm)
                        .map_err(DispatchError::Di)?;
                    args.push(service);
                }
            }
        }

        // 3. Direct invocation of controller method with zero reflection!
        vm.invoke_method_sync(controller, &self.method_name, args, Span::default())
            .map_err(|e| DispatchError::ExecutionError(e.to_string()))
    }
}

/// Extract variable names inside `{param}` in a URL path template.
pub fn extract_url_param_names(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut name = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '}' {
                    chars.next();
                    break;
                }
                name.push(next_ch);
                chars.next();
            }
            let clean = name.trim_end_matches('?').to_string();
            if !clean.is_empty() {
                params.push(clean);
            }
        }
    }
    params
}

fn is_scalar_type(ty: &RhpType) -> bool {
    matches!(
        ty,
        RhpType::Int
            | RhpType::String
            | RhpType::Float
            | RhpType::Bool
            | RhpType::RustInt(_)
            | RhpType::RustFloat(_)
    )
}

fn is_type_route_bindable(
    type_name: &str,
    metadata: &ProgramMetadata,
    hierarchy: Option<&TypeHierarchy>,
) -> bool {
    let clean = type_name.strip_prefix('\\').unwrap_or(type_name);
    if let Some(cls) = metadata.get_class(clean) {
        if cls.implements_interface("RouteBindable") || cls.methods.contains_key("resolveRouteBinding") {
            return true;
        }
    }
    if let Some(h) = hierarchy {
        if let Some(cls) = h.classes.get(clean) {
            if cls.implements.iter().any(|i| i == "RouteBindable")
                || cls.methods.contains_key("resolveRouteBinding")
            {
                return true;
            }
        }
    }
    false
}

fn parse_scalar_param(
    param_name: &str,
    val_str: &str,
    target_type: &RhpType,
) -> Result<PhpValue, DispatchError> {
    match target_type {
        RhpType::Int | RhpType::RustInt(_) => {
            val_str
                .parse::<i64>()
                .map(|n| PhpValue::Int(n as i32))
                .map_err(|_| DispatchError::ValidationError {
                    param_name: param_name.to_string(),
                    expected: "integer".to_string(),
                    actual: val_str.to_string(),
                })
        }
        RhpType::Float | RhpType::RustFloat(_) => {
            val_str
                .parse::<f64>()
                .map(|f| PhpValue::Float(f as f32))
                .map_err(|_| DispatchError::ValidationError {
                    param_name: param_name.to_string(),
                    expected: "float".to_string(),
                    actual: val_str.to_string(),
                })
        }
        RhpType::Bool => match val_str.to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(PhpValue::Bool(true)),
            "0" | "false" | "no" | "off" => Ok(PhpValue::Bool(false)),
            _ => Err(DispatchError::ValidationError {
                param_name: param_name.to_string(),
                expected: "boolean".to_string(),
                actual: val_str.to_string(),
            }),
        },
        RhpType::String => Ok(PhpValue::string(val_str.to_string())),
        _ => Ok(PhpValue::string(val_str.to_string())),
    }
}

/// Construct a `Request` instance from incoming [`WorkerRequest`].
pub fn build_request_object(req: &WorkerRequest) -> PhpValue {
    let mut obj = VmObject::new("Request");

    let mut get_dict = indexmap::IndexMap::new();
    for (k, v) in &req.get_params {
        get_dict.insert(k.clone(), v.to_php_value());
    }
    obj.set_property("query", PhpValue::dict(get_dict));

    let mut post_dict = indexmap::IndexMap::new();
    for (k, v) in &req.post_params {
        post_dict.insert(k.clone(), v.to_php_value());
    }
    obj.set_property("request", PhpValue::dict(post_dict));

    let mut header_dict = indexmap::IndexMap::new();
    for (k, v) in &req.headers {
        header_dict.insert(k.clone(), PhpValue::string(v.clone()));
    }
    obj.set_property("headers", PhpValue::dict(header_dict));

    obj.set_property("id", PhpValue::string(req.id.clone()));

    PhpValue::object(obj)
}
