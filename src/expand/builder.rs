use crate::ast::{
    expr::{Argument, ArrayElement, Expr, ExprKind, Literal},
    stmt::{
        ClassMember, ClassMemberKind, ElseIfBranch, PropertyDecl, Stmt,
        StmtKind,
    },
    types::{Ident, TypeHint, TypeHintKind, Variable, Visibility},
};
use crate::expand::context::ExpandCtx;
use crate::lexer::span::Span;

/// Strongly-typed AST builder that automatically stamps all constructed nodes
/// with the originating expansion site span and tracks span provenance.
pub struct AstBuilder<'a> {
    pub ctx: &'a mut ExpandCtx,
    pub origin_span: Span,
    pub expander_name: &'static str,
}

impl<'a> AstBuilder<'a> {
    /// Create a new builder stamped with the given expansion site span and handler name.
    pub fn new(ctx: &'a mut ExpandCtx, origin_span: Span, expander_name: &'static str) -> Self {
        Self {
            ctx,
            origin_span,
            expander_name,
        }
    }

    /// Intern a string to `'static`.
    pub fn intern(&mut self, s: &str) -> &'static str {
        self.ctx.intern(s)
    }

    /// Construct a stamped identifier.
    pub fn ident(&mut self, name: &str) -> Ident<'static> {
        let interned = self.ctx.intern(name);
        self.ctx
            .record_span_chain(self.origin_span, self.origin_span, self.expander_name);
        Ident::new(interned, self.origin_span)
    }

    /// Construct a stamped variable (e.g. `"$name"`).
    pub fn variable(&mut self, name: &str) -> Variable<'static> {
        let full_name = if name.starts_with('$') {
            self.ctx.intern(name)
        } else {
            self.ctx.intern(&format!("${}", name))
        };
        self.ctx
            .record_span_chain(self.origin_span, self.origin_span, self.expander_name);
        Variable::new(full_name, self.origin_span)
    }

    /// Construct a stamped type hint (e.g. `"array"`, `"string"`).
    pub fn type_hint(&mut self, name: &str) -> TypeHint<'static> {
        let ident = self.ident(name);
        TypeHint::new(TypeHintKind::Named(ident), self.origin_span)
    }

    /// Construct a string literal expression.
    pub fn string_expr(&mut self, val: &str) -> Expr<'static> {
        let raw = self.ctx.intern(&format!("\"{}\"", val));
        Expr::new(
            ExprKind::Literal(Literal::String {
                raw,
                value: val.to_string(),
            }),
            self.origin_span,
        )
    }

    /// Construct an integer literal expression.
    pub fn int_expr(&mut self, val: i64) -> Expr<'static> {
        let raw = self.ctx.intern(&val.to_string());
        Expr::new(
            ExprKind::Literal(Literal::Int { raw, value: val }),
            self.origin_span,
        )
    }

    /// Construct a boolean literal expression.
    pub fn bool_expr(&mut self, val: bool) -> Expr<'static> {
        Expr::new(ExprKind::Literal(Literal::Bool(val)), self.origin_span)
    }

    /// Construct a variable reference expression.
    pub fn var_expr(&mut self, var_name: &str) -> Expr<'static> {
        let var = self.variable(var_name);
        Expr::new(ExprKind::Variable(var), self.origin_span)
    }

    /// Construct an array literal expression `[item1, item2, ...]`.
    pub fn array_expr(&mut self, items: Vec<Expr<'static>>) -> Expr<'static> {
        let elements = items
            .into_iter()
            .map(|val| ArrayElement::value(val, self.origin_span))
            .collect();
        Expr::new(ExprKind::Array(elements), self.origin_span)
    }

    /// Construct a call argument.
    pub fn arg(&mut self, expr: Expr<'static>) -> Argument<'static> {
        Argument::new(expr, self.origin_span)
    }

    /// Construct a function call expression.
    pub fn call_expr(&mut self, callee: &str, args: Vec<Expr<'static>>) -> Expr<'static> {
        let arg_nodes = args.into_iter().map(|a| Argument::new(a, self.origin_span)).collect();
        Expr::user_function_call(
            self.ctx.intern(callee),
            arg_nodes,
            self.origin_span,
            self.origin_span,
        )
    }

    /// Construct a static method call expression `Class::method(args)`.
    pub fn static_call_expr(
        &mut self,
        class_name: &str,
        method_name: &str,
        args: Vec<Expr<'static>>,
    ) -> Expr<'static> {
        let arg_nodes = args.into_iter().map(|a| Argument::new(a, self.origin_span)).collect();
        Expr::static_call(
            self.ctx.intern(class_name),
            self.ctx.intern(method_name),
            arg_nodes,
            self.origin_span,
            self.origin_span,
        )
    }

    /// Construct a class property member: `visibility type_hint $name = default;`.
    pub fn property(
        &mut self,
        name: &str,
        visibility: Visibility,
        type_hint: Option<&str>,
        default_expr: Option<Expr<'static>>,
    ) -> ClassMember<'static> {
        let var = self.variable(name);
        let hint = type_hint.map(|h| self.type_hint(h));
        let prop = PropertyDecl {
            attributes: Vec::new(),
            visibility,
            is_static: false,
            is_readonly: false,
            type_hint: hint,
            name: var,
            default: default_expr,
        };
        self.ctx
            .record_span_chain(self.origin_span, self.origin_span, self.expander_name);
        ClassMember {
            span: self.origin_span,
            kind: ClassMemberKind::Property(prop),
        }
    }

    /// Construct an echo statement: `echo expr1, expr2;`.
    pub fn echo_stmt(&mut self, exprs: Vec<Expr<'static>>) -> Stmt<'static> {
        Stmt::new(StmtKind::Echo(exprs), self.origin_span)
    }

    /// Construct an expression statement.
    pub fn expr_stmt(&mut self, expr: Expr<'static>) -> Stmt<'static> {
        Stmt::new(StmtKind::Expr(expr), self.origin_span)
    }

    /// Construct an `if` statement.
    pub fn if_stmt(
        &mut self,
        condition: Expr<'static>,
        then_stmts: Vec<Stmt<'static>>,
        elseif_branches: Vec<ElseIfBranch<'static>>,
        else_stmts: Option<Vec<Stmt<'static>>>,
    ) -> Stmt<'static> {
        let then_block = Stmt::new(StmtKind::Block(then_stmts), self.origin_span);
        let else_block = else_stmts.map(|s| Box::new(Stmt::new(StmtKind::Block(s), self.origin_span)));
        Stmt::new(
            StmtKind::If {
                condition,
                then_branch: Box::new(then_block),
                elseif_branches,
                else_branch: else_block,
            },
            self.origin_span,
        )
    }

    /// Construct an `elseif` branch.
    pub fn elseif_branch(
        &mut self,
        condition: Expr<'static>,
        body: Vec<Stmt<'static>>,
    ) -> ElseIfBranch<'static> {
        let body_block = Stmt::new(StmtKind::Block(body), self.origin_span);
        ElseIfBranch {
            span: self.origin_span,
            condition,
            body: body_block,
        }
    }

    /// Construct a `for` loop statement.
    pub fn for_stmt(
        &mut self,
        init: Vec<Expr<'static>>,
        condition: Vec<Expr<'static>>,
        update: Vec<Expr<'static>>,
        body: Vec<Stmt<'static>>,
    ) -> Stmt<'static> {
        let body_block = Stmt::new(StmtKind::Block(body), self.origin_span);
        Stmt::new(
            StmtKind::For {
                init,
                condition,
                update,
                body: Box::new(body_block),
            },
            self.origin_span,
        )
    }

    /// Construct a `foreach` loop statement.
    pub fn foreach_stmt(
        &mut self,
        expr: Expr<'static>,
        key_var: Option<Variable<'static>>,
        value_expr: Expr<'static>,
        body: Vec<Stmt<'static>>,
    ) -> Stmt<'static> {
        let body_block = Stmt::new(StmtKind::Block(body), self.origin_span);
        Stmt::new(
            StmtKind::Foreach {
                expr,
                key_var,
                value_var: Box::new(value_expr),
                body: Box::new(body_block),
            },
            self.origin_span,
        )
    }
}
