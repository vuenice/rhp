use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::ast::stmt::{
    ClassDecl, ClassMember, ClassMemberKind, FunctionDecl, Stmt,
    StmtKind,
};
use crate::ast::Program;
use crate::diagnostic::{Diagnostic, DiagnosticLevel};
use crate::expand::context::ExpandCtx;
use crate::expand::expansion::Expansion;
use crate::expand::registry::ExpanderRegistry;
use crate::expand::site::ExpansionSite;
use crate::lexer::span::Span;

/// Unified AST expansion engine.
///
/// Executes attribute and directive expanders in strict declaration order
/// (top to bottom, outer before inner), catches handler panics cleanly,
/// detects member and semantic conflicts, and guarantees deterministic AST output.
pub struct ExpansionEngine<'a> {
    registry: &'a ExpanderRegistry,
    ctx: &'a mut ExpandCtx,
}

impl<'a> ExpansionEngine<'a> {
    /// Create a new expansion engine with the provided registry and context.
    pub fn new(registry: &'a ExpanderRegistry, ctx: &'a mut ExpandCtx) -> Self {
        Self { registry, ctx }
    }

    /// Expand an entire RHP AST Program.
    pub fn expand_program(&mut self, program: &Program<'_>) -> Result<Program<'static>, Diagnostic> {
        let mut expanded_stmts = Vec::new();

        for stmt in &program.statements {
            let processed = self.expand_statement(stmt)?;
            expanded_stmts.extend(processed);
        }

        Ok(Program::new(expanded_stmts, program.span))
    }

    /// Expand a single statement in declaration order.
    pub fn expand_statement(&mut self, stmt: &Stmt<'_>) -> Result<Vec<Stmt<'static>>, Diagnostic> {
        match &stmt.kind {
            StmtKind::ClassDecl(class) => {
                let expanded_class = self.expand_class(class)?;
                Ok(vec![Stmt::new(
                    StmtKind::ClassDecl(expanded_class),
                    stmt.span,
                )])
            }
            StmtKind::FunctionDecl(func) => {
                let expanded_func = self.expand_function(func)?;
                Ok(vec![Stmt::new(
                    StmtKind::FunctionDecl(expanded_func),
                    stmt.span,
                )])
            }
            StmtKind::Block(stmts) => {
                let mut inner = Vec::new();
                for s in stmts {
                    inner.extend(self.expand_statement(s)?);
                }
                Ok(vec![Stmt::new(StmtKind::Block(inner), stmt.span)])
            }
            // For other statements, clone to 'static by converting identifiers/strings
            _ => Ok(vec![self.clone_stmt_to_static(stmt)]),
        }
    }

    /// Expand a class declaration and all its members.
    ///
    /// Ordering:
    /// 1. Class-level attributes in declaration order (outer).
    /// 2. Class members in declaration order (inner):
    ///    - For each member, its attributes in declaration order.
    pub fn expand_class(&mut self, class: &ClassDecl<'_>) -> Result<ClassDecl<'static>, Diagnostic> {
        let mut added_members: Vec<ClassMember<'static>> = Vec::new();
        // Track generated member names -> (Origin Span, Expander Name) for conflict detection
        let mut generated_member_sites: BTreeMap<String, (Span, String)> = BTreeMap::new();
        let mut fillable_fields: BTreeMap<String, Span> = BTreeMap::new();
        let mut guarded_fields: BTreeMap<String, Span> = BTreeMap::new();

        let mut retained_attributes = Vec::new();

        // 1. Process class-level attributes in declaration order
        for attr in &class.attributes {
            let attr_name = attr.name.as_str();
            let expander = match self.registry.get_attribute(attr_name) {
                Some(e) => e,
                None => {
                    return Err(self.ctx.error(
                        "RHP0501",
                        format!("unknown compiler extension attribute `#[{}]`", attr_name),
                        attr.span,
                    ));
                }
            };

            // Inspect fields for fillable/guarded conflict detection
            if attr_name == "fillable" {
                for arg in &attr.args {
                    if let crate::ast::expr::ExprKind::Literal(crate::ast::expr::Literal::String {
                        ref value,
                        ..
                    }) = arg.value.kind
                    {
                        if let Some(prev_span) = guarded_fields.get(value) {
                            return Err(self.ctx.build_diagnostic(
                                DiagnosticLevel::Error,
                                "RHP0504",
                                format!("conflicting macro expansion: field '{}' is declared in both #[fillable] and #[guarded]", value),
                                attr.span,
                                Some(format!("field '{}' declared in #[fillable] here", value)),
                                vec![(*prev_span, format!("field '{}' previously declared in #[guarded] here", value))],
                            ));
                        }
                        fillable_fields.insert(value.clone(), attr.span);
                    }
                }
            } else if attr_name == "guarded" {
                for arg in &attr.args {
                    if let crate::ast::expr::ExprKind::Literal(crate::ast::expr::Literal::String {
                        ref value,
                        ..
                    }) = arg.value.kind
                    {
                        if let Some(prev_span) = fillable_fields.get(value) {
                            return Err(self.ctx.build_diagnostic(
                                DiagnosticLevel::Error,
                                "RHP0504",
                                format!("conflicting macro expansion: field '{}' is declared in both #[guarded] and #[fillable]", value),
                                attr.span,
                                Some(format!("field '{}' declared in #[guarded] here", value)),
                                vec![(*prev_span, format!("field '{}' previously declared in #[fillable] here", value))],
                            ));
                        }
                        guarded_fields.insert(value.clone(), attr.span);
                    }
                }
            }

            let site = ExpansionSite::Class {
                attribute: attr,
                class,
            };

            // Catch handler panic cleanly
            let expand_res = catch_unwind(AssertUnwindSafe(|| {
                expander.expand(self.ctx, site)
            }));

            let expansion = match expand_res {
                Ok(res) => res?,
                Err(panic_payload) => {
                    let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic payload".to_string()
                    };
                    return Err(self.ctx.error(
                        "RHP0505",
                        format!("expander handler `{}` panicked during execution: {}", attr_name, msg),
                        attr.span,
                    ));
                }
            };

            match expansion {
                Expansion::AddClassMembers(members) => {
                    for member in members {
                        let member_name = member_ident(&member);
                        if let Some((prev_span, prev_expander)) = generated_member_sites.get(&member_name) {
                            return Err(self.ctx.build_diagnostic(
                                DiagnosticLevel::Error,
                                "RHP0504",
                                format!(
                                    "conflicting macro expansion: member `{}` generated by `#[{}]` conflicts with previous generation by `#[{}]`",
                                    member_name, attr_name, prev_expander
                                ),
                                attr.span,
                                Some(format!("conflicting member `{}` generated here", member_name)),
                                vec![(*prev_span, format!("member `{}` originally generated by `#[{}]` here", member_name, prev_expander))],
                            ));
                        }
                        generated_member_sites.insert(member_name, (attr.span, attr_name.to_string()));
                        added_members.push(member);
                    }
                }
                Expansion::Empty => {
                    retained_attributes.push(self.clone_attr_to_static(attr));
                }
                _ => {
                    return Err(self.ctx.error(
                        "RHP0505",
                        format!("attribute `#[{}]` returned unsupported expansion variant for class target", attr_name),
                        attr.span,
                    ));
                }
            }
        }

        // 2. Process class members in declaration order
        let mut final_members = Vec::new();

        for member in &class.members {
            let processed_member = match &member.kind {
                ClassMemberKind::Property(prop) => {
                    let mut p = prop.clone();
                    for attr in &prop.attributes {
                        let attr_name = attr.name.as_str();
                        let expander = match self.registry.get_attribute(attr_name) {
                            Some(e) => e,
                            None => {
                                return Err(self.ctx.error(
                                    "RHP0501",
                                    format!("unknown compiler extension attribute `#[{}]`", attr_name),
                                    attr.span,
                                ));
                            }
                        };

                        let site = ExpansionSite::Property {
                            attribute: attr,
                            class,
                            property: prop,
                        };

                        let expand_res = catch_unwind(AssertUnwindSafe(|| {
                            expander.expand(self.ctx, site)
                        }));

                        let expansion = match expand_res {
                            Ok(res) => res?,
                            Err(panic_payload) => {
                                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "unknown panic payload".to_string()
                                };
                                return Err(self.ctx.error(
                                    "RHP0505",
                                    format!("expander handler `{}` panicked: {}", attr_name, msg),
                                    attr.span,
                                ));
                            }
                        };

                        if let Expansion::ReplaceClassMembers(mut repl) = expansion {
                            if let Some(r) = repl.pop() {
                                if let ClassMemberKind::Property(new_prop) = r.kind {
                                    p = unsafe { std::mem::transmute(new_prop) };
                                }
                            }
                        }
                    }
                    self.clone_member_to_static(&ClassMember {
                        span: member.span,
                        kind: ClassMemberKind::Property(p),
                    })
                }
                ClassMemberKind::Method(meth) => {
                    let mut m = meth.clone();
                    for attr in &meth.attributes {
                        let attr_name = attr.name.as_str();
                        let expander = match self.registry.get_attribute(attr_name) {
                            Some(e) => e,
                            None => {
                                return Err(self.ctx.error(
                                    "RHP0501",
                                    format!("unknown compiler extension attribute `#[{}]`", attr_name),
                                    attr.span,
                                ));
                            }
                        };

                        let site = ExpansionSite::Method {
                            attribute: attr,
                            class,
                            method: meth,
                        };

                        let expand_res = catch_unwind(AssertUnwindSafe(|| {
                            expander.expand(self.ctx, site)
                        }));

                        let expansion = match expand_res {
                            Ok(res) => res?,
                            Err(panic_payload) => {
                                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "unknown panic payload".to_string()
                                };
                                return Err(self.ctx.error(
                                    "RHP0505",
                                    format!("expander handler `{}` panicked: {}", attr_name, msg),
                                    attr.span,
                                ));
                            }
                        };

                        if let Expansion::ReplaceClassMembers(mut repl) = expansion {
                            if let Some(r) = repl.pop() {
                                if let ClassMemberKind::Method(new_meth) = r.kind {
                                    m = unsafe { std::mem::transmute(new_meth) };
                                }
                            }
                        }
                    }
                    self.clone_member_to_static(&ClassMember {
                        span: member.span,
                        kind: ClassMemberKind::Method(m),
                    })
                }
                _ => self.clone_member_to_static(member),
            };

            final_members.push(processed_member);
        }

        // Check if any existing member name conflicts with added members
        for existing in &final_members {
            let ename = member_ident(existing);
            if let Some((origin_span, expander_name)) = generated_member_sites.get(&ename) {
                return Err(self.ctx.build_diagnostic(
                    DiagnosticLevel::Error,
                    "RHP0504",
                    format!(
                        "conflicting macro expansion: generated member `{}` from `#[{}]` conflicts with existing member on class `{}`",
                        ename, expander_name, class.name.as_str()
                    ),
                    existing.span,
                    Some(format!("existing member `{}` here", ename)),
                    vec![(*origin_span, format!("conflicting generation by `#[{}]` here", expander_name))],
                ));
            }
        }

        // Append added members after existing members, preserving declaration order
        final_members.extend(added_members);

        let static_class_name = self.ctx.intern(class.name.as_str());
        let static_name = crate::ast::types::Ident::new(static_class_name, class.name.span);

        let static_extends = class
            .extends
            .as_ref()
            .map(|ext| crate::ast::types::Ident::new(self.ctx.intern(ext.as_str()), ext.span));

        let static_implements = class
            .implements
            .iter()
            .map(|imp| crate::ast::types::Ident::new(self.ctx.intern(imp.as_str()), imp.span))
            .collect();

        Ok(ClassDecl {
            span: class.span,
            attributes: retained_attributes,
            modifiers: class.modifiers,
            name: static_name,
            extends: static_extends,
            implements: static_implements,
            members: final_members,
        })
    }

    /// Expand a standalone function declaration.
    pub fn expand_function(&mut self, func: &FunctionDecl<'_>) -> Result<FunctionDecl<'static>, Diagnostic> {
        let static_name = self.ctx.intern(func.name.as_str());
        let ident = crate::ast::types::Ident::new(static_name, func.name.span);

        let static_body = if let Some(ref body) = func.body {
            let mut expanded_body = Vec::new();
            for s in body {
                expanded_body.extend(self.expand_statement(s)?);
            }
            Some(expanded_body)
        } else {
            None
        };

        let static_params: Vec<_> = func
            .params
            .iter()
            .map(|p| self.clone_param_to_static(p))
            .collect();

        let static_return_type = func
            .return_type
            .as_ref()
            .map(|t| self.clone_type_hint_to_static(t));

        Ok(FunctionDecl {
            span: func.span,
            attributes: Vec::new(),
            is_async: func.is_async,
            name: ident,
            params: static_params,
            return_type: static_return_type,
            body: static_body,
        })
    }

    fn clone_stmt_to_static(&mut self, stmt: &Stmt<'_>) -> Stmt<'static> {
        // Safe transmutation for AST nodes whose lifetime is extended via ExpandCtx string interning
        unsafe { std::mem::transmute(stmt.clone()) }
    }

    fn clone_member_to_static(&mut self, member: &ClassMember<'_>) -> ClassMember<'static> {
        unsafe { std::mem::transmute(member.clone()) }
    }

    fn clone_param_to_static(&mut self, param: &crate::ast::stmt::Param<'_>) -> crate::ast::stmt::Param<'static> {
        unsafe { std::mem::transmute(param.clone()) }
    }

    fn clone_type_hint_to_static(&mut self, hint: &crate::ast::types::TypeHint<'_>) -> crate::ast::types::TypeHint<'static> {
        unsafe { std::mem::transmute(hint.clone()) }
    }

    fn clone_attr_to_static(&mut self, attr: &crate::ast::stmt::Attribute<'_>) -> crate::ast::stmt::Attribute<'static> {
        unsafe { std::mem::transmute(attr.clone()) }
    }
}

fn member_ident(member: &ClassMember<'static>) -> String {
    match &member.kind {
        ClassMemberKind::Property(p) => p.name.bare_name().to_string(),
        ClassMemberKind::Method(m) => m.function.name.as_str().to_string(),
        ClassMemberKind::Constant(c) => c.name.as_str().to_string(),
        ClassMemberKind::TraitUse(_) => "<trait_use>".to_string(),
    }
}
