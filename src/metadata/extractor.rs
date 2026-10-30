//! # AST Metadata Extractor
//!
//! Extracts compile-time class, method, property, function, and attribute metadata
//! directly from typed AST nodes and nominal type hierarchies.

use std::collections::HashMap;

use crate::ast::{
    expr::{Argument, Expr, ExprKind, Literal},
    stmt::{
        Attribute, ClassDecl, ClassMemberKind, EnumDecl, FunctionDecl, InterfaceDecl, MethodDecl,
        Param, PropertyDecl, Stmt, StmtKind, TraitDecl,
    },
    types::TypeHint,
    Program,
};
use crate::metadata::registry::AttributeRegistry;
use crate::metadata::table::{
    AttributeArg, AttributeArgValue, ClassMetadata, CompiledAttribute, FunctionMetadata,
    MethodMetadata, ParamMetadata, ProgramMetadata, PropertyMetadata,
};
use crate::type_system::types::{RhpType, TypeHierarchy};

pub struct MetadataExtractor<'a> {
    _hierarchy: Option<&'a TypeHierarchy>,
    metadata: ProgramMetadata,
}

impl<'a> MetadataExtractor<'a> {
    pub fn new(hierarchy: Option<&'a TypeHierarchy>) -> Self {
        Self {
            _hierarchy: hierarchy,
            metadata: ProgramMetadata::new(),
        }
    }

    /// Extract complete metadata table from an AST [`Program`].
    pub fn extract(mut self, program: &Program<'_>) -> ProgramMetadata {
        self.extract_statements(&program.statements, None);

        // Run build-time attribute indexing across all discovered classes and methods
        let mut registry = AttributeRegistry::new();
        registry.scan_from_metadata(&self.metadata);
        self.metadata.attributes = registry;

        self.metadata
    }

    fn extract_statements(&mut self, statements: &[Stmt<'_>], namespace: Option<&str>) {
        for stmt in statements {
            match &stmt.kind {
                StmtKind::ClassDecl(class_decl) => {
                    self.extract_class(class_decl, namespace);
                }
                StmtKind::InterfaceDecl(iface_decl) => {
                    self.extract_interface(iface_decl, namespace);
                }
                StmtKind::TraitDecl(trait_decl) => {
                    self.extract_trait(trait_decl, namespace);
                }
                StmtKind::EnumDecl(enum_decl) => {
                    self.extract_enum(enum_decl, namespace);
                }
                StmtKind::FunctionDecl(func_decl) => {
                    self.extract_function(func_decl, namespace);
                }
                StmtKind::Namespace { name, body } => {
                    let ns_name = name.as_ref().map(|id| id.name);
                    if let Some(body_stmts) = body {
                        self.extract_statements(body_stmts, ns_name);
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_class(&mut self, class: &ClassDecl<'_>, namespace: Option<&str>) {
        let full_name = match namespace {
            Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, class.name.name),
            _ => class.name.name.to_string(),
        };

        let extends = class.extends.as_ref().map(|id| id.name.to_string());
        let implements = class.implements.iter().map(|id| id.name.to_string()).collect();
        let attributes = self.extract_attributes(&class.attributes);

        let mut constructor = None;
        let mut methods = HashMap::new();
        let mut properties = HashMap::new();

        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Method(method_decl) => {
                    let method_meta = self.extract_method(method_decl);
                    if method_decl.function.name.name == "__construct" {
                        constructor = Some(method_meta.clone());
                    }
                    methods.insert(method_decl.function.name.name.to_string(), method_meta);
                }
                ClassMemberKind::Property(prop_decl) => {
                    let prop_meta = self.extract_property(prop_decl);
                    let raw_name = prop_decl.name.name.to_string();
                    let clean_name = prop_decl.name.name.strip_prefix('$').unwrap_or(&prop_decl.name.name).to_string();
                    properties.insert(clean_name, prop_meta.clone());
                    if raw_name != prop_decl.name.name.strip_prefix('$').unwrap_or(&prop_decl.name.name) {
                        properties.insert(raw_name, prop_meta);
                    }
                }
                _ => {}
            }
        }

        let class_meta = ClassMetadata {
            name: full_name.clone(),
            extends,
            implements,
            constructor,
            methods,
            properties,
            attributes,
            is_abstract: class.modifiers.is_abstract,
            is_final: class.modifiers.is_final,
            is_singleton: class.modifiers.is_singleton,
        };

        self.metadata.string_to_type.insert(full_name.clone(), RhpType::Class(full_name.clone()));
        self.metadata.classes.insert(full_name, class_meta);
    }

    fn extract_interface(&mut self, iface: &InterfaceDecl<'_>, namespace: Option<&str>) {
        let full_name = match namespace {
            Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, iface.name.name),
            _ => iface.name.name.to_string(),
        };

        let extends = iface.extends.iter().map(|id| id.name.to_string()).collect();
        let attributes = self.extract_attributes(&iface.attributes);
        let mut methods = HashMap::new();

        for method_decl in &iface.methods {
            let method_meta = self.extract_method(method_decl);
            methods.insert(method_decl.function.name.name.to_string(), method_meta);
        }

        let class_meta = ClassMetadata {
            name: full_name.clone(),
            extends: None,
            implements: extends,
            constructor: None,
            methods,
            properties: HashMap::new(),
            attributes,
            is_abstract: true,
            is_final: false,
            is_singleton: false,
        };

        self.metadata.string_to_type.insert(full_name.clone(), RhpType::Class(full_name.clone()));
        self.metadata.classes.insert(full_name, class_meta);
    }

    fn extract_trait(&mut self, trait_decl: &TraitDecl<'_>, namespace: Option<&str>) {
        let full_name = match namespace {
            Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, trait_decl.name.name),
            _ => trait_decl.name.name.to_string(),
        };

        let attributes = self.extract_attributes(&trait_decl.attributes);
        let mut methods = HashMap::new();
        let mut properties = HashMap::new();

        for member in &trait_decl.members {
            match &member.kind {
                ClassMemberKind::Method(method_decl) => {
                    let method_meta = self.extract_method(method_decl);
                    methods.insert(method_decl.function.name.name.to_string(), method_meta);
                }
                ClassMemberKind::Property(prop_decl) => {
                    let prop_meta = self.extract_property(prop_decl);
                    let clean = prop_decl.name.name.strip_prefix('$').unwrap_or(&prop_decl.name.name).to_string();
                    properties.insert(clean, prop_meta);
                }
                _ => {}
            }
        }

        let class_meta = ClassMetadata {
            name: full_name.clone(),
            extends: None,
            implements: Vec::new(),
            constructor: None,
            methods,
            properties,
            attributes,
            is_abstract: true,
            is_final: false,
            is_singleton: false,
        };

        self.metadata.string_to_type.insert(full_name.clone(), RhpType::Class(full_name.clone()));
        self.metadata.classes.insert(full_name, class_meta);
    }

    fn extract_enum(&mut self, enum_decl: &EnumDecl<'_>, namespace: Option<&str>) {
        let full_name = match namespace {
            Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, enum_decl.name.name),
            _ => enum_decl.name.name.to_string(),
        };

        let implements = enum_decl.implements.iter().map(|id| id.name.to_string()).collect();
        let attributes = self.extract_attributes(&enum_decl.attributes);
        let mut methods = HashMap::new();

        for method_decl in &enum_decl.methods {
            let method_meta = self.extract_method(method_decl);
            methods.insert(method_decl.function.name.name.to_string(), method_meta);
        }

        let class_meta = ClassMetadata {
            name: full_name.clone(),
            extends: None,
            implements,
            constructor: None,
            methods,
            properties: HashMap::new(),
            attributes,
            is_abstract: false,
            is_final: true,
            is_singleton: false,
        };

        self.metadata.string_to_type.insert(full_name.clone(), RhpType::Class(full_name.clone()));
        self.metadata.classes.insert(full_name, class_meta);
    }

    fn extract_method(&self, method: &MethodDecl<'_>) -> MethodMetadata {
        let params = method
            .function
            .params
            .iter()
            .map(|p| self.extract_param(p))
            .collect();
        let return_type = method
            .function
            .return_type
            .as_ref()
            .map(|th| self.type_hint_to_rhp_type(th))
            .unwrap_or(RhpType::Mixed);
        let attributes = self.extract_attributes(&method.attributes);

        MethodMetadata {
            name: method.function.name.name.to_string(),
            params,
            return_type,
            attributes,
            is_static: method.is_static,
            is_abstract: method.is_abstract,
            visibility: method.visibility,
        }
    }

    fn extract_function(&mut self, func: &FunctionDecl<'_>, namespace: Option<&str>) {
        let full_name = match namespace {
            Some(ns) if !ns.is_empty() => format!("{}\\{}", ns, func.name.name),
            _ => func.name.name.to_string(),
        };

        let params = func.params.iter().map(|p| self.extract_param(p)).collect();
        let return_type = func
            .return_type
            .as_ref()
            .map(|th| self.type_hint_to_rhp_type(th))
            .unwrap_or(RhpType::Mixed);
        let attributes = self.extract_attributes(&func.attributes);

        let func_meta = FunctionMetadata {
            name: full_name.clone(),
            params,
            return_type,
            attributes,
            is_async: func.is_async,
        };

        self.metadata.functions.insert(full_name, func_meta);
    }

    fn extract_property(&self, prop: &PropertyDecl<'_>) -> PropertyMetadata {
        let rhp_type = prop
            .type_hint
            .as_ref()
            .map(|th| self.type_hint_to_rhp_type(th))
            .unwrap_or(RhpType::Mixed);
        let attributes = self.extract_attributes(&prop.attributes);

        PropertyMetadata {
            name: prop.name.name.strip_prefix('$').unwrap_or(&prop.name.name).to_string(),
            rhp_type,
            visibility: prop.visibility,
            is_static: prop.is_static,
            is_readonly: prop.is_readonly,
            attributes,
        }
    }

    fn extract_param(&self, param: &Param<'_>) -> ParamMetadata {
        let rhp_type = param
            .type_hint
            .as_ref()
            .map(|th| self.type_hint_to_rhp_type(th))
            .unwrap_or(RhpType::Mixed);
        let attributes = self.extract_attributes(&param.attributes);

        ParamMetadata {
            name: param.name.name.to_string(),
            rhp_type,
            has_default: param.default_value.is_some(),
            is_variadic: param.is_variadic,
            attributes,
        }
    }

    fn extract_attributes(&self, attrs: &[Attribute<'_>]) -> Vec<CompiledAttribute> {
        attrs
            .iter()
            .map(|a| {
                let name = a.name.name.to_string();
                let args = a.args.iter().map(|arg| self.extract_argument(arg)).collect();
                CompiledAttribute::new(name, args)
            })
            .collect()
    }

    fn extract_argument(&self, arg: &Argument<'_>) -> AttributeArg {
        let name = arg.name.as_ref().map(|id| id.name.to_string());
        let value = self.eval_expr_to_attribute_value(&arg.value);
        AttributeArg { name, value }
    }

    fn eval_expr_to_attribute_value(&self, expr: &Expr<'_>) -> AttributeArgValue {
        match &expr.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::String { value, .. } => AttributeArgValue::String(value.clone()),
                Literal::Int { value, .. } => AttributeArgValue::Int(*value),
                Literal::Float { value, .. } => AttributeArgValue::Float(*value),
                Literal::Bool(b) => AttributeArgValue::Bool(*b),
                Literal::Null => AttributeArgValue::Null,
                Literal::InterpolatedString { parts, .. } => {
                    let mut s = String::new();
                    for part in parts {
                        match part {
                            crate::ast::expr::StringPart::Literal(lit_str) => s.push_str(lit_str),
                            crate::ast::expr::StringPart::Expr(sub) => {
                                let sub_val = self.eval_expr_to_attribute_value(sub);
                                s.push_str(&sub_val.to_string());
                            }
                        }
                    }
                    AttributeArgValue::String(s)
                }
            },
            ExprKind::Array(elements) => {
                let has_keys = elements.iter().any(|e| e.key.is_some());
                if has_keys {
                    let mut pairs = Vec::new();
                    for elem in elements {
                        let k = elem
                            .key
                            .as_ref()
                            .map(|k_expr| self.eval_expr_to_attribute_value(k_expr).to_string())
                            .unwrap_or_else(|| pairs.len().to_string());
                        let v = self.eval_expr_to_attribute_value(&elem.value);
                        pairs.push((k, v));
                    }
                    AttributeArgValue::Map(pairs)
                } else {
                    let items = elements
                        .iter()
                        .map(|e| self.eval_expr_to_attribute_value(&e.value))
                        .collect();
                    AttributeArgValue::Array(items)
                }
            }
            ExprKind::ClassConstFetch { class, const_name } => {
                if const_name.name == "class" {
                    AttributeArgValue::Symbol(class.name.to_string())
                } else {
                    AttributeArgValue::Symbol(format!("{}::{}", class.name, const_name.name))
                }
            }
            ExprKind::Ident(id) => AttributeArgValue::Symbol(id.name.to_string()),
            ExprKind::Variable(var) => AttributeArgValue::Symbol(var.name.to_string()),
            _ => AttributeArgValue::String(format!("{:?}", expr)),
        }
    }

    fn type_hint_to_rhp_type(&self, hint: &TypeHint<'_>) -> RhpType {
        RhpType::from_type_hint(hint, None).unwrap_or(RhpType::Mixed)
    }
}

/// Convenience function to compile metadata from an AST Program.
pub fn extract_metadata(program: &Program<'_>, hierarchy: Option<&TypeHierarchy>) -> ProgramMetadata {
    MetadataExtractor::new(hierarchy).extract(program)
}
