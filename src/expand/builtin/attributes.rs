use crate::ast::expr::{Expr, ExprKind, Literal};
use crate::ast::types::Visibility;
use crate::diagnostic::Diagnostic;
use crate::expand::builder::AstBuilder;
use crate::expand::context::ExpandCtx;
use crate::expand::expansion::Expansion;
use crate::expand::site::ExpansionSite;
use crate::expand::traits::Expander;

/// Helper to extract string literal arguments from an attribute's arguments.
fn extract_string_args(site: &ExpansionSite, ctx: &mut ExpandCtx) -> Result<Vec<String>, Diagnostic> {
    let mut names = Vec::new();
    for arg in site.args() {
        match &arg.value.kind {
            ExprKind::Literal(Literal::String { value, .. }) => {
                names.push(value.clone());
            }
            _ => {
                return Err(ctx.error(
                    "RHP0502",
                    format!(
                        "attribute `#[{}]` expects string literal arguments, got expression",
                        site.name()
                    ),
                    arg.span,
                ));
            }
        }
    }
    Ok(names)
}

/// Declarative metadata attribute expander (e.g. `#[Route]`, `#[Singleton]`).
///
/// Returns `Expansion::Empty` while retaining the attribute for compile-time reflection / metadata extraction.
pub struct DeclarativeExpander {
    name: &'static str,
}

impl DeclarativeExpander {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl Expander for DeclarativeExpander {
    fn name(&self) -> &str {
        self.name
    }

    fn expand(&self, _ctx: &mut ExpandCtx, _site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        Ok(Expansion::Empty)
    }
}

/// Built-in `#[fillable('col1', 'col2', ...)]` attribute expander.
///
/// Adds `protected array $fillable = ['col1', 'col2', ...];` to the target class.
pub struct FillableExpander;

impl Expander for FillableExpander {
    fn name(&self) -> &str {
        "fillable"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        if !site.is_class() {
            return Err(ctx.error(
                "RHP0506",
                "attribute `#[fillable]` can only be applied to a class declaration",
                site.span(),
            ));
        }

        let fields = extract_string_args(&site, ctx)?;
        let mut builder = AstBuilder::new(ctx, site.span(), "fillable");
        let item_exprs: Vec<Expr<'static>> = fields
            .into_iter()
            .map(|f| builder.string_expr(&f))
            .collect();

        let default_val = builder.array_expr(item_exprs);
        let prop = builder.property("fillable", Visibility::Protected, Some("array"), Some(default_val));

        Ok(Expansion::AddClassMembers(vec![prop]))
    }
}

/// Built-in `#[guarded('col1', 'col2', ...)]` attribute expander.
///
/// Adds `protected array $guarded = ['col1', 'col2', ...];` to the target class.
pub struct GuardedExpander;

impl Expander for GuardedExpander {
    fn name(&self) -> &str {
        "guarded"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        if !site.is_class() {
            return Err(ctx.error(
                "RHP0506",
                "attribute `#[guarded]` can only be applied to a class declaration",
                site.span(),
            ));
        }

        let fields = extract_string_args(&site, ctx)?;
        let mut builder = AstBuilder::new(ctx, site.span(), "guarded");
        let item_exprs: Vec<Expr<'static>> = fields
            .into_iter()
            .map(|f| builder.string_expr(&f))
            .collect();

        let default_val = builder.array_expr(item_exprs);
        let prop = builder.property("guarded", Visibility::Protected, Some("array"), Some(default_val));

        Ok(Expansion::AddClassMembers(vec![prop]))
    }
}

/// Built-in `#[table('table_name')]` attribute expander.
///
/// Adds `protected string $table = 'table_name';` to the target class.
pub struct TableExpander;

impl Expander for TableExpander {
    fn name(&self) -> &str {
        "table"
    }

    fn expand(&self, ctx: &mut ExpandCtx, site: ExpansionSite) -> Result<Expansion, Diagnostic> {
        if !site.is_class() {
            return Err(ctx.error(
                "RHP0506",
                "attribute `#[table]` can only be applied to a class declaration",
                site.span(),
            ));
        }

        let args = site.args();
        if args.len() != 1 {
            return Err(ctx.error(
                "RHP0502",
                format!("attribute `#[table]` requires exactly 1 argument, got {}", args.len()),
                site.span(),
            ));
        }

        let table_name = match &args[0].value.kind {
            ExprKind::Literal(Literal::String { value, .. }) => value.clone(),
            _ => {
                return Err(ctx.error(
                    "RHP0502",
                    "attribute `#[table]` requires a string literal table name",
                    args[0].span,
                ));
            }
        };

        let mut builder = AstBuilder::new(ctx, site.span(), "table");
        let default_val = builder.string_expr(&table_name);
        let prop = builder.property("table", Visibility::Protected, Some("string"), Some(default_val));

        Ok(Expansion::AddClassMembers(vec![prop]))
    }
}
