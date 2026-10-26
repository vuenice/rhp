use std::collections::HashMap;

use crate::ast::{
    expr::{Argument, AssignOp, BinaryOp, Expr, ExprKind, Literal, UnaryOp},
    stmt::{
        ClassDecl, ClassMemberKind, EnumDecl, FunctionDecl, InterfaceDecl, MethodDecl, Param,
        PropertyDecl, Stmt, StmtKind, TraitAdaptation, TraitDecl,
    },
    types::{CastType, TypeHint, Visibility},
    visitor::AstVisitor,
    Program,
};
use crate::lexer::{
    facade::{is_static_facade, is_static_only},
    span::Span,
};
use crate::type_system::{
    env::{resolve_builtin_fluent_method, resolve_facade_static_call, TypeEnvironment},
    error::{TypeError, TypeErrorKind},
    types::{
        ClassTypeInfo, CollectionSubtype, EnumTypeInfo, FunctionSignature, InterfaceTypeInfo,
        ParamSignature, PropertyTypeInfo, RhpType, TraitAdaptationInfo, TraitUseInfo,
        TypeHierarchy,
    },
};

/// Output of the Medium/Gradual Type Checker containing resolved boundary contracts
/// and dynamically inferred body types.
#[derive(Debug, Clone, Default)]
pub struct TypeCheckResult {
    /// Strict boundary signatures for all standalone functions.
    pub functions: HashMap<String, FunctionSignature>,
    /// Class, interface, trait, and enum type hierarchy and member contracts.
    pub hierarchy: TypeHierarchy,
    /// Dynamically inferred local variable types per function or `Class::method` body.
    pub function_body_inferences: HashMap<String, HashMap<String, RhpType>>,
    /// Dynamically inferred variable types in the top-level script scope.
    pub top_level_inferences: HashMap<String, RhpType>,
    /// Inferred semantic type for every expression node indexed by its source `Span`.
    pub expr_types: HashMap<Span, RhpType>,
}

impl TypeCheckResult {
    /// Retrieve the inferred type of a local variable inside a function or `Class::method`.
    pub fn inferred_var_type(&self, callable_key: &str, var_name: &str) -> Option<&RhpType> {
        let key = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        self.function_body_inferences
            .get(callable_key)
            .and_then(|map| map.get(&key))
    }

    /// Retrieve the inferred type of a top-level script variable.
    pub fn top_level_var_type(&self, var_name: &str) -> Option<&RhpType> {
        let key = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        self.top_level_inferences.get(&key)
    }

    /// Retrieve the inferred type of an expression by its `Span`.
    pub fn expr_type(&self, span: &Span) -> Option<&RhpType> {
        self.expr_types.get(span)
    }
}

/// Active callable context during Pass 2 body type inference.
#[derive(Debug, Clone)]
struct CallableContext {
    callable_kind: String,
    callable_name: String,
    return_type: RhpType,
    enclosing_class: Option<String>,
    is_constructor: bool,
}

/// RHP Medium/Gradual Type Checker.
///
/// Enforces strict parameter and return type signatures across function and method boundaries
/// while performing dynamic type inference inside function bodies.
pub struct TypeChecker {
    result: TypeCheckResult,
    current_namespace: Option<String>,
    use_aliases: HashMap<String, String>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            result: TypeCheckResult::default(),
            current_namespace: None,
            use_aliases: HashMap::new(),
        }
    }

    /// Construct a new [`TypeChecker`] pre-seeded with a parent [`TypeHierarchy`].
    pub fn with_hierarchy(hierarchy: TypeHierarchy) -> Self {
        Self {
            result: TypeCheckResult {
                hierarchy,
                ..Default::default()
            },
            current_namespace: None,
            use_aliases: HashMap::new(),
        }
    }

    /// Execute the two-pass type checker over a parsed `Program`.
    pub fn check_program(mut self, program: &Program<'_>) -> Result<TypeCheckResult, TypeError> {
        // Pass 1: Collect and enforce strict boundary signatures across functions, classes, interfaces, traits, enums
        self.collect_declarations(&program.statements)?;
        self.compose_and_validate_traits()?;
        self.validate_hierarchy_contracts()?;

        self.current_namespace = None;
        self.use_aliases.clear();

        // Pass 2: Perform dynamic type inference within function/method bodies & top-level statements
        let mut top_env = TypeEnvironment::new();
        self.check_statements(&program.statements, &mut top_env, None)?;
        self.result.top_level_inferences = top_env.snapshot_types();

        Ok(self.result)
    }

    // =========================================================================
    // PASS 1: Strict Boundary Signature Collection & Hierarchy Validation
    // =========================================================================

    fn collect_declarations(&mut self, stmts: &[Stmt<'_>]) -> Result<(), TypeError> {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace { name, body } => {
                    let prev_ns = self.current_namespace.clone();
                    let prev_aliases = self.use_aliases.clone();
                    if body.is_none() && prev_ns.is_some() {
                        self.use_aliases.clear();
                    }
                    self.current_namespace = name.as_ref().map(|id| {
                        let n = id.name.strip_prefix('\\').unwrap_or(id.name);
                        n.to_string()
                    });
                    if let Some(inner) = body {
                        self.collect_declarations(inner)?;
                        self.current_namespace = prev_ns;
                        self.use_aliases = prev_aliases;
                    }
                }
                StmtKind::Use(imports) => {
                    for imp in imports {
                        let target = imp.name.name.strip_prefix('\\').unwrap_or(imp.name.name);
                        let alias = imp
                            .alias
                            .as_ref()
                            .map(|a| a.name.to_string())
                            .unwrap_or_else(|| {
                                target
                                    .rsplit('\\')
                                    .next()
                                    .unwrap_or(target)
                                    .to_string()
                            });
                        self.use_aliases.insert(alias, target.to_string());
                    }
                }
                StmtKind::FunctionDecl(func) => {
                    let func_name = if let Some(ref ns) = self.current_namespace {
                        format!("{}\\{}", ns, func.name.name)
                    } else {
                        func.name.name.to_string()
                    };
                    let sig = self.build_function_signature(
                        func,
                        "function",
                        &func_name,
                        None,
                        Visibility::Public,
                        false,
                        false,
                        false,
                    )?;
                    self.result.functions.insert(func_name.clone(), sig.clone());
                    if func_name != func.name.name {
                        self.result.functions.insert(func.name.name.to_string(), sig);
                    }
                }
                StmtKind::InterfaceDecl(iface) => {
                    let info = self.build_interface_info(iface)?;
                    let iface_name = if let Some(ref ns) = self.current_namespace {
                        format!("{}\\{}", ns, iface.name.name)
                    } else {
                        iface.name.name.to_string()
                    };
                    self.result
                        .hierarchy
                        .interfaces
                        .insert(iface_name.clone(), info.clone());
                    if iface_name != iface.name.name {
                        self.result
                            .hierarchy
                            .interfaces
                            .insert(iface.name.name.to_string(), info);
                    }
                }
                StmtKind::TraitDecl(tr) => {
                    let info = self.build_trait_info(tr)?;
                    let trait_name = if let Some(ref ns) = self.current_namespace {
                        format!("{}\\{}", ns, tr.name.name)
                    } else {
                        tr.name.name.to_string()
                    };
                    self.result
                        .hierarchy
                        .traits
                        .insert(trait_name.clone(), info.clone());
                    if trait_name != tr.name.name {
                        self.result
                            .hierarchy
                            .traits
                            .insert(tr.name.name.to_string(), info);
                    }
                }
                StmtKind::EnumDecl(enm) => {
                    let info = self.build_enum_info(enm)?;
                    let enum_name = if let Some(ref ns) = self.current_namespace {
                        format!("{}\\{}", ns, enm.name.name)
                    } else {
                        enm.name.name.to_string()
                    };
                    self.result
                        .hierarchy
                        .enums
                        .insert(enum_name.clone(), info.clone());
                    if enum_name != enm.name.name {
                        self.result
                            .hierarchy
                            .enums
                            .insert(enm.name.name.to_string(), info);
                    }
                }
                StmtKind::RustImport { path, alias } => {
                    let alias_str = alias.as_ref().map(|a| a.name);
                    let _ = crate::vm::rust_loader::load_and_register_rust_file(path, alias_str, None);
                }
                StmtKind::ClassDecl(cls) => {
                    let info = self.build_class_info(cls)?;
                    let class_name = if let Some(ref ns) = self.current_namespace {
                        format!("{}\\{}", ns, cls.name.name)
                    } else {
                        cls.name.name.to_string()
                    };
                    self.result
                        .hierarchy
                        .classes
                        .insert(class_name.clone(), info.clone());
                    if class_name != cls.name.name {
                        self.result
                            .hierarchy
                            .classes
                            .insert(cls.name.name.to_string(), info);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_function_signature(
        &self,
        func: &FunctionDecl<'_>,
        callable_kind: &str,
        display_name: &str,
        current_class: Option<&str>,
        visibility: Visibility,
        is_static: bool,
        is_abstract: bool,
        is_final: bool,
    ) -> Result<FunctionSignature, TypeError> {
        let is_lifecycle_magic =
            current_class.is_some() && matches!(func.name.name, "__construct" | "__destruct");

        let mut param_sigs = Vec::with_capacity(func.params.len());
        for param in &func.params {
            // Strict parameter type signature requirement across function/method boundaries
            let type_hint = param.type_hint.as_ref().ok_or_else(|| {
                TypeError::new(
                    TypeErrorKind::MissingParameterType {
                        callable_kind: callable_kind.to_string(),
                        callable_name: display_name.to_string(),
                        param_name: param.name.name.to_string(),
                    },
                    param.span,
                )
            })?;

            let rhp_type = self.resolve_type_hint(type_hint, current_class)?;
            if matches!(rhp_type, RhpType::Void | RhpType::Never) {
                return Err(TypeError::new(
                    TypeErrorKind::InvalidTypeAnnotation {
                        type_name: rhp_type.to_string(),
                        reason: format!(
                            "'{}' cannot be used as a parameter type for '{}'",
                            rhp_type, param.name.name
                        ),
                    },
                    type_hint.span,
                ));
            }

            param_sigs.push(ParamSignature {
                name: param.name.name.to_string(),
                rhp_type,
                has_default: param.default_value.is_some(),
                is_variadic: param.is_variadic,
                is_reference: param.is_reference,
                span: param.span,
            });
        }

        // 3. Strict return type signature requirement across function/method boundaries
        let return_type = if is_lifecycle_magic {
            if let Some(ref ret_hint) = func.return_type {
                let resolved = self.resolve_type_hint(ret_hint, current_class)?;
                if !matches!(resolved, RhpType::Void) {
                    return Err(TypeError::new(
                        TypeErrorKind::ConstructorReturnTypeForbidden {
                            class_name: current_class.unwrap_or_default().to_string(),
                            method_name: func.name.name.to_string(),
                            found_type: resolved.to_string(),
                        },
                        ret_hint.span,
                    ));
                }
            }
            RhpType::Void
        } else if let Some(ref ret_hint) = func.return_type {
            self.resolve_type_hint(ret_hint, current_class)?
        } else {
            let has_value_return = func
                .body
                .as_ref()
                .map(|b| Self::stmts_have_value_return(b))
                .unwrap_or(true);

            if has_value_return {
                return Err(TypeError::new(
                    TypeErrorKind::MissingReturnType {
                        callable_kind: callable_kind.to_string(),
                        callable_name: display_name.to_string(),
                    },
                    func.name.span,
                ));
            } else {
                RhpType::Void
            }
        };

        Ok(FunctionSignature {
            name: func.name.name.to_string(),
            params: param_sigs,
            return_type,
            is_async: func.is_async,
            is_static,
            is_abstract,
            is_final,
            visibility,
            span: func.span,
        })
    }

    fn build_interface_info(
        &self,
        iface: &InterfaceDecl<'_>,
    ) -> Result<InterfaceTypeInfo, TypeError> {
        let iface_name = iface.name.name;
        let mut methods = HashMap::new();
        for m in &iface.methods {
            let display_name = format!("{}::{}", iface_name, m.function.name.name);
            let sig = self.build_function_signature(
                &m.function,
                "interface method",
                &display_name,
                Some(iface_name),
                m.visibility,
                m.is_static,
                true,
                m.is_final,
            )?;
            methods.insert(m.function.name.name.to_string(), sig);
        }

        Ok(InterfaceTypeInfo {
            name: iface_name.to_string(),
            extends: iface.extends.iter().map(|i| self.resolve_class_name(i.name, None)).collect(),
            methods,
            span: iface.span,
        })
    }

    fn build_trait_info(&self, tr: &TraitDecl<'_>) -> Result<ClassTypeInfo, TypeError> {
        let trait_name = tr.name.name;
        let mut properties = HashMap::new();
        let mut constants = HashMap::new();
        let mut methods = HashMap::new();
        let mut traits = Vec::new();
        let mut trait_uses = Vec::new();

        for member in &tr.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    let prop_info = self.build_property_info(prop, trait_name, member.span)?;
                    properties.insert(prop.name.name.to_string(), prop_info);
                }
                ClassMemberKind::Method(m) => {
                    let display_name = format!("{}::{}", trait_name, m.function.name.name);
                    let sig = self.build_function_signature(
                        &m.function,
                        "trait method",
                        &display_name,
                        Some(trait_name),
                        m.visibility,
                        m.is_static,
                        m.is_abstract,
                        m.is_final,
                    )?;
                    methods.insert(m.function.name.name.to_string(), sig);
                }
                ClassMemberKind::Constant(c) => {
                    constants.insert(c.name.name.to_string(), RhpType::Mixed);
                }
                ClassMemberKind::TraitUse(tu) => {
                    let mut use_traits = Vec::new();
                    for t in &tu.traits {
                        let resolved = self.resolve_class_name(t.name, None);
                        use_traits.push(resolved.clone());
                        traits.push(resolved);
                    }
                    let mut adaptations = Vec::new();
                    for adapt in &tu.adaptations {
                        match adapt {
                            TraitAdaptation::InsteadOf(io) => {
                                adaptations.push(TraitAdaptationInfo::InsteadOf {
                                    span: io.span,
                                    trait_name: io
                                        .trait_name
                                        .as_ref()
                                        .map(|t| self.resolve_class_name(t.name, None)),
                                    method: io.method.name.to_string(),
                                    instead_of: io
                                        .instead_of
                                        .iter()
                                        .map(|t| self.resolve_class_name(t.name, None))
                                        .collect(),
                                });
                            }
                            TraitAdaptation::Alias(al) => {
                                adaptations.push(TraitAdaptationInfo::Alias {
                                    span: al.span,
                                    trait_name: al
                                        .trait_name
                                        .as_ref()
                                        .map(|t| self.resolve_class_name(t.name, None)),
                                    method: al.method.name.to_string(),
                                    visibility: al.visibility,
                                    alias: al.alias.as_ref().map(|a| a.name.to_string()),
                                });
                            }
                        }
                    }
                    trait_uses.push(TraitUseInfo {
                        traits: use_traits,
                        adaptations,
                        span: member.span,
                    });
                }
            }
        }

        Ok(ClassTypeInfo {
            name: trait_name.to_string(),
            is_abstract: false,
            is_final: false,
            is_readonly: false,
            is_static_only: false,
            is_singleton: false,
            extends: None,
            implements: Vec::new(),
            traits,
            trait_uses,
            properties,
            constants,
            methods,
            span: tr.span,
        })
    }

    fn build_enum_info(&self, enm: &EnumDecl<'_>) -> Result<EnumTypeInfo, TypeError> {
        let enum_name = enm.name.name;
        let backing_type = if let Some(ref hint) = enm.backing_type {
            let ty = self.resolve_type_hint(hint, Some(enum_name))?;
            if !matches!(ty, RhpType::Int | RhpType::String) {
                return Err(TypeError::new(
                    TypeErrorKind::InvalidTypeAnnotation {
                        type_name: ty.to_string(),
                        reason: "backed enum type must be 'int' or 'string'".to_string(),
                    },
                    hint.span,
                ));
            }
            Some(ty)
        } else {
            None
        };

        let mut methods = HashMap::new();
        for m in &enm.methods {
            let display_name = format!("{}::{}", enum_name, m.function.name.name);
            let sig = self.build_function_signature(
                &m.function,
                "enum method",
                &display_name,
                Some(enum_name),
                m.visibility,
                m.is_static,
                m.is_abstract,
                m.is_final,
            )?;
            methods.insert(m.function.name.name.to_string(), sig);
        }

        Ok(EnumTypeInfo {
            name: enum_name.to_string(),
            backing_type,
            implements: enm.implements.iter().map(|i| self.resolve_class_name(i.name, None)).collect(),
            cases: enm.cases.iter().map(|c| c.name.name.to_string()).collect(),
            methods,
            span: enm.span,
        })
    }

    fn build_class_info(&self, cls: &ClassDecl<'_>) -> Result<ClassTypeInfo, TypeError> {
        let class_name = cls.name.name;
        let mut properties = HashMap::new();
        let mut constants = HashMap::new();
        let mut methods = HashMap::new();
        let mut traits = Vec::new();
        let mut trait_uses = Vec::new();

        for member in &cls.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    let prop_info = self.build_property_info(prop, class_name, member.span)?;
                    properties.insert(prop.name.name.to_string(), prop_info);
                }
                ClassMemberKind::Method(m) => {
                    let display_name = format!("{}::{}", class_name, m.function.name.name);
                    let sig = self.build_function_signature(
                        &m.function,
                        "method",
                        &display_name,
                        Some(class_name),
                        m.visibility,
                        m.is_static,
                        m.is_abstract,
                        m.is_final,
                    )?;

                    // Handle constructor property promotion (`public readonly string $name` in `__construct`)
                    if m.function.name.name == "__construct" {
                        for (idx, param) in m.function.params.iter().enumerate() {
                            if let Some(vis) = param.visibility {
                                let promoted_type = sig.params[idx].rhp_type.clone();
                                properties.insert(
                                    param.name.name.to_string(),
                                    PropertyTypeInfo {
                                        name: param.name.name.to_string(),
                                        rhp_type: promoted_type,
                                        visibility: vis,
                                        is_static: false,
                                        is_readonly: param.is_readonly || cls.modifiers.is_readonly,
                                        has_default: param.default_value.is_some(),
                                        span: param.span,
                                    },
                                );
                            }
                        }
                    }

                    methods.insert(m.function.name.name.to_string(), sig);
                }
                ClassMemberKind::Constant(c) => {
                    constants.insert(c.name.name.to_string(), RhpType::Mixed);
                }
                ClassMemberKind::TraitUse(tu) => {
                    let mut use_traits = Vec::new();
                    for t in &tu.traits {
                        let resolved = self.resolve_class_name(t.name, None);
                        use_traits.push(resolved.clone());
                        traits.push(resolved);
                    }
                    let mut adaptations = Vec::new();
                    for adapt in &tu.adaptations {
                        match adapt {
                            TraitAdaptation::InsteadOf(io) => {
                                adaptations.push(TraitAdaptationInfo::InsteadOf {
                                    span: io.span,
                                    trait_name: io
                                        .trait_name
                                        .as_ref()
                                        .map(|t| self.resolve_class_name(t.name, None)),
                                    method: io.method.name.to_string(),
                                    instead_of: io
                                        .instead_of
                                        .iter()
                                        .map(|t| self.resolve_class_name(t.name, None))
                                        .collect(),
                                });
                            }
                            TraitAdaptation::Alias(al) => {
                                adaptations.push(TraitAdaptationInfo::Alias {
                                    span: al.span,
                                    trait_name: al
                                        .trait_name
                                        .as_ref()
                                        .map(|t| self.resolve_class_name(t.name, None)),
                                    method: al.method.name.to_string(),
                                    visibility: al.visibility,
                                    alias: al.alias.as_ref().map(|a| a.name.to_string()),
                                });
                            }
                        }
                    }
                    trait_uses.push(TraitUseInfo {
                        traits: use_traits,
                        adaptations,
                        span: member.span,
                    });
                }
            }
        }

        Ok(ClassTypeInfo {
            name: class_name.to_string(),
            is_abstract: cls.modifiers.is_abstract,
            is_final: cls.modifiers.is_final,
            is_readonly: cls.modifiers.is_readonly,
            is_static_only: cls.modifiers.is_static_only,
            is_singleton: cls.modifiers.is_singleton,
            extends: cls.extends.as_ref().map(|e| self.resolve_class_name(e.name, None)),
            implements: cls.implements.iter().map(|i| self.resolve_class_name(i.name, None)).collect(),
            traits,
            trait_uses,
            properties,
            constants,
            methods,
            span: cls.span,
        })
    }

    fn build_property_info(
        &self,
        prop: &PropertyDecl<'_>,
        class_name: &str,
        span: Span,
    ) -> Result<PropertyTypeInfo, TypeError> {
        let rhp_type = if let Some(ref hint) = prop.type_hint {
            let resolved = self.resolve_type_hint(hint, Some(class_name))?;
            if matches!(
                resolved,
                RhpType::Void | RhpType::Never | RhpType::Callable { .. }
            ) {
                return Err(TypeError::new(
                    TypeErrorKind::InvalidTypeAnnotation {
                        type_name: resolved.to_string(),
                        reason: format!(
                            "property '{}::{}' cannot have type '{}'",
                            class_name, prop.name.name, resolved
                        ),
                    },
                    hint.span,
                ));
            }
            resolved
        } else {
            RhpType::Dynamic
        };

        Ok(PropertyTypeInfo {
            name: prop.name.name.to_string(),
            rhp_type,
            visibility: prop.visibility,
            is_static: prop.is_static,
            is_readonly: prop.is_readonly,
            has_default: prop.default.is_some(),
            span,
        })
    }

    /// Compose used traits into declared traits and classes, resolving conflict adaptation rules (`insteadof`, `as`).
    fn compose_and_validate_traits(&mut self) -> Result<(), TypeError> {
        // Step 1: Compose traits that use other traits
        let trait_names: Vec<String> = self.result.hierarchy.traits.keys().cloned().collect();
        for tr_name in &trait_names {
            let mut tr_info = self.result.hierarchy.traits.get(tr_name).unwrap().clone();
            if !tr_info.traits.is_empty() {
                Self::compose_traits_into_type_info(tr_name, &mut tr_info, &self.result.hierarchy.traits)?;
                self.result.hierarchy.traits.insert(tr_name.clone(), tr_info);
            }
        }

        // Step 2: Compose traits into declared classes
        let class_names: Vec<String> = self.result.hierarchy.classes.keys().cloned().collect();
        for cls_name in &class_names {
            let mut cls_info = self.result.hierarchy.classes.get(cls_name).unwrap().clone();
            if !cls_info.traits.is_empty() {
                Self::compose_traits_into_type_info(cls_name, &mut cls_info, &self.result.hierarchy.traits)?;
                self.result.hierarchy.classes.insert(cls_name.clone(), cls_info);
            }
        }

        Ok(())
    }

    fn compose_traits_into_type_info(
        target_name: &str,
        target_info: &mut ClassTypeInfo,
        all_traits: &HashMap<String, ClassTypeInfo>,
    ) -> Result<(), TypeError> {
        // 1. Validate used traits exist
        for tr_name in &target_info.traits {
            let clean = tr_name.strip_prefix('\\').unwrap_or(tr_name);
            if !all_traits.contains_key(clean) && !all_traits.contains_key(tr_name) {
                return Err(TypeError::new(
                    TypeErrorKind::TraitAdaptationError {
                        class_name: target_name.to_string(),
                        reason: format!("Trait '{}' not found", tr_name),
                    },
                    target_info.span,
                ));
            }
        }

        let find_trait = |name: &str| -> Option<&ClassTypeInfo> {
            let clean = name.strip_prefix('\\').unwrap_or(name);
            all_traits.get(clean).or_else(|| all_traits.get(name))
        };

        let trait_matches = |t1: &str, t2: &str| -> bool {
            let c1 = t1.strip_prefix('\\').unwrap_or(t1);
            let c2 = t2.strip_prefix('\\').unwrap_or(t2);
            c1 == c2 || c1.ends_with(&format!("\\{}", c2)) || c2.ends_with(&format!("\\{}", c1))
        };

        // 2. Validate adaptations
        for use_decl in &target_info.trait_uses {
            for adapt in &use_decl.adaptations {
                match adapt {
                    TraitAdaptationInfo::InsteadOf { span, trait_name, method, instead_of } => {
                        if let Some(tn) = trait_name {
                            if !target_info.traits.iter().any(|t| trait_matches(t, tn)) {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait '{}' is not used by '{}'", tn, target_name),
                                    },
                                    *span,
                                ));
                            }
                            let tn_info = find_trait(tn).unwrap();
                            if !tn_info.methods.contains_key(method) {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait method '{}::{}' does not exist", tn, method),
                                    },
                                    *span,
                                ));
                            }
                        }

                        for ex in instead_of {
                            if !target_info.traits.iter().any(|t| trait_matches(t, ex)) {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait '{}' is not used by '{}'", ex, target_name),
                                    },
                                    *span,
                                ));
                            }
                            if let Some(tn) = trait_name {
                                if trait_matches(tn, ex) {
                                    return Err(TypeError::new(
                                        TypeErrorKind::TraitAdaptationError {
                                            class_name: target_name.to_string(),
                                            reason: format!("Cannot exclude trait '{}' from itself in insteadof rule", ex),
                                        },
                                        *span,
                                    ));
                                }
                            }
                            let ex_info = find_trait(ex).unwrap();
                            if !ex_info.methods.contains_key(method) {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait method '{}::{}' does not exist", ex, method),
                                    },
                                    *span,
                                ));
                            }
                        }
                    }
                    TraitAdaptationInfo::Alias { span, trait_name, method, visibility: _, alias: _ } => {
                        if let Some(tn) = trait_name {
                            if !target_info.traits.iter().any(|t| trait_matches(t, tn)) {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait '{}' is not used by '{}'", tn, target_name),
                                    },
                                    *span,
                                ));
                            }
                            let tn_info = find_trait(tn).unwrap();
                            if !tn_info.methods.contains_key(method) {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait method '{}::{}' does not exist", tn, method),
                                    },
                                    *span,
                                ));
                            }
                        } else {
                            let matches_count = target_info
                                .traits
                                .iter()
                                .filter(|t| find_trait(t).map(|ti| ti.methods.contains_key(method)).unwrap_or(false))
                                .count();
                            if matches_count == 0 {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("Trait method '{}' does not exist on any used traits", method),
                                    },
                                    *span,
                                ));
                            }
                            if matches_count > 1 {
                                return Err(TypeError::new(
                                    TypeErrorKind::TraitAdaptationError {
                                        class_name: target_name.to_string(),
                                        reason: format!("An alias rule for method '{}' is ambiguous; specify the trait explicitly (e.g. 'TraitName::{} as ...')", method, method),
                                    },
                                    *span,
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 3. Collect candidates for each method provided by used traits
        let mut candidates_map: HashMap<String, Vec<(String, FunctionSignature)>> = HashMap::new();
        for tr_name in &target_info.traits {
            if let Some(tr_info) = find_trait(tr_name) {
                for (m_name, sig) in &tr_info.methods {
                    candidates_map
                        .entry(m_name.clone())
                        .or_default()
                        .push((tr_name.clone(), sig.clone()));
                }
            }
        }

        // 4. Resolve collisions and compose methods
        for (m_name, mut candidates) in candidates_map {
            // Class/trait's own method takes precedence
            if target_info.methods.contains_key(&m_name) {
                continue;
            }

            if candidates.len() > 1 {
                // Apply insteadof exclusions
                for use_decl in &target_info.trait_uses {
                    for adapt in &use_decl.adaptations {
                        if let TraitAdaptationInfo::InsteadOf { method, instead_of, .. } = adapt {
                            if method == &m_name {
                                for ex in instead_of {
                                    candidates.retain(|(t, _)| !trait_matches(t, ex));
                                }
                            }
                        }
                    }
                }

                if candidates.len() == 1 {
                    let (_, sig) = candidates.remove(0);
                    target_info.methods.insert(m_name, sig);
                } else if candidates.len() > 1 {
                    return Err(TypeError::new(
                        TypeErrorKind::TraitMethodCollision {
                            class_name: target_name.to_string(),
                            trait_name: candidates[0].0.clone(),
                            conflicting_trait: candidates[1].0.clone(),
                            method_name: m_name,
                        },
                        target_info.span,
                    ));
                } else {
                    return Err(TypeError::new(
                        TypeErrorKind::TraitAdaptationError {
                            class_name: target_name.to_string(),
                            reason: format!("All traits providing method '{}' were excluded via insteadof", m_name),
                        },
                        target_info.span,
                    ));
                }
            } else if candidates.len() == 1 {
                let (_, sig) = candidates.remove(0);
                target_info.methods.insert(m_name, sig);
            }
        }

        // 5. Apply alias and visibility adaptations
        for use_decl in &target_info.trait_uses {
            for adapt in &use_decl.adaptations {
                if let TraitAdaptationInfo::Alias { trait_name, method, visibility, alias, .. } = adapt {
                    let orig_sig = if let Some(tn) = trait_name {
                        find_trait(tn).and_then(|ti| ti.methods.get(method)).cloned()
                    } else {
                        target_info
                            .traits
                            .iter()
                            .find_map(|t| find_trait(t).and_then(|ti| ti.methods.get(method)))
                            .cloned()
                    };

                    if let Some(orig) = orig_sig {
                        if let Some(alias_name) = alias {
                            let mut alias_sig = orig.clone();
                            alias_sig.name = format!("{}::{}", target_name, alias_name);
                            if let Some(vis) = visibility {
                                alias_sig.visibility = *vis;
                            }
                            target_info.methods.insert(alias_name.clone(), alias_sig);
                        } else if let Some(vis) = visibility {
                            // Visibility change on existing method
                            if let Some(m_sig) = target_info.methods.get_mut(method) {
                                m_sig.visibility = *vis;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate inheritance (`extends`) and interface (`implements`) boundary contracts across all declared classes.
    fn validate_hierarchy_contracts(&self) -> Result<(), TypeError> {
        let hierarchy = &self.result.hierarchy;

        for (class_name, class_info) in &hierarchy.classes {
            // 1. Validate parent class overrides (`extends`)
            if let Some(ref parent_name) = class_info.extends {
                if is_static_facade(parent_name) || is_static_only(parent_name) {
                    return Err(TypeError::new(
                        TypeErrorKind::StaticFacadeInheritanceForbidden {
                            class_name: parent_name.clone(),
                        },
                        class_info.span,
                    ));
                }

                if let Some(parent_info) = hierarchy.classes.get(parent_name) {
                    if parent_info.is_singleton && !class_info.is_singleton {
                        return Err(TypeError::new(
                            TypeErrorKind::NonSingletonExtendsSingletonForbidden {
                                class_name: class_name.clone(),
                                parent_name: parent_name.clone(),
                            },
                            class_info.span,
                        ));
                    }

                    if parent_info.is_static_only {
                        return Err(TypeError::new(
                            TypeErrorKind::StaticFacadeInheritanceForbidden {
                                class_name: parent_name.clone(),
                            },
                            class_info.span,
                        ));
                    }

                    if parent_info.is_final {
                        return Err(TypeError::new(
                            TypeErrorKind::MethodOverrideContractViolation {
                                class_name: class_name.clone(),
                                parent_name: parent_name.clone(),
                                method_name: "<class>".to_string(),
                                reason: format!("cannot extend final class '{}'", parent_name),
                            },
                            class_info.span,
                        ));
                    }

                    for (method_name, child_sig) in &class_info.methods {
                        if method_name == "__construct" || method_name == "__destruct" {
                            continue;
                        }
                        if let Some(parent_sig) = hierarchy.lookup_method(parent_name, method_name)
                        {
                            if parent_sig.is_final {
                                return Err(TypeError::new(
                                    TypeErrorKind::MethodOverrideContractViolation {
                                        class_name: class_name.clone(),
                                        parent_name: parent_name.clone(),
                                        method_name: method_name.clone(),
                                        reason: format!(
                                            "cannot override final method '{}::{}'",
                                            parent_name, method_name
                                        ),
                                    },
                                    child_sig.span,
                                ));
                            }

                            self.verify_signature_compatibility(
                                child_sig,
                                parent_sig,
                                class_name,
                                parent_name,
                                false,
                            )?;
                        }
                    }
                }
            }

            // 2. Validate interface implementations (`implements`)
            if !class_info.is_abstract {
                for iface_name in &class_info.implements {
                    self.verify_interface_implementation(class_name, class_info, iface_name)?;
                }
            }
        }

        Ok(())
    }

    fn verify_interface_implementation(
        &self,
        class_name: &str,
        class_info: &ClassTypeInfo,
        iface_name: &str,
    ) -> Result<(), TypeError> {
        let hierarchy = &self.result.hierarchy;
        if let Some(iface_info) = hierarchy.interfaces.get(iface_name) {
            for (method_name, iface_sig) in &iface_info.methods {
                let impl_sig = hierarchy
                    .lookup_method(class_name, method_name)
                    .ok_or_else(|| {
                        TypeError::new(
                            TypeErrorKind::InterfaceContractViolation {
                                class_name: class_name.to_string(),
                                interface_name: iface_name.to_string(),
                                reason: format!("missing required method '{}'", method_name),
                            },
                            class_info.span,
                        )
                    })?;

                self.verify_signature_compatibility(
                    impl_sig,
                    iface_sig,
                    class_name,
                    iface_name,
                    true,
                )?;
            }

            for parent_iface in &iface_info.extends {
                self.verify_interface_implementation(class_name, class_info, parent_iface)?;
            }
        }
        Ok(())
    }

    fn verify_signature_compatibility(
        &self,
        child_sig: &FunctionSignature,
        contract_sig: &FunctionSignature,
        class_name: &str,
        contract_owner: &str,
        is_interface: bool,
    ) -> Result<(), TypeError> {
        let hierarchy = Some(&self.result.hierarchy);

        // Required parameter count must not increase
        if child_sig.min_arity() > contract_sig.min_arity()
            || child_sig.params.len() < contract_sig.params.len()
        {
            let reason = format!(
                "parameter count mismatch: '{}' requires {} parameter(s), but '{}' has {}",
                contract_owner,
                contract_sig.params.len(),
                class_name,
                child_sig.params.len()
            );
            return Err(if is_interface {
                TypeError::new(
                    TypeErrorKind::InterfaceContractViolation {
                        class_name: class_name.to_string(),
                        interface_name: contract_owner.to_string(),
                        reason,
                    },
                    child_sig.span,
                )
            } else {
                TypeError::new(
                    TypeErrorKind::MethodOverrideContractViolation {
                        class_name: class_name.to_string(),
                        parent_name: contract_owner.to_string(),
                        method_name: child_sig.name.clone(),
                        reason,
                    },
                    child_sig.span,
                )
            });
        }

        // Parameter contravariance: contract parameter type must be assignable to child parameter type
        for (idx, contract_param) in contract_sig.params.iter().enumerate() {
            let child_param = &child_sig.params[idx];
            if !contract_param
                .rhp_type
                .is_assignable_to(&child_param.rhp_type, hierarchy)
            {
                let reason = format!(
                    "parameter #{} ('{}') type '{}' is not compatible with contract type '{}'",
                    idx + 1,
                    child_param.name,
                    child_param.rhp_type,
                    contract_param.rhp_type
                );
                return Err(if is_interface {
                    TypeError::new(
                        TypeErrorKind::InterfaceContractViolation {
                            class_name: class_name.to_string(),
                            interface_name: contract_owner.to_string(),
                            reason,
                        },
                        child_param.span,
                    )
                } else {
                    TypeError::new(
                        TypeErrorKind::MethodOverrideContractViolation {
                            class_name: class_name.to_string(),
                            parent_name: contract_owner.to_string(),
                            method_name: child_sig.name.clone(),
                            reason,
                        },
                        child_param.span,
                    )
                });
            }
        }

        // Return type covariance: child return type must be assignable to contract return type
        if !child_sig
            .return_type
            .is_assignable_to(&contract_sig.return_type, hierarchy)
        {
            let reason = format!(
                "return type '{}' is not compatible with contract return type '{}'",
                child_sig.return_type, contract_sig.return_type
            );
            return Err(if is_interface {
                TypeError::new(
                    TypeErrorKind::InterfaceContractViolation {
                        class_name: class_name.to_string(),
                        interface_name: contract_owner.to_string(),
                        reason,
                    },
                    child_sig.span,
                )
            } else {
                TypeError::new(
                    TypeErrorKind::MethodOverrideContractViolation {
                        class_name: class_name.to_string(),
                        parent_name: contract_owner.to_string(),
                        method_name: child_sig.name.clone(),
                        reason,
                    },
                    child_sig.span,
                )
            });
        }

        Ok(())
    }

    // =========================================================================
    // PASS 2: Dynamic Type Inference Within Function Bodies & Boundary Checks
    // =========================================================================

    fn check_statements(
        &mut self,
        stmts: &[Stmt<'_>],
        env: &mut TypeEnvironment,
        ctx: Option<&CallableContext>,
    ) -> Result<bool, TypeError> {
        let mut definitely_returns = false;

        for stmt in stmts {
            if self.check_stmt(stmt, env, ctx)? {
                definitely_returns = true;
            }
        }

        Ok(definitely_returns)
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt<'_>,
        env: &mut TypeEnvironment,
        ctx: Option<&CallableContext>,
    ) -> Result<bool, TypeError> {
        match &stmt.kind {
            StmtKind::Empty | StmtKind::RustImport { .. } | StmtKind::InterfaceDecl(_) => Ok(false),

            StmtKind::Namespace { name, body } => {
                let prev_ns = self.current_namespace.clone();
                let prev_aliases = self.use_aliases.clone();
                if body.is_none() && prev_ns.is_some() {
                    self.use_aliases.clear();
                }
                self.current_namespace = name.as_ref().map(|id| {
                    let n = id.name.strip_prefix('\\').unwrap_or(id.name);
                    n.to_string()
                });
                let res = if let Some(inner) = body {
                    let r = self.check_statements(inner, env, ctx)?;
                    self.current_namespace = prev_ns;
                    self.use_aliases = prev_aliases;
                    r
                } else {
                    false
                };
                Ok(res)
            }

            StmtKind::Use(imports) => {
                for imp in imports {
                    let target = imp.name.name.strip_prefix('\\').unwrap_or(imp.name.name);
                    let alias = imp
                        .alias
                        .as_ref()
                        .map(|a| a.name.to_string())
                        .unwrap_or_else(|| {
                            target
                                .rsplit('\\')
                                .next()
                                .unwrap_or(target)
                                .to_string()
                        });
                    self.use_aliases.insert(alias, target.to_string());
                }
                Ok(false)
            }

            StmtKind::Global(vars) => {
                for v in vars {
                    if env.get_type(v.name).is_none() {
                        let ty = self
                            .result
                            .top_level_inferences
                            .get(v.name)
                            .cloned()
                            .unwrap_or(RhpType::Dynamic);
                        let _ = env.assign_inferred(v.name, ty, v.span, Some(&self.result.hierarchy));
                    }
                }
                Ok(false)
            }

            StmtKind::Block(stmts) => self.check_statements(stmts, env, ctx),

            StmtKind::Expr(expr) => {
                let ty = self.infer_expr(expr, env, ctx, None)?;
                Ok(matches!(ty, RhpType::Never))
            }

            StmtKind::VarDecl {
                var,
                type_hint,
                init,
            } => {
                let current_class = ctx.and_then(|c| c.enclosing_class.as_deref());
                let init_ty = if let Some(init_expr) = init {
                    Some(self.infer_expr(init_expr, env, ctx, None)?)
                } else {
                    None
                };

                if let Some(hint) = type_hint {
                    let declared_ty = self.resolve_type_hint(hint, current_class)?;
                    env.declare_typed(
                        var.name,
                        declared_ty,
                        init_ty.as_ref(),
                        stmt.span,
                        Some(&self.result.hierarchy),
                    )?;
                } else if let Some(inferred) = init_ty {
                    env.assign_inferred(
                        var.name,
                        inferred,
                        stmt.span,
                        Some(&self.result.hierarchy),
                    )?;
                }
                Ok(false)
            }

            StmtKind::If {
                condition,
                then_branch,
                elseif_branches,
                else_branch,
            } => {
                self.infer_expr(condition, env, ctx, None)?;

                let mut then_env = env.clone();
                let then_returns = self.check_stmt(then_branch, &mut then_env, ctx)?;
                let mut merged_env = then_env;

                let mut all_elseif_return = true;
                for elif in elseif_branches {
                    self.infer_expr(&elif.condition, env, ctx, None)?;
                    let mut elif_env = env.clone();
                    let elif_ret = self.check_stmt(&elif.body, &mut elif_env, ctx)?;
                    if !elif_ret {
                        all_elseif_return = false;
                    }
                    merged_env.merge_branch(&elif_env);
                }

                if let Some(else_stmt) = else_branch {
                    let mut else_env = env.clone();
                    let else_returns = self.check_stmt(else_stmt, &mut else_env, ctx)?;
                    merged_env.merge_branch(&else_env);
                    *env = merged_env;
                    Ok(then_returns && all_elseif_return && else_returns)
                } else {
                    env.merge_branch(&merged_env);
                    Ok(false)
                }
            }

            StmtKind::While { condition, body } => {
                self.infer_expr(condition, env, ctx, None)?;
                let mut loop_env = env.clone();
                self.check_stmt(body, &mut loop_env, ctx)?;
                env.merge_branch(&loop_env);
                Ok(false)
            }

            StmtKind::DoWhile { body, condition } => {
                let mut loop_env = env.clone();
                self.check_stmt(body, &mut loop_env, ctx)?;
                self.infer_expr(condition, &mut loop_env, ctx, None)?;
                env.merge_branch(&loop_env);
                Ok(false)
            }

            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                for e in init {
                    self.infer_expr(e, env, ctx, None)?;
                }
                for e in condition {
                    self.infer_expr(e, env, ctx, None)?;
                }
                let mut loop_env = env.clone();
                self.check_stmt(body, &mut loop_env, ctx)?;
                for e in update {
                    self.infer_expr(e, &mut loop_env, ctx, None)?;
                }
                env.merge_branch(&loop_env);
                Ok(false)
            }

            StmtKind::Foreach {
                expr,
                key_var,
                value_var,
                body,
            } => {
                let iterable_ty = self.infer_expr(expr, env, ctx, None)?;
                let (key_ty, val_ty) = match &iterable_ty {
                    RhpType::Collection(sub) => (sub.key_type(), sub.element_type().clone()),
                    RhpType::Array(inner) | RhpType::Iterable(inner) => (
                        RhpType::Union(vec![RhpType::Int, RhpType::String]),
                        (**inner).clone(),
                    ),
                    RhpType::Tuple(items) => {
                        let mut acc: Option<RhpType> = None;
                        for item in items {
                            acc = Some(match acc {
                                None => item.clone(),
                                Some(prev) => RhpType::unify(&prev, item),
                            });
                        }
                        (RhpType::Int, acc.unwrap_or(RhpType::Mixed))
                    }
                    _ => (RhpType::Dynamic, RhpType::Dynamic),
                };

                let mut loop_env = env.clone();
                if let Some(kv) = key_var {
                    loop_env.assign_inferred(
                        kv.name,
                        key_ty,
                        kv.span,
                        Some(&self.result.hierarchy),
                    )?;
                }
                if let ExprKind::Variable(vv) = &value_var.kind {
                    loop_env.assign_inferred(
                        vv.name,
                        val_ty,
                        vv.span,
                        Some(&self.result.hierarchy),
                    )?;
                }
                self.check_stmt(body, &mut loop_env, ctx)?;
                env.merge_branch(&loop_env);
                Ok(false)
            }

            StmtKind::Switch { condition, cases } => {
                self.infer_expr(condition, env, ctx, None)?;
                let mut has_default = false;
                let mut all_return = !cases.is_empty();
                let mut merged_env = env.clone();

                for case in cases {
                    if let Some(ref val) = case.value {
                        self.infer_expr(val, env, ctx, None)?;
                    } else {
                        has_default = true;
                    }
                    let mut case_env = env.clone();
                    let case_returns = self.check_statements(&case.body, &mut case_env, ctx)?;
                    if !case_returns {
                        all_return = false;
                    }
                    merged_env.merge_branch(&case_env);
                }
                *env = merged_env;
                Ok(has_default && all_return)
            }

            StmtKind::Break(opt) | StmtKind::Continue(opt) => {
                if let Some(e) = opt {
                    self.infer_expr(e, env, ctx, None)?;
                }
                Ok(false)
            }

            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.infer_expr(e, env, ctx, None)?;
                }
                Ok(false)
            }

            StmtKind::Defer(inner) => {
                let mut defer_env = env.clone();
                self.check_stmt(inner, &mut defer_env, ctx)?;
                Ok(false)
            }

            StmtKind::Throw(expr) => {
                self.infer_expr(expr, env, ctx, None)?;
                Ok(true)
            }

            StmtKind::TryCatch {
                try_block,
                catches,
                finally_block,
            } => {
                let mut try_env = env.clone();
                let try_returns = self.check_statements(try_block, &mut try_env, ctx)?;
                let mut all_catches_return = true;

                for catch in catches {
                    let mut catch_env = env.clone();
                    let current_class = ctx.and_then(|c| c.enclosing_class.as_deref());
                    let catch_ty = if catch.types.len() == 1 {
                        let name = catch.types[0].name;
                        if matches!(name, "self" | "static") {
                            current_class
                                .map(|c| RhpType::Class(c.to_string()))
                                .unwrap_or(RhpType::Object)
                        } else {
                            let resolved = self.resolve_class_name(name, current_class);
                            RhpType::Class(resolved)
                        }
                    } else {
                        RhpType::Union(
                            catch
                                .types
                                .iter()
                                .map(|id| {
                                    let resolved = self.resolve_class_name(id.name, current_class);
                                    RhpType::Class(resolved)
                                })
                                .collect(),
                        )
                    };
                    catch_env.declare_typed(
                        catch.var.name,
                        catch_ty,
                        None,
                        catch.var.span,
                        Some(&self.result.hierarchy),
                    )?;
                    let catch_ret = self.check_statements(&catch.body, &mut catch_env, ctx)?;
                    if !catch_ret {
                        all_catches_return = false;
                    }
                    try_env.merge_branch(&catch_env);
                }

                if let Some(fin) = finally_block {
                    self.check_statements(fin, &mut try_env, ctx)?;
                }

                *env = try_env;
                Ok(try_returns && all_catches_return)
            }

            StmtKind::Return(opt_expr) => {
                let expr_ty = if let Some(e) = opt_expr {
                    Some(self.infer_expr(e, env, ctx, None)?)
                } else {
                    None
                };

                if let Some(callable) = ctx {
                    match (&callable.return_type, expr_ty) {
                        (RhpType::Never, _) => {
                            return Err(TypeError::new(
                                TypeErrorKind::NeverFunctionReturns {
                                    callable_kind: callable.callable_kind.clone(),
                                    callable_name: callable.callable_name.clone(),
                                },
                                stmt.span,
                            ));
                        }
                        (RhpType::Void, Some(found_ty)) => {
                            return Err(TypeError::new(
                                TypeErrorKind::VoidReturnWithValue {
                                    callable_kind: callable.callable_kind.clone(),
                                    callable_name: callable.callable_name.clone(),
                                    found: found_ty.to_string(),
                                },
                                stmt.span,
                            ));
                        }
                        (RhpType::Void, None) => {}
                        (expected, None) => {
                            if !matches!(expected, RhpType::Mixed | RhpType::Dynamic) {
                                return Err(TypeError::new(
                                    TypeErrorKind::ReturnTypeMismatch {
                                        callable_kind: callable.callable_kind.clone(),
                                        callable_name: callable.callable_name.clone(),
                                        expected: expected.to_string(),
                                        found: "void".to_string(),
                                    },
                                    stmt.span,
                                ));
                            }
                        }
                        (expected, Some(found_ty)) => {
                            if !found_ty.is_assignable_to(expected, Some(&self.result.hierarchy)) {
                                return Err(TypeError::new(
                                    TypeErrorKind::ReturnTypeMismatch {
                                        callable_kind: callable.callable_kind.clone(),
                                        callable_name: callable.callable_name.clone(),
                                        expected: expected.to_string(),
                                        found: found_ty.to_string(),
                                    },
                                    stmt.span,
                                ));
                            }
                        }
                    }
                }
                Ok(true)
            }

            StmtKind::FunctionDecl(func) => {
                let sig = self
                    .result
                    .functions
                    .get(func.name.name)
                    .cloned()
                    .expect("Function signature collected in Pass 1");
                self.check_callable_decl(
                    func,
                    &sig,
                    "function",
                    func.name.name,
                    func.name.name,
                    None,
                )?;
                Ok(false)
            }

            StmtKind::ClassDecl(cls) => {
                self.check_class_bodies(cls)?;
                Ok(false)
            }

            StmtKind::TraitDecl(tr) => {
                self.check_trait_bodies(tr)?;
                Ok(false)
            }

            StmtKind::EnumDecl(enm) => {
                self.check_enum_bodies(enm)?;
                Ok(false)
            }
        }
    }

    fn check_callable_decl(
        &mut self,
        func: &FunctionDecl<'_>,
        sig: &FunctionSignature,
        callable_kind: &str,
        display_name: &str,
        inference_key: &str,
        enclosing_class: Option<&str>,
    ) -> Result<(), TypeError> {
        let mut body_env = TypeEnvironment::new();

        // Bind `$this` inside instance methods
        if let Some(cls_name) = enclosing_class {
            if !sig.is_static {
                body_env.declare_typed(
                    "$this",
                    RhpType::Class(cls_name.to_string()),
                    None,
                    func.span,
                    Some(&self.result.hierarchy),
                )?;
            }
        }

        // Validate parameter default values and seed parameters in local `TypeEnvironment`
        for (idx, param) in func.params.iter().enumerate() {
            let param_sig = &sig.params[idx];
            if let Some(ref def_expr) = param.default_value {
                let def_ty = self.infer_expr(def_expr, &mut body_env, None, None)?;
                if !def_ty.is_assignable_to(&param_sig.rhp_type, Some(&self.result.hierarchy)) {
                    return Err(TypeError::new(
                        TypeErrorKind::ParameterDefaultTypeMismatch {
                            callable_name: display_name.to_string(),
                            param_name: param.name.name.to_string(),
                            expected: param_sig.rhp_type.to_string(),
                            found: def_ty.to_string(),
                        },
                        def_expr.span,
                    ));
                }
            }

            let effective_param_ty = if param.is_variadic {
                RhpType::Array(Box::new(param_sig.rhp_type.clone()))
            } else {
                param_sig.rhp_type.clone()
            };

            body_env.declare_typed(
                param.name.name,
                effective_param_ty,
                None,
                param.span,
                Some(&self.result.hierarchy),
            )?;
        }

        if let Some(ref body_stmts) = func.body {
            let ctx = CallableContext {
                callable_kind: callable_kind.to_string(),
                callable_name: display_name.to_string(),
                return_type: sig.return_type.clone(),
                enclosing_class: enclosing_class.map(|s| s.to_string()),
                is_constructor: enclosing_class.is_some() && func.name.name == "__construct",
            };

            let all_paths_return = self.check_statements(body_stmts, &mut body_env, Some(&ctx))?;

            // If body is non-empty (not an empty `{}` signature stub) and declares a concrete non-void return type,
            // verify that execution paths return a value.
            if !body_stmts.is_empty()
                && !all_paths_return
                && !matches!(
                    sig.return_type,
                    RhpType::Void | RhpType::Mixed | RhpType::Dynamic
                )
            {
                return Err(TypeError::new(
                    TypeErrorKind::MissingReturnStatement {
                        callable_kind: callable_kind.to_string(),
                        callable_name: display_name.to_string(),
                        expected: sig.return_type.to_string(),
                    },
                    func.span,
                ));
            }
        }

        self.result
            .function_body_inferences
            .insert(inference_key.to_string(), body_env.snapshot_types());

        Ok(())
    }

    fn check_class_bodies(&mut self, cls: &ClassDecl<'_>) -> Result<(), TypeError> {
        let class_name = cls.name.name;
        let class_info = self
            .result
            .hierarchy
            .classes
            .get(class_name)
            .cloned()
            .expect("Class collected in Pass 1");

        let mut init_env = TypeEnvironment::new();
        for member in &cls.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    if let Some(ref def_expr) = prop.default {
                        let def_ty = self.infer_expr(def_expr, &mut init_env, None, None)?;
                        if let Some(prop_info) = class_info.properties.get(prop.name.name) {
                            if !def_ty
                                .is_assignable_to(&prop_info.rhp_type, Some(&self.result.hierarchy))
                            {
                                return Err(TypeError::new(
                                    TypeErrorKind::PropertyDefaultTypeMismatch {
                                        class_name: class_name.to_string(),
                                        property_name: prop.name.name.to_string(),
                                        expected: prop_info.rhp_type.to_string(),
                                        found: def_ty.to_string(),
                                    },
                                    def_expr.span,
                                ));
                            }
                        }
                    }
                }
                ClassMemberKind::Method(MethodDecl { function, .. }) => {
                    let sig = class_info
                        .methods
                        .get(function.name.name)
                        .expect("Method collected in Pass 1");
                    let key = format!("{}::{}", class_name, function.name.name);
                    self.check_callable_decl(
                        function,
                        sig,
                        "method",
                        &key,
                        &key,
                        Some(class_name),
                    )?;
                }
                ClassMemberKind::Constant(c) => {
                    let const_ty = self.infer_expr(&c.value, &mut init_env, None, None)?;
                    if let Some(ci) = self.result.hierarchy.classes.get_mut(class_name) {
                        ci.constants.insert(c.name.name.to_string(), const_ty);
                    }
                }
                ClassMemberKind::TraitUse(_) => {}
            }
        }
        Ok(())
    }

    fn check_trait_bodies(&mut self, tr: &TraitDecl<'_>) -> Result<(), TypeError> {
        let trait_name = tr.name.name;
        let trait_info = self
            .result
            .hierarchy
            .traits
            .get(trait_name)
            .cloned()
            .expect("Trait collected in Pass 1");

        for member in &tr.members {
            if let ClassMemberKind::Method(MethodDecl { function, .. }) = &member.kind {
                let sig = trait_info
                    .methods
                    .get(function.name.name)
                    .expect("Trait method collected in Pass 1");
                let key = format!("{}::{}", trait_name, function.name.name);
                self.check_callable_decl(
                    function,
                    sig,
                    "trait method",
                    &key,
                    &key,
                    Some(trait_name),
                )?;
            }
        }
        Ok(())
    }

    fn check_enum_bodies(&mut self, enm: &EnumDecl<'_>) -> Result<(), TypeError> {
        let enum_name = enm.name.name;
        let enum_info = self
            .result
            .hierarchy
            .enums
            .get(enum_name)
            .cloned()
            .expect("Enum collected in Pass 1");

        let mut case_env = TypeEnvironment::new();
        for case in &enm.cases {
            if let Some(ref val_expr) = case.value {
                let val_ty = self.infer_expr(val_expr, &mut case_env, None, None)?;
                if let Some(ref backing) = enum_info.backing_type {
                    if !val_ty.is_assignable_to(backing, Some(&self.result.hierarchy)) {
                        return Err(TypeError::new(
                            TypeErrorKind::PropertyDefaultTypeMismatch {
                                class_name: enum_name.to_string(),
                                property_name: case.name.name.to_string(),
                                expected: backing.to_string(),
                                found: val_ty.to_string(),
                            },
                            val_expr.span,
                        ));
                    }
                }
            }
        }

        for m in &enm.methods {
            let sig = enum_info
                .methods
                .get(m.function.name.name)
                .expect("Enum method collected in Pass 1");
            let key = format!("{}::{}", enum_name, m.function.name.name);
            self.check_callable_decl(
                &m.function,
                sig,
                "enum method",
                &key,
                &key,
                Some(enum_name),
            )?;
        }

        Ok(())
    }

    // =========================================================================
    // Expression Dynamic Type Inference Engine
    // =========================================================================

    fn infer_expr(
        &mut self,
        expr: &Expr<'_>,
        env: &mut TypeEnvironment,
        ctx: Option<&CallableContext>,
        contextual_param_hints: Option<&[RhpType]>,
    ) -> Result<RhpType, TypeError> {
        let inferred = match &expr.kind {
            ExprKind::Literal(lit) => match lit {
                Literal::Int { .. } => RhpType::Int,
                Literal::Float { .. } => RhpType::Float,
                Literal::String { .. } => RhpType::String,
                Literal::InterpolatedString { parts, .. } => {
                    for part in parts {
                        if let crate::ast::expr::StringPart::Expr(inner_expr) = part {
                            self.infer_expr(inner_expr, env, ctx, None)?;
                        }
                    }
                    RhpType::String
                }
                Literal::Bool(_) => RhpType::Bool,
                Literal::Null => RhpType::Null,
            },

            ExprKind::Variable(var) => env.get_type(var.name).unwrap_or(RhpType::Dynamic),

            ExprKind::Ident(ident) => match ident.name {
                "self" | "static" => ctx
                    .and_then(|c| c.enclosing_class.as_ref())
                    .map(|cls| RhpType::Class(cls.clone()))
                    .unwrap_or(RhpType::Object),
                name => RhpType::Class(name.to_string()),
            },

            ExprKind::Grouping(inner) => self.infer_expr(inner, env, ctx, contextual_param_hints)?,

            ExprKind::Unary { op, expr: inner, .. } => {
                let operand_ty = self.infer_expr(inner, env, ctx, None)?;
                match op {
                    UnaryOp::Not => RhpType::Bool,
                    UnaryOp::Plus | UnaryOp::Neg => {
                        if matches!(
                            operand_ty,
                            RhpType::String | RhpType::Array(_) | RhpType::Collection(_)
                        ) {
                            return Err(TypeError::new(
                                TypeErrorKind::InvalidUnaryOperation {
                                    op: op.to_string(),
                                    operand_type: operand_ty.to_string(),
                                },
                                expr.span,
                            ));
                        }
                        operand_ty
                    }
                    UnaryOp::PreInc | UnaryOp::PostInc | UnaryOp::PreDec | UnaryOp::PostDec => {
                        if matches!(
                            operand_ty,
                            RhpType::Bool | RhpType::Array(_) | RhpType::Collection(_)
                        ) {
                            return Err(TypeError::new(
                                TypeErrorKind::InvalidUnaryOperation {
                                    op: op.to_string(),
                                    operand_type: operand_ty.to_string(),
                                },
                                expr.span,
                            ));
                        }
                        operand_ty
                    }
                    UnaryOp::BitNot => {
                        if operand_ty.is_integer() {
                            operand_ty
                        } else {
                            RhpType::Int
                        }
                    }
                    UnaryOp::Silence => operand_ty,
                }
            }

            ExprKind::Binary { op, left, right } => {
                let left_ty = self.infer_expr(left, env, ctx, None)?;
                let right_ty = self.infer_expr(right, env, ctx, None)?;
                self.infer_binary_op(*op, &left_ty, &right_ty, expr.span)?
            }

            ExprKind::Assign { op, target, value } => {
                self.check_cyclic_reference_assignment(target, value, expr.span)?;
                let rhs_ty = self.infer_expr(value, env, ctx, None)?;
                self.check_assignment(*op, target, rhs_ty, expr.span, env, ctx)?
            }

            ExprKind::Ternary {
                condition,
                if_true,
                if_false,
            } => {
                let cond_ty = self.infer_expr(condition, env, ctx, None)?;
                let false_ty = self.infer_expr(if_false, env, ctx, None)?;
                if let Some(true_expr) = if_true {
                    let true_ty = self.infer_expr(true_expr, env, ctx, None)?;
                    RhpType::unify(&true_ty, &false_ty)
                } else {
                    // Elvis `$cond ?: $false_branch`
                    RhpType::unify(&cond_ty.non_null_type(), &false_ty)
                }
            }

            ExprKind::Array(elements) => {
                let mut is_keyed = false;
                let mut all_string_keys = true;
                let mut key_types = Vec::with_capacity(elements.len());
                let mut elem_types = Vec::with_capacity(elements.len());
                for el in elements {
                    if let Some(ref k) = el.key {
                        is_keyed = true;
                        let kt = self.infer_expr(k, env, ctx, None)?;
                        if !matches!(kt, RhpType::String) {
                            all_string_keys = false;
                        }
                        key_types.push(kt);
                    } else {
                        all_string_keys = false;
                    }
                    elem_types.push(self.infer_expr(&el.value, env, ctx, None)?);
                }

                if elements.is_empty() {
                    RhpType::Array(Box::new(RhpType::Mixed))
                } else if !is_keyed {
                    // If all elements have the same concrete type (and not callables/futures), infer homogeneous Array<T> or Tuple
                    let first = &elem_types[0];
                    let all_same = elem_types.iter().all(|t| t == first);
                    if all_same && !matches!(first, RhpType::Callable { .. } | RhpType::Future(_))
                    {
                        RhpType::Array(Box::new(first.clone()))
                    } else {
                        RhpType::Tuple(elem_types)
                    }
                } else {
                    let mut unified_val: Option<RhpType> = None;
                    for t in &elem_types {
                        unified_val = Some(match unified_val {
                            None => t.clone(),
                            Some(prev) => RhpType::unify(&prev, t),
                        });
                    }
                    let val_ty = unified_val.unwrap_or(RhpType::Mixed);
                    if all_string_keys {
                        RhpType::Collection(CollectionSubtype::Dict(Box::new(val_ty)))
                    } else {
                        let mut unified_key: Option<RhpType> = None;
                        for kt in &key_types {
                            unified_key = Some(match unified_key {
                                None => kt.clone(),
                                Some(prev) => RhpType::unify(&prev, kt),
                            });
                        }
                        let key_ty = unified_key.unwrap_or(RhpType::Mixed);
                        RhpType::Collection(CollectionSubtype::HashMap(
                            Box::new(key_ty),
                            Box::new(val_ty),
                        ))
                    }
                }
            }

            ExprKind::ArrayIndex { array, index } => {
                let arr_ty = self.infer_expr(array, env, ctx, None)?;
                let idx_ty = if let Some(idx_expr) = index {
                    Some(self.infer_expr(idx_expr, env, ctx, None)?)
                } else {
                    None
                };

                match &arr_ty {
                    RhpType::Collection(sub) => sub.element_type().clone(),
                    RhpType::Array(inner) | RhpType::Iterable(inner) => (**inner).clone(),
                    RhpType::Tuple(items) => {
                        if let Some(idx_expr) = index {
                            if let ExprKind::Literal(Literal::Int { value, .. }) = idx_expr.kind {
                                if value >= 0 && (value as usize) < items.len() {
                                    items[value as usize].clone()
                                } else {
                                    RhpType::Mixed
                                }
                            } else {
                                let _ = idx_ty;
                                RhpType::Mixed
                            }
                        } else {
                            RhpType::Mixed
                        }
                    }
                    RhpType::String => RhpType::String,
                    _ => RhpType::Dynamic,
                }
            }

            ExprKind::List(elements) => {
                let mut types = Vec::new();
                for el in elements.iter().flatten() {
                    types.push(self.infer_expr(&el.value, env, ctx, None)?);
                }
                RhpType::Tuple(types)
            }

            ExprKind::ArrowFunction {
                params,
                return_type,
                body,
            } => {
                let current_class = ctx.and_then(|c| c.enclosing_class.as_deref());
                let mut fn_env = env.clone();
                let param_types =
                    self.bind_closure_params(params, &mut fn_env, current_class, contextual_param_hints)?;

                let inferred_body_ty = self.infer_expr(body, &mut fn_env, ctx, None)?;
                let final_ret_ty = if let Some(ret_hint) = return_type {
                    let declared_ret = self.resolve_type_hint(ret_hint, current_class)?;
                    if !inferred_body_ty
                        .is_assignable_to(&declared_ret, Some(&self.result.hierarchy))
                    {
                        return Err(TypeError::new(
                            TypeErrorKind::ReturnTypeMismatch {
                                callable_kind: "arrow function".to_string(),
                                callable_name: "fn(...)".to_string(),
                                expected: declared_ret.to_string(),
                                found: inferred_body_ty.to_string(),
                            },
                            body.span,
                        ));
                    }
                    declared_ret
                } else {
                    inferred_body_ty
                };

                RhpType::Callable {
                    params: Some(param_types),
                    return_type: Some(Box::new(final_ret_ty)),
                }
            }

            ExprKind::Closure {
                is_static,
                params,
                uses,
                return_type,
                body,
                ..
            } => {
                let current_class = ctx.and_then(|c| c.enclosing_class.as_deref());
                let mut closure_env = TypeEnvironment::new();
                if !*is_static {
                    let this_ty = if let Some(cls) = current_class {
                        RhpType::Class(cls.to_string())
                    } else {
                        RhpType::Dynamic
                    };
                    let _ = closure_env.declare_typed(
                        "$this",
                        this_ty,
                        None,
                        expr.span,
                        Some(&self.result.hierarchy),
                    );
                }

                // Validate `use ($x)` captures (reject `&$x` reference capture!)
                for u in uses {
                    if u.by_ref {
                        return Err(TypeError::new(
                            TypeErrorKind::ClosureReferenceCaptureForbidden {
                                var_name: u.var.name.to_string(),
                            },
                            u.span,
                        ));
                    }
                    if u.var.name == "$this" || u.var.name == "this" {
                        return Err(TypeError::new(
                            TypeErrorKind::CyclicClosureCaptureForbidden {
                                target: "$this".to_string(),
                            },
                            u.span,
                        ));
                    }
                    let captured_ty = env.get_type(u.var.name).unwrap_or(RhpType::Dynamic);
                    closure_env.assign_inferred(
                        u.var.name,
                        captured_ty,
                        u.span,
                        Some(&self.result.hierarchy),
                    )?;
                }

                let param_types = self.bind_closure_params(
                    params,
                    &mut closure_env,
                    current_class,
                    contextual_param_hints,
                )?;

                let declared_ret = if let Some(ret_hint) = return_type {
                    Some(self.resolve_type_hint(ret_hint, current_class)?)
                } else {
                    None
                };

                let closure_ctx = CallableContext {
                    callable_kind: "closure".to_string(),
                    callable_name: "function(...)".to_string(),
                    return_type: declared_ret.clone().unwrap_or(RhpType::Dynamic),
                    enclosing_class: current_class.map(|s| s.to_string()),
                    is_constructor: false,
                };

                self.check_statements(body, &mut closure_env, Some(&closure_ctx))?;

                let inferred_ret = declared_ret.unwrap_or_else(|| {
                    self.infer_closure_return_type(body, &closure_env)
                });

                RhpType::Callable {
                    params: Some(param_types),
                    return_type: Some(Box::new(inferred_ret)),
                }
            }

            ExprKind::Match { condition, arms } => {
                self.infer_expr(condition, env, ctx, None)?;
                let mut unified_arm_ty: Option<RhpType> = None;
                for arm in arms {
                    for pat in &arm.patterns {
                        self.infer_expr(pat, env, ctx, None)?;
                    }
                    let arm_ty = self.infer_expr(&arm.body, env, ctx, None)?;
                    unified_arm_ty = Some(match unified_arm_ty {
                        None => arm_ty,
                        Some(prev) => RhpType::unify(&prev, &arm_ty),
                    });
                }
                unified_arm_ty.unwrap_or(RhpType::Mixed)
            }

            ExprKind::New { class, args } => {
                let raw_name = match &class.kind {
                    ExprKind::Ident(id) => id.name,
                    _ => "object",
                };
                let class_name =
                    self.resolve_class_name(raw_name, ctx.and_then(|c| c.enclosing_class.as_deref()));

                if is_static_facade(&class_name) || is_static_only(&class_name) {
                    return Err(TypeError::new(
                        TypeErrorKind::StaticFacadeInstantiationForbidden {
                            class_name: class_name.clone(),
                        },
                        expr.span,
                    ));
                }

                if let Some(ci) = self.result.hierarchy.classes.get(&class_name) {
                    if ci.is_static_only {
                        return Err(TypeError::new(
                            TypeErrorKind::StaticFacadeInstantiationForbidden {
                                class_name: class_name.clone(),
                            },
                            expr.span,
                        ));
                    }

                    if ci.is_singleton {
                        if let Some(caller_ctx) = ctx {
                            if let Some(ref caller_class) = caller_ctx.enclosing_class {
                                let caller_is_singleton = self
                                    .result
                                    .hierarchy
                                    .classes
                                    .get(caller_class)
                                    .map(|c| c.is_singleton)
                                    .unwrap_or(false);
                                if !caller_is_singleton {
                                    return Err(TypeError::new(
                                        TypeErrorKind::SingletonInstantiationForbidden {
                                            singleton_class: class_name.clone(),
                                            caller_class: caller_class.clone(),
                                        },
                                        expr.span,
                                    ));
                                }
                            }
                        }
                    }
                }

                let arg_types = self.infer_arguments(args, env, ctx, None)?;

                if let Some(ctor_sig) = self
                    .result
                    .hierarchy
                    .lookup_method(&class_name, "__construct")
                    .cloned()
                {
                    let display = format!("{}::__construct", class_name);
                    self.verify_call_arguments(&display, &ctor_sig, args, &arg_types, expr.span)?;
                }

                RhpType::Class(class_name)
            }

            ExprKind::Clone(inner) => self.infer_expr(inner, env, ctx, None)?,

            ExprKind::Instanceof {
                expr: left,
                class: right,
            } => {
                self.infer_expr(left, env, ctx, None)?;
                self.infer_expr(right, env, ctx, None)?;
                RhpType::Bool
            }

            ExprKind::Cast { kind, expr: inner } => {
                self.infer_expr(inner, env, ctx, None)?;
                match kind {
                    CastType::Int => RhpType::Int,
                    CastType::Float => RhpType::Float,
                    CastType::String => RhpType::String,
                    CastType::Bool => RhpType::Bool,
                    CastType::Array => RhpType::Array(Box::new(RhpType::Mixed)),
                    CastType::Object => RhpType::Object,
                }
            }

            ExprKind::Yield { key, value, .. } => {
                if let Some(k) = key {
                    self.infer_expr(k, env, ctx, None)?;
                }
                if let Some(v) = value {
                    self.infer_expr(v, env, ctx, None)?;
                }
                RhpType::Dynamic
            }

            ExprKind::Await(inner) => {
                let inner_ty = self.infer_expr(inner, env, ctx, None)?;
                match inner_ty {
                    RhpType::Future(resolved) => *resolved,
                    other => other,
                }
            }

            ExprKind::PropertyFetch {
                object,
                property,
                is_nullsafe,
            } => {
                let obj_ty = self.infer_expr(object, env, ctx, None)?;
                let base_obj_ty = obj_ty.non_null_type();
                let prop_ty = if let RhpType::Class(ref cls_name) = base_obj_ty {
                    self.result
                        .hierarchy
                        .lookup_property(cls_name, property.name)
                        .map(|p| p.rhp_type.clone())
                        .unwrap_or(RhpType::Dynamic)
                } else {
                    RhpType::Dynamic
                };

                if *is_nullsafe || obj_ty.accepts_null() {
                    prop_ty.into_nullable()
                } else {
                    prop_ty
                }
            }

            ExprKind::StaticPropertyFetch { class, property } => {
                let resolved_cls =
                    self.resolve_class_name(class.name, ctx.and_then(|c| c.enclosing_class.as_deref()));
                self.result
                    .hierarchy
                    .lookup_property(&resolved_cls, property.name)
                    .map(|p| p.rhp_type.clone())
                    .unwrap_or(RhpType::Dynamic)
            }

            ExprKind::ClassConstFetch { class, const_name } => {
                if const_name.name == "class" {
                    RhpType::String
                } else {
                    let resolved_cls =
                        self.resolve_class_name(class.name, ctx.and_then(|c| c.enclosing_class.as_deref()));
                    if self.result.hierarchy.enums.contains_key(&resolved_cls) {
                        RhpType::Class(resolved_cls)
                    } else if let Some(cls_info) = self.result.hierarchy.classes.get(&resolved_cls) {
                        cls_info
                            .constants
                            .get(const_name.name)
                            .cloned()
                            .unwrap_or(RhpType::Mixed)
                    } else {
                        RhpType::Mixed
                    }
                }
            }

            ExprKind::Call { callee, args } => {
                if let ExprKind::Ident(ref fn_ident) = callee.kind {
                    if fn_ident.name == "async" {
                        let arg_types = self.infer_arguments(args, env, ctx, None)?;
                        let inner = arg_types.into_iter().next().unwrap_or(RhpType::Mixed);
                        return Ok(self.record_expr_type(expr.span, RhpType::Future(Box::new(inner))));
                    }

                    if let Some(sig) = self.result.functions.get(fn_ident.name).cloned() {
                        let param_hints: Vec<RhpType> =
                            sig.params.iter().map(|p| p.rhp_type.clone()).collect();
                        let arg_types =
                            self.infer_arguments(args, env, ctx, Some(&param_hints))?;
                        self.verify_call_arguments(
                            fn_ident.name,
                            &sig,
                            args,
                            &arg_types,
                            expr.span,
                        )?;
                        return Ok(self.record_expr_type(expr.span, sig.return_type));
                    }
                }

                let callee_ty = self.infer_expr(callee, env, ctx, None)?;
                let arg_types = self.infer_arguments(args, env, ctx, None)?;
                let ret_ty = match callee_ty {
                    RhpType::Callable {
                        return_type: Some(r),
                        ..
                    } => *r,
                    _ => RhpType::Dynamic,
                };
                let _ = arg_types;
                ret_ty
            }

            ExprKind::StaticCall {
                target,
                method,
                args,
            } => {
                let target_name = match &target.kind {
                    ExprKind::Ident(id) => id.name,
                    _ => "",
                };

                let resolved_target: String =
                    self.resolve_class_name(target_name, ctx.and_then(|c| c.enclosing_class.as_deref()));

                // 1. Check if calling a built-in Static Facade (`Arr`, `Str`, `Int`, `Float`, `Future`, `Sys`, `phpFun`, etc.)
                if is_static_facade(&resolved_target) {
                    let arg_types = self.infer_arguments(args, env, ctx, None)?;
                    resolve_facade_static_call(
                        &resolved_target,
                        method.name,
                        &arg_types,
                        expr.span,
                        Some(&self.result.hierarchy),
                    )?
                } else if let Some(sig) = self
                    .result
                    .hierarchy
                    .lookup_method(&resolved_target, method.name)
                    .cloned()
                {
                    // 2. Declared user class/enum/trait static method call
                    let param_hints: Vec<RhpType> =
                        sig.params.iter().map(|p| p.rhp_type.clone()).collect();
                    let arg_types = self.infer_arguments(args, env, ctx, Some(&param_hints))?;
                    let display = format!("{}::{}", resolved_target, method.name);
                    self.verify_call_arguments(&display, &sig, args, &arg_types, expr.span)?;
                    sig.return_type
                } else {
                    let _ = self.infer_arguments(args, env, ctx, None)?;
                    RhpType::Dynamic
                }
            }

            ExprKind::MethodCall {
                object,
                method,
                args,
                is_nullsafe,
            } => {
                let obj_ty = self.infer_expr(object, env, ctx, None)?;
                let base_obj_ty = obj_ty.non_null_type();

                // Provide contextual element type hints for collection functional methods (`map`, `filter`, `reject`, `find`, `reduce`)
                let collection_cb_hints: Option<Vec<RhpType>> =
                    if let RhpType::Collection(ref sub) = base_obj_ty {
                        let elem = sub.element_type().clone();
                        match method.name {
                            "map" | "filter" | "reject" | "find" => Some(vec![elem]),
                            "reduce" => Some(vec![elem.clone(), elem]),
                            _ => None,
                        }
                    } else {
                        None
                    };

                // 1. Built-in fluent methods (`Arr::list()->map()`, `Int::i64()->add()`, `Future->wait()`, `HTTP`, `Image`, `DB`)
                if let Some(ref cb_hints) = collection_cb_hints {
                    let mut arg_types = Vec::with_capacity(args.len());
                    for (i, arg) in args.iter().enumerate() {
                        let hints = if i == 0 {
                            Some(cb_hints.as_slice())
                        } else {
                            None
                        };
                        arg_types.push(self.infer_expr(&arg.value, env, ctx, hints)?);
                    }
                    if let Some(fluent_ret) =
                        resolve_builtin_fluent_method(&base_obj_ty, method.name, &arg_types)
                    {
                        let final_ty = if *is_nullsafe {
                            fluent_ret.into_nullable()
                        } else {
                            fluent_ret
                        };
                        return Ok(self.record_expr_type(expr.span, final_ty));
                    }
                }

                // 2. User-defined class/interface/trait/enum method calls
                if let RhpType::Class(ref cls_name) = base_obj_ty {
                    // Check singleton rule:
                    // "one can use both static and non-static methods of singleton class within other singleton class
                    //  but can only use static methods of singleton class inside non-singleton class."
                    if let Some(target_ci) = self.result.hierarchy.classes.get(cls_name) {
                        if target_ci.is_singleton {
                            if let Some(caller_ctx) = ctx {
                                if let Some(ref caller_class) = caller_ctx.enclosing_class {
                                    let caller_is_singleton = self
                                        .result
                                        .hierarchy
                                        .classes
                                        .get(caller_class)
                                        .map(|c| c.is_singleton)
                                        .unwrap_or(false);
                                    if !caller_is_singleton {
                                        return Err(TypeError::new(
                                            TypeErrorKind::NonStaticSingletonMethodCallForbidden {
                                                singleton_class: cls_name.clone(),
                                                method: method.name.to_string(),
                                                caller_class: caller_class.clone(),
                                            },
                                            expr.span,
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    if let Some(sig) = self
                        .result
                        .hierarchy
                        .lookup_method(cls_name, method.name)
                        .cloned()
                    {
                        let param_hints: Vec<RhpType> =
                            sig.params.iter().map(|p| p.rhp_type.clone()).collect();
                        let arg_types =
                            self.infer_arguments(args, env, ctx, Some(&param_hints))?;
                        let display = format!("{}::{}", cls_name, method.name);
                        self.verify_call_arguments(&display, &sig, args, &arg_types, expr.span)?;
                        let final_ty = if *is_nullsafe {
                            sig.return_type.into_nullable()
                        } else {
                            sig.return_type
                        };
                        return Ok(self.record_expr_type(expr.span, final_ty));
                    }
                }

                // 3. Built-in fluent methods without collection callback hints
                let arg_types = self.infer_arguments(args, env, ctx, None)?;
                if let Some(fluent_ret) =
                    resolve_builtin_fluent_method(&base_obj_ty, method.name, &arg_types)
                {
                    let final_ty = if *is_nullsafe {
                        fluent_ret.into_nullable()
                    } else {
                        fluent_ret
                    };
                    return Ok(self.record_expr_type(expr.span, final_ty));
                }

                if *is_nullsafe {
                    RhpType::Dynamic.into_nullable()
                } else {
                    RhpType::Dynamic
                }
            }
        };

        Ok(self.record_expr_type(expr.span, inferred))
    }

    #[inline]
    fn record_expr_type(&mut self, span: Span, ty: RhpType) -> RhpType {
        self.result.expr_types.insert(span, ty.clone());
        ty
    }

    fn infer_arguments(
        &mut self,
        args: &[Argument<'_>],
        env: &mut TypeEnvironment,
        ctx: Option<&CallableContext>,
        param_hints: Option<&[RhpType]>,
    ) -> Result<Vec<RhpType>, TypeError> {
        let mut types = Vec::with_capacity(args.len());
        for (i, arg) in args.iter().enumerate() {
            let contextual = param_hints.and_then(|hints| {
                if let Some(RhpType::Callable {
                    params: Some(cb_params),
                    ..
                }) = hints.get(i)
                {
                    Some(cb_params.as_slice())
                } else {
                    None
                }
            });
            types.push(self.infer_expr(&arg.value, env, ctx, contextual)?);
        }
        Ok(types)
    }

    fn bind_closure_params(
        &mut self,
        params: &[Param<'_>],
        fn_env: &mut TypeEnvironment,
        current_class: Option<&str>,
        contextual_param_hints: Option<&[RhpType]>,
    ) -> Result<Vec<RhpType>, TypeError> {
        let mut param_types = Vec::with_capacity(params.len());

        for (idx, param) in params.iter().enumerate() {
            let default_ty = if let Some(ref def_expr) = param.default_value {
                Some(self.infer_expr(def_expr, fn_env, None, None)?)
            } else {
                None
            };

            if let Some(ref hint) = param.type_hint {
                let declared = self.resolve_type_hint(hint, current_class)?;
                if let Some(ref def_t) = default_ty {
                    if !def_t.is_assignable_to(&declared, Some(&self.result.hierarchy)) {
                        return Err(TypeError::new(
                            TypeErrorKind::ParameterDefaultTypeMismatch {
                                callable_name: "closure".to_string(),
                                param_name: param.name.name.to_string(),
                                expected: declared.to_string(),
                                found: def_t.to_string(),
                            },
                            param.span,
                        ));
                    }
                }
                fn_env.declare_typed(
                    param.name.name,
                    declared.clone(),
                    default_ty.as_ref(),
                    param.span,
                    Some(&self.result.hierarchy),
                )?;
                param_types.push(declared);
            } else {
                // Dynamically infer closure parameter type from contextual hints (e.g. `Arr::list([1, 2])->map(fn($n) => $n * 2)`)
                // or default parameter value, falling back to `Dynamic`.
                let inferred = contextual_param_hints
                    .and_then(|hints| hints.get(idx).cloned())
                    .or(default_ty)
                    .unwrap_or(RhpType::Dynamic);
                fn_env.assign_inferred(
                    param.name.name,
                    inferred.clone(),
                    param.span,
                    Some(&self.result.hierarchy),
                )?;
                param_types.push(inferred);
            }
        }

        Ok(param_types)
    }

    fn infer_closure_return_type(&self, stmts: &[Stmt<'_>], env: &TypeEnvironment) -> RhpType {
        let mut ret_ty: Option<RhpType> = None;
        for stmt in stmts {
            if let StmtKind::Return(Some(ref expr)) = stmt.kind {
                if let Some(ty) = self.result.expr_types.get(&expr.span) {
                    ret_ty = Some(match ret_ty {
                        None => ty.clone(),
                        Some(prev) => RhpType::unify(&prev, ty),
                    });
                } else if let ExprKind::Variable(ref v) = expr.kind {
                    if let Some(ty) = env.get_type(v.name) {
                        ret_ty = Some(match ret_ty {
                            None => ty,
                            Some(prev) => RhpType::unify(&prev, &ty),
                        });
                    }
                }
            }
        }
        ret_ty.unwrap_or(RhpType::Dynamic)
    }

    fn infer_binary_op(
        &self,
        op: BinaryOp,
        left_ty: &RhpType,
        right_ty: &RhpType,
        span: Span,
    ) -> Result<RhpType, TypeError> {
        match op {
            // Arithmetic: +, -, *, /, %, **
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow => {
                // Reject non-numeric static primitives like string, bool, array in arithmetic
                let is_valid_operand = |t: &RhpType| -> bool {
                    t.is_numeric()
                        || matches!(t, RhpType::Dynamic | RhpType::Mixed)
                        || matches!(t, RhpType::Union(u) if u.iter().any(|m| m.is_numeric()))
                };

                if !is_valid_operand(left_ty) || !is_valid_operand(right_ty) {
                    return Err(TypeError::new(
                        TypeErrorKind::InvalidBinaryOperation {
                            op: op.to_string(),
                            left_type: left_ty.to_string(),
                            right_type: right_ty.to_string(),
                        },
                        span,
                    ));
                }

                if matches!(op, BinaryOp::Mod) {
                    return Ok(RhpType::Int);
                }

                // Preserve specific Rust numeric types when matching (`i64 + i64 -> i64`, `f64 * f64 -> f64`)
                match (left_ty, right_ty) {
                    (RhpType::RustFloat(f1), RhpType::RustFloat(f2)) if f1 == f2 => {
                        Ok(RhpType::RustFloat(*f1))
                    }
                    (RhpType::RustFloat(f), _) | (_, RhpType::RustFloat(f)) => {
                        Ok(RhpType::RustFloat(*f))
                    }
                    (RhpType::Float, _) | (_, RhpType::Float) => Ok(RhpType::Float),
                    (RhpType::RustInt(i1), RhpType::RustInt(i2)) if i1 == i2 => {
                        Ok(RhpType::RustInt(*i1))
                    }
                    (RhpType::RustInt(i), RhpType::Int) | (RhpType::Int, RhpType::RustInt(i)) => {
                        Ok(RhpType::RustInt(*i))
                    }
                    (RhpType::Int, RhpType::Int) => Ok(RhpType::Int),
                    _ => Ok(RhpType::Float),
                }
            }

            // String concatenation `.`
            BinaryOp::Concat => Ok(RhpType::String),

            // Bitwise operations
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                if let RhpType::RustInt(k) = left_ty {
                    Ok(RhpType::RustInt(*k))
                } else {
                    Ok(RhpType::Int)
                }
            }

            // Comparisons & Equality
            BinaryOp::Eq
            | BinaryOp::Identical
            | BinaryOp::NotEq
            | BinaryOp::NotIdentical
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::LtEq
            | BinaryOp::GtEq => Ok(RhpType::Bool),

            // Spaceship `<=>`
            BinaryOp::Spaceship => Ok(RhpType::Int),

            // Logical operations
            BinaryOp::LogicalAnd
            | BinaryOp::LogicalOr
            | BinaryOp::WordAnd
            | BinaryOp::WordOr
            | BinaryOp::WordXor => Ok(RhpType::Bool),

            // Null coalescing `$left ?? $right`
            BinaryOp::Coalesce => {
                if matches!(left_ty, RhpType::Null) {
                    Ok(right_ty.clone())
                } else if left_ty.accepts_null() {
                    Ok(RhpType::unify(&left_ty.non_null_type(), right_ty))
                } else {
                    Ok(left_ty.clone())
                }
            }
        }
    }

    fn check_assignment(
        &mut self,
        op: AssignOp,
        target: &Expr<'_>,
        rhs_ty: RhpType,
        span: Span,
        env: &mut TypeEnvironment,
        ctx: Option<&CallableContext>,
    ) -> Result<RhpType, TypeError> {
        // Compute effective assigned type for compound assignments (`+=`, `.=`, `??=`, etc.)
        let effective_rhs_ty = match op {
            AssignOp::Assign => rhs_ty,
            AssignOp::ConcatAssign => RhpType::String,
            AssignOp::AddAssign
            | AssignOp::SubAssign
            | AssignOp::MulAssign
            | AssignOp::DivAssign
            | AssignOp::ModAssign
            | AssignOp::PowAssign => {
                let lhs_ty = self.infer_expr(target, env, ctx, None)?;
                self.infer_binary_op(BinaryOp::Add, &lhs_ty, &rhs_ty, span)?
            }
            AssignOp::CoalesceAssign => {
                let lhs_ty = self.infer_expr(target, env, ctx, None)?;
                self.infer_binary_op(BinaryOp::Coalesce, &lhs_ty, &rhs_ty, span)?
            }
            AssignOp::BitAndAssign
            | AssignOp::BitOrAssign
            | AssignOp::BitXorAssign
            | AssignOp::ShlAssign
            | AssignOp::ShrAssign => RhpType::Int,
        };

        match &target.kind {
            // 1. Variable assignment `$x = ...`
            ExprKind::Variable(var) => env.assign_inferred(
                var.name,
                effective_rhs_ty,
                span,
                Some(&self.result.hierarchy),
            ),

            // 2. Array / Tuple destructuring `[$user, $posts] = Future::wait([...])`
            ExprKind::Array(elements) => {
                for (idx, el) in elements.iter().enumerate() {
                    let elem_ty = match &effective_rhs_ty {
                        RhpType::Tuple(items) => items.get(idx).cloned().unwrap_or(RhpType::Mixed),
                        RhpType::Collection(sub) => sub.element_type().clone(),
                        RhpType::Array(inner) | RhpType::Iterable(inner) => (**inner).clone(),
                        _ => RhpType::Dynamic,
                    };
                    if let ExprKind::Variable(ref v) = el.value.kind {
                        env.assign_inferred(
                            v.name,
                            elem_ty,
                            el.span,
                            Some(&self.result.hierarchy),
                        )?;
                    }
                }
                Ok(effective_rhs_ty)
            }

            ExprKind::List(elements) => {
                for (idx, opt_el) in elements.iter().enumerate() {
                    if let Some(el) = opt_el {
                        let elem_ty = match &effective_rhs_ty {
                            RhpType::Tuple(items) => {
                                items.get(idx).cloned().unwrap_or(RhpType::Mixed)
                            }
                            RhpType::Collection(sub) => sub.element_type().clone(),
                            RhpType::Array(inner) | RhpType::Iterable(inner) => (**inner).clone(),
                            _ => RhpType::Dynamic,
                        };
                        if let ExprKind::Variable(ref v) = el.value.kind {
                            env.assign_inferred(
                                v.name,
                                elem_ty,
                                el.span,
                                Some(&self.result.hierarchy),
                            )?;
                        }
                    }
                }
                Ok(effective_rhs_ty)
            }

            // 3. Instance property assignment `$this->prop = ...` or `$obj->prop = ...`
            ExprKind::PropertyFetch {
                object, property, ..
            } => {
                let obj_ty = self.infer_expr(object, env, ctx, None)?.non_null_type();
                if let RhpType::Class(ref cls_name) = obj_ty {
                    if let Some(prop_info) = self
                        .result
                        .hierarchy
                        .lookup_property(cls_name, property.name)
                        .cloned()
                    {
                        // Enforce readonly property immutability outside `__construct`
                        let is_in_own_constructor = ctx
                            .map(|c| {
                                c.is_constructor
                                    && c.enclosing_class.as_deref() == Some(cls_name.as_str())
                            })
                            .unwrap_or(false);
                        if prop_info.is_readonly && !is_in_own_constructor {
                            return Err(TypeError::new(
                                TypeErrorKind::ReadonlyPropertyMutation {
                                    class_name: cls_name.clone(),
                                    property_name: prop_info.name.clone(),
                                },
                                span,
                            ));
                        }

                        if !effective_rhs_ty
                            .is_assignable_to(&prop_info.rhp_type, Some(&self.result.hierarchy))
                        {
                            return Err(TypeError::new(
                                TypeErrorKind::PropertyAssignmentTypeMismatch {
                                    class_name: cls_name.clone(),
                                    property_name: prop_info.name.clone(),
                                    expected: prop_info.rhp_type.to_string(),
                                    found: effective_rhs_ty.to_string(),
                                },
                                span,
                            ));
                        }
                    }
                }
                Ok(effective_rhs_ty)
            }

            // 4. Static property assignment `self::$instances = ...`
            ExprKind::StaticPropertyFetch { class, property } => {
                let resolved_cls = if matches!(class.name, "self" | "static") {
                    ctx.and_then(|c| c.enclosing_class.as_deref())
                        .unwrap_or(class.name)
                } else {
                    class.name
                };

                if let Some(prop_info) = self
                    .result
                    .hierarchy
                    .lookup_property(resolved_cls, property.name)
                    .cloned()
                {
                    if !effective_rhs_ty
                        .is_assignable_to(&prop_info.rhp_type, Some(&self.result.hierarchy))
                    {
                        return Err(TypeError::new(
                            TypeErrorKind::PropertyAssignmentTypeMismatch {
                                class_name: resolved_cls.to_string(),
                                property_name: prop_info.name.clone(),
                                expected: prop_info.rhp_type.to_string(),
                                found: effective_rhs_ty.to_string(),
                            },
                            span,
                        ));
                    }
                }
                Ok(effective_rhs_ty)
            }

            // 5. Array index assignment `$arr[$i] = $val`
            ExprKind::ArrayIndex { array, index } => {
                self.infer_expr(array, env, ctx, None)?;
                if let Some(idx) = index {
                    self.infer_expr(idx, env, ctx, None)?;
                }
                Ok(effective_rhs_ty)
            }

            _ => Ok(effective_rhs_ty),
        }
    }

    /// Verify call-site arguments against a declared function or method `FunctionSignature`.
    fn verify_call_arguments(
        &self,
        callable_name: &str,
        sig: &FunctionSignature,
        args: &[Argument<'_>],
        arg_types: &[RhpType],
        call_span: Span,
    ) -> Result<(), TypeError> {
        let has_unpack = args.iter().any(|a| a.is_unpack);
        let min_args = sig.min_arity();
        let max_args = sig.max_arity();

        if !has_unpack {
            if args.len() < min_args {
                return Err(TypeError::new(
                    TypeErrorKind::ArityMismatch {
                        callable_name: callable_name.to_string(),
                        min_args,
                        max_args: max_args.unwrap_or(usize::MAX),
                        found_args: args.len(),
                    },
                    call_span,
                ));
            }
            if let Some(max) = max_args {
                if args.len() > max {
                    return Err(TypeError::new(
                        TypeErrorKind::ArityMismatch {
                            callable_name: callable_name.to_string(),
                            min_args,
                            max_args: max,
                            found_args: args.len(),
                        },
                        call_span,
                    ));
                }
            }
        }

        let hierarchy = Some(&self.result.hierarchy);

        for (idx, (arg, arg_ty)) in args.iter().zip(arg_types.iter()).enumerate() {
            if arg.is_unpack {
                continue;
            }

            // Resolve target parameter by named argument or position
            let target_param = if let Some(ref named_id) = arg.name {
                let target_name = format!("${}", named_id.name);
                sig.params
                    .iter()
                    .find(|p| p.name == target_name || p.name == named_id.name)
                    .ok_or_else(|| {
                        TypeError::new(
                            TypeErrorKind::UnknownNamedArgument {
                                callable_name: callable_name.to_string(),
                                arg_name: named_id.name.to_string(),
                            },
                            arg.span,
                        )
                    })?
            } else if let Some(p) = sig.params.get(idx) {
                p
            } else if let Some(last_variadic) = sig.params.last().filter(|p| p.is_variadic) {
                last_variadic
            } else {
                continue;
            };

            // Two-way pass-by-reference enforcement:
            // Both function declaration and function call must declare/pass the variable as &$var
            if target_param.is_reference {
                if !arg.is_reference {
                    let var_name = match &arg.value.kind {
                        ExprKind::Variable(v) => v.name.to_string(),
                        _ => target_param.name.clone(),
                    };
                    return Err(TypeError::new(
                        TypeErrorKind::MissingReferenceArgument {
                            callable_name: callable_name.to_string(),
                            param_name: target_param.name.clone(),
                            arg_index: idx + 1,
                            var_name,
                        },
                        arg.span,
                    ));
                }
                if !matches!(arg.value.kind, ExprKind::Variable(_)) {
                    return Err(TypeError::new(
                        TypeErrorKind::ReferenceArgumentMustBeVariable {
                            callable_name: callable_name.to_string(),
                            arg_index: idx + 1,
                        },
                        arg.span,
                    ));
                }
            } else if arg.is_reference {
                return Err(TypeError::new(
                    TypeErrorKind::UnexpectedReferenceArgument {
                        callable_name: callable_name.to_string(),
                        param_name: target_param.name.clone(),
                        arg_index: idx + 1,
                    },
                    arg.span,
                ));
            }

            if !arg_ty.is_assignable_to(&target_param.rhp_type, hierarchy) {
                return Err(TypeError::new(
                    TypeErrorKind::ArgumentTypeMismatch {
                        callable_name: callable_name.to_string(),
                        param_name: target_param.name.clone(),
                        arg_index: idx + 1,
                        expected: target_param.rhp_type.to_string(),
                        found: arg_ty.to_string(),
                    },
                    arg.span,
                ));
            }
        }

        Ok(())
    }

    fn stmts_have_value_return(stmts: &[crate::ast::stmt::Stmt<'_>]) -> bool {
        stmts.iter().any(Self::stmt_has_value_return)
    }

    fn stmt_has_value_return(stmt: &crate::ast::stmt::Stmt<'_>) -> bool {
        use crate::ast::stmt::StmtKind;
        match &stmt.kind {
            StmtKind::Return(Some(_)) => true,
            StmtKind::Block(inner) => Self::stmts_have_value_return(inner),
            StmtKind::If {
                then_branch,
                elseif_branches,
                else_branch,
                ..
            } => {
                Self::stmt_has_value_return(then_branch)
                    || elseif_branches.iter().any(|eb| Self::stmt_has_value_return(&eb.body))
                    || else_branch.as_ref().map(|b| Self::stmt_has_value_return(b)).unwrap_or(false)
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::Foreach { body, .. } => Self::stmt_has_value_return(body),
            StmtKind::Switch { cases, .. } => {
                cases.iter().any(|c| Self::stmts_have_value_return(&c.body))
            }
            StmtKind::TryCatch {
                try_block,
                catches,
                finally_block,
            } => {
                Self::stmts_have_value_return(try_block)
                    || catches.iter().any(|c| Self::stmts_have_value_return(&c.body))
                    || finally_block.as_ref().map(|b| Self::stmts_have_value_return(b)).unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Static AST pass preventing circular references at compile time.
    ///
    /// 1. Closure Capture Inspection: Detect closures or arrow functions capturing `$this`
    ///    that are assigned into `$this->property`, forming direct strong cycles.
    /// 2. Bidirectional Object Links: Require explicit `Weak::to($parent)` wrappers
    ///    on parent/owner object links to break circular reference graphs.
    fn check_cyclic_reference_assignment(
        &self,
        target: &Expr<'_>,
        value: &Expr<'_>,
        span: Span,
    ) -> Result<(), TypeError> {
        if let ExprKind::PropertyFetch { object, property, .. } = &target.kind {
            let is_target_this = matches!(&object.kind, ExprKind::Variable(v) if v.name == "$this" || v.name == "this");

            // 1. Inspect closures assigned to $this->prop
            if is_target_this {
                let captures_this = match &value.kind {
                    ExprKind::Closure { is_static, body, .. } => {
                        !*is_static && stmts_contain_this(body)
                    }
                    ExprKind::ArrowFunction { body, .. } => {
                        expr_contains_this(body)
                    }
                    _ => false,
                };

                if captures_this {
                    return Err(TypeError::new(
                        TypeErrorKind::CyclicClosureCaptureForbidden {
                            target: format!("$this->{}", property.name),
                        },
                        span,
                    ));
                }
            }

            // 2. Bidirectional parent-child relationship check
            if matches!(property.name, "parent" | "owner" | "container") {
                let is_weak = is_weak_reference_expression(value);
                if !is_weak {
                    let obj_name = match &object.kind {
                        ExprKind::Variable(v) => v.name,
                        _ => "child",
                    };
                    return Err(TypeError::new(
                        TypeErrorKind::CyclicReferenceRisk {
                            target: format!("${}->{}", obj_name.trim_start_matches('$'), property.name),
                        },
                        span,
                    ));
                }
            }
        }
        Ok(())
    }

    fn resolve_class_name(&self, raw_name: &str, enclosing_class: Option<&str>) -> String {
        match raw_name {
            "self" | "static" => enclosing_class.unwrap_or(raw_name).to_string(),
            "parent" => {
                if let Some(cls) = enclosing_class {
                    if let Some(info) = self.result.hierarchy.classes.get(cls) {
                        if let Some(ref p) = info.extends {
                            return p.clone();
                        }
                    }
                }
                "parent".to_string()
            }
            other => {
                // 1. Fully qualified name (starts with leading `\`)
                if let Some(stripped) = other.strip_prefix('\\') {
                    return stripped.to_string();
                }

                // 2. Direct match in use aliases (`use App\Models\User;` or `use App\Models\User as ModelUser;`)
                if let Some(aliased) = self.use_aliases.get(other) {
                    let clean = aliased.strip_prefix('\\').unwrap_or(aliased);
                    return clean.to_string();
                }

                // 3. Sub-namespace qualified name (e.g. `Models\User` where `use App\Models;`)
                if let Some((first_seg, rest)) = other.split_once('\\') {
                    if let Some(aliased_prefix) = self.use_aliases.get(first_seg) {
                        let clean_prefix = aliased_prefix.strip_prefix('\\').unwrap_or(aliased_prefix);
                        return format!("{}\\{}", clean_prefix, rest);
                    }
                    if let Some(ref ns) = self.current_namespace {
                        return format!("{}\\{}", ns, other);
                    }
                    return other.to_string();
                }

                // 4. Built-in facades
                if is_static_facade(other) || is_static_only(other) {
                    return other.to_string();
                }

                // 5. Unqualified name: if inside a namespace, qualify with current namespace
                if let Some(ref ns) = self.current_namespace {
                    return format!("{}\\{}", ns, other);
                }

                other.to_string()
            }
        }
    }

    pub fn resolve_type_hint(
        &self,
        hint: &TypeHint<'_>,
        current_class: Option<&str>,
    ) -> Result<RhpType, TypeError> {
        RhpType::from_type_hint_with_resolver(hint, current_class, &|raw| {
            self.resolve_class_name(raw, current_class)
        })
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

struct ThisFinder {
    found: bool,
}

impl<'s> crate::ast::visitor::AstVisitor<'s> for ThisFinder {
    fn visit_expr(&mut self, expr: &Expr<'s>) {
        if self.found {
            return;
        }
        if let ExprKind::Variable(v) = &expr.kind {
            if v.name == "$this" || v.name == "this" {
                self.found = true;
                return;
            }
        }
        crate::ast::visitor::walk_expr(self, expr);
    }
}

fn expr_contains_this(expr: &Expr<'_>) -> bool {
    let mut finder = ThisFinder { found: false };
    finder.visit_expr(expr);
    finder.found
}

fn stmts_contain_this(stmts: &[crate::ast::stmt::Stmt<'_>]) -> bool {
    let mut finder = ThisFinder { found: false };
    for stmt in stmts {
        finder.visit_stmt(stmt);
        if finder.found {
            return true;
        }
    }
    finder.found
}

fn is_weak_reference_expression(expr: &Expr<'_>) -> bool {
    match &expr.kind {
        ExprKind::StaticCall { target, method, .. } => {
            if method.name == "to" {
                if let ExprKind::Ident(id) = &target.kind {
                    return id.name == "Weak";
                }
            }
            false
        }
        ExprKind::Call { callee, .. } => match &callee.kind {
            ExprKind::StaticPropertyFetch { class, property } => {
                class.name == "Weak" && property.name == "to"
            }
            ExprKind::StaticCall { target, method, .. } => {
                if method.name == "to" {
                    if let ExprKind::Ident(id) = &target.kind {
                        return id.name == "Weak";
                    }
                }
                false
            }
            _ => false,
        },
        _ => false,
    }
}
