pub mod lexer;
pub mod parser;

pub use lexer::{TemplateLexer, TemplateToken};
pub use parser::{TemplateDirective, TemplateElseBranch, TemplateNode, TemplateParser};

use crate::ast::Program;
use crate::diagnostic::Diagnostic;
use crate::expand::builtin::directives::lower_template_nodes;
use crate::expand::context::ExpandCtx;
use crate::expand::registry::ExpanderRegistry;
use crate::lexer::span::Span;

/// Parse and compile a template source string (.rhp.html, .blade.rhp) into a complete RHP AST Program.
pub fn compile_template(
    source: &str,
    file_id: usize,
    registry: &ExpanderRegistry,
    ctx: &mut ExpandCtx,
) -> Result<Program<'static>, Diagnostic> {
    let mut lexer = TemplateLexer::new(source, file_id);
    let tokens = lexer.tokenize().map_err(|e| {
        ctx.error(
            "RHP0102",
            format!("template lexical error: {}", e),
            Span::new(file_id, 0, 1, 1, 1),
        )
    })?;

    let mut parser = TemplateParser::new(tokens, ctx);
    let nodes = parser.parse()?;

    let statements = lower_template_nodes(&nodes, ctx, registry)?;
    let program_span = Span::new(file_id, 0, source.len(), 1, 1);

    Ok(Program::new(statements, program_span))
}
