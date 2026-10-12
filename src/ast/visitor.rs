use crate::ast::{
    expr::{Expr, ExprKind},
    stmt::{ClassMemberKind, Stmt, StmtKind},
    Program,
};

/// Immutable AST Visitor trait allowing traversal across programs, statements, and expressions.
pub trait AstVisitor<'s> {
    fn visit_program(&mut self, program: &Program<'s>) {
        walk_program(self, program);
    }

    fn visit_stmt(&mut self, stmt: &Stmt<'s>) {
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr<'s>) {
        walk_expr(self, expr);
    }
}

pub fn walk_program<'s, V: AstVisitor<'s> + ?Sized>(visitor: &mut V, program: &Program<'s>) {
    for stmt in &program.statements {
        visitor.visit_stmt(stmt);
    }
}

pub fn walk_stmt<'s, V: AstVisitor<'s> + ?Sized>(visitor: &mut V, stmt: &Stmt<'s>) {
    match &stmt.kind {
        StmtKind::Expr(expr) => {
            visitor.visit_expr(expr);
        }
        StmtKind::VarDecl { init, .. } => {
            if let Some(expr) = init {
                visitor.visit_expr(expr);
            }
        }
        StmtKind::Block(stmts) => {
            for s in stmts {
                visitor.visit_stmt(s);
            }
        }
        StmtKind::If {
            condition,
            then_branch,
            elseif_branches,
            else_branch,
        } => {
            visitor.visit_expr(condition);
            visitor.visit_stmt(then_branch);
            for branch in elseif_branches {
                visitor.visit_expr(&branch.condition);
                visitor.visit_stmt(&branch.body);
            }
            if let Some(else_b) = else_branch {
                visitor.visit_stmt(else_b);
            }
        }
        StmtKind::While { condition, body } => {
            visitor.visit_expr(condition);
            visitor.visit_stmt(body);
        }
        StmtKind::DoWhile { body, condition } => {
            visitor.visit_stmt(body);
            visitor.visit_expr(condition);
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            for e in init {
                visitor.visit_expr(e);
            }
            for e in condition {
                visitor.visit_expr(e);
            }
            for e in update {
                visitor.visit_expr(e);
            }
            visitor.visit_stmt(body);
        }
        StmtKind::Foreach {
            expr,
            value_var,
            body,
            ..
        } => {
            visitor.visit_expr(expr);
            visitor.visit_expr(value_var);
            visitor.visit_stmt(body);
        }
        StmtKind::Switch { condition, cases } => {
            visitor.visit_expr(condition);
            for case in cases {
                if let Some(v) = &case.value {
                    visitor.visit_expr(v);
                }
                for s in &case.body {
                    visitor.visit_stmt(s);
                }
            }
        }
        StmtKind::Break(opt) | StmtKind::Continue(opt) | StmtKind::Return(opt) => {
            if let Some(e) = opt {
                visitor.visit_expr(e);
            }
        }
        StmtKind::Throw(expr) => {
            visitor.visit_expr(expr);
        }
        StmtKind::TryCatch {
            try_block,
            catches,
            finally_block,
        } => {
            for s in try_block {
                visitor.visit_stmt(s);
            }
            for c in catches {
                for s in &c.body {
                    visitor.visit_stmt(s);
                }
            }
            if let Some(fin) = finally_block {
                for s in fin {
                    visitor.visit_stmt(s);
                }
            }
        }
        StmtKind::FunctionDecl(f) => {
            if let Some(body) = &f.body {
                for s in body {
                    visitor.visit_stmt(s);
                }
            }
        }
        StmtKind::ClassDecl(c) => {
            for member in &c.members {
                match &member.kind {
                    ClassMemberKind::Property(p) => {
                        if let Some(e) = &p.default {
                            visitor.visit_expr(e);
                        }
                    }
                    ClassMemberKind::Method(m) => {
                        if let Some(body) = &m.function.body {
                            for s in body {
                                visitor.visit_stmt(s);
                            }
                        }
                    }
                    ClassMemberKind::Constant(co) => {
                        visitor.visit_expr(&co.value);
                    }
                    ClassMemberKind::TraitUse(_) => {}
                }
            }
        }
        StmtKind::InterfaceDecl(_) => {}
        StmtKind::TraitDecl(t) => {
            for member in &t.members {
                if let ClassMemberKind::Method(m) = &member.kind {
                    if let Some(body) = &m.function.body {
                        for s in body {
                            visitor.visit_stmt(s);
                        }
                    }
                }
            }
        }
        StmtKind::EnumDecl(e) => {
            for case in &e.cases {
                if let Some(val) = &case.value {
                    visitor.visit_expr(val);
                }
            }
            for m in &e.methods {
                if let Some(body) = &m.function.body {
                    for s in body {
                        visitor.visit_stmt(s);
                    }
                }
            }
        }
        StmtKind::Namespace { body, .. } => {
            if let Some(stmts) = body {
                for s in stmts {
                    visitor.visit_stmt(s);
                }
            }
        }
        StmtKind::Use(_) | StmtKind::Global(_) => {}
        StmtKind::Echo(exprs) => {
            for e in exprs {
                visitor.visit_expr(e);
            }
        }
        StmtKind::Defer(s) => {
            visitor.visit_stmt(s);
        }
        StmtKind::Empty => {}
    }
}

pub fn walk_expr<'s, V: AstVisitor<'s> + ?Sized>(visitor: &mut V, expr: &Expr<'s>) {
    match &expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Ident(_) => {}
        ExprKind::Unary { expr, .. } => {
            visitor.visit_expr(expr);
        }
        ExprKind::Binary { left, right, .. } => {
            visitor.visit_expr(left);
            visitor.visit_expr(right);
        }
        ExprKind::Assign { target, value, .. } => {
            visitor.visit_expr(target);
            visitor.visit_expr(value);
        }
        ExprKind::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            visitor.visit_expr(condition);
            if let Some(t) = if_true {
                visitor.visit_expr(t);
            }
            visitor.visit_expr(if_false);
        }
        ExprKind::Call { callee, args } => {
            visitor.visit_expr(callee);
            for a in args {
                visitor.visit_expr(&a.value);
            }
        }
        ExprKind::MethodCall { object, args, .. } => {
            visitor.visit_expr(object);
            for a in args {
                visitor.visit_expr(&a.value);
            }
        }
        ExprKind::StaticCall { target, args, .. } => {
            visitor.visit_expr(target);
            for a in args {
                visitor.visit_expr(&a.value);
            }
        }
        ExprKind::PropertyFetch { object, .. } => {
            visitor.visit_expr(object);
        }
        ExprKind::StaticPropertyFetch { .. } | ExprKind::ClassConstFetch { .. } => {}
        ExprKind::Array(elements) => {
            for el in elements {
                if let Some(k) = &el.key {
                    visitor.visit_expr(k);
                }
                visitor.visit_expr(&el.value);
            }
        }
        ExprKind::ArrayIndex { array, index } => {
            visitor.visit_expr(array);
            if let Some(idx) = index {
                visitor.visit_expr(idx);
            }
        }
        ExprKind::List(elements) => {
            for opt_el in elements.iter().flatten() {
                if let Some(k) = &opt_el.key {
                    visitor.visit_expr(k);
                }
                visitor.visit_expr(&opt_el.value);
            }
        }
        ExprKind::ArrowFunction { body, .. } => {
            visitor.visit_expr(body);
        }
        ExprKind::Closure { body, .. } => {
            for s in body {
                visitor.visit_stmt(s);
            }
        }
        ExprKind::Match { condition, arms } => {
            visitor.visit_expr(condition);
            for arm in arms {
                for p in &arm.patterns {
                    visitor.visit_expr(p);
                }
                visitor.visit_expr(&arm.body);
            }
        }
        ExprKind::New { class, args } => {
            visitor.visit_expr(class);
            for a in args {
                visitor.visit_expr(&a.value);
            }
        }
        ExprKind::Clone(e) | ExprKind::Await(e) | ExprKind::Grouping(e) => {
            visitor.visit_expr(e);
        }
        ExprKind::Instanceof { expr, class } => {
            visitor.visit_expr(expr);
            visitor.visit_expr(class);
        }
        ExprKind::Cast { expr, .. } => {
            visitor.visit_expr(expr);
        }
        ExprKind::Yield { key, value, .. } => {
            if let Some(k) = key {
                visitor.visit_expr(k);
            }
            if let Some(v) = value {
                visitor.visit_expr(v);
            }
        }
    }
}
