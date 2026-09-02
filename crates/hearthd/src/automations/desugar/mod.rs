//! Desugaring pass for the HearthD Automations language.
//!
//! Transforms the high-level AST into a lowered representation where
//! list comprehensions are expanded into explicit loop constructs.

use std::rc::Rc;

use super::repr::ast;
use super::repr::ast::Arg;
use super::repr::ast::Expr;
use super::repr::ast::Stmt;
use super::repr::ast::StructField;
use super::repr::lowered::LoweredArg;
use super::repr::lowered::LoweredAutomation;
use super::repr::lowered::LoweredExpr;
use super::repr::lowered::LoweredProgram;
use super::repr::lowered::LoweredStmt;
use super::repr::lowered::LoweredStructField;
use super::repr::lowered::Origin;
use super::repr::lowered::Spanned;

#[cfg(test)]
mod tests;

/// State for generating unique variable names during desugaring.
pub struct Desugarer {
    counter: usize,
}

impl Default for Desugarer {
    fn default() -> Self {
        Self::new()
    }
}

impl Desugarer {
    pub fn new() -> Self {
        Self { counter: 0 }
    }

    /// Generate a unique variable name for desugared constructs.
    fn fresh_name(&mut self, prefix: &str) -> String {
        let name = format!("__{}{}", prefix, self.counter);
        self.counter += 1;
        name
    }

    /// Desugar an automation, lowering its filter and body expressions.
    pub fn desugar_automation(&mut self, auto: ast::Automation) -> LoweredAutomation {
        LoweredAutomation {
            kind: auto.kind,
            pattern: auto.pattern,
            filter: auto.filter.map(|f| self.desugar_expr(f)),
            body: auto
                .body
                .into_iter()
                .map(|s| self.desugar_stmt(s))
                .collect(),
        }
    }

    /// Desugar a complete program.
    pub fn desugar_program(&mut self, program: ast::Spanned<ast::Program>) -> LoweredProgram {
        match program.node {
            ast::Program::Automation(auto) => {
                LoweredProgram::Automation(self.desugar_automation(auto))
            }
            ast::Program::Template(tmpl) => LoweredProgram::Template {
                params: tmpl.params,
                automations: tmpl
                    .automations
                    .into_iter()
                    .map(|a| self.desugar_automation(a.node))
                    .collect(),
            },
        }
    }

    /// Desugar an expression from AST to LoweredAST.
    pub fn desugar_expr(&mut self, expr: ast::Spanned<Expr>) -> Spanned<LoweredExpr> {
        let span = expr.span;
        match expr.node {
            // Leaf cases: reconstruct origin from components
            Expr::Int(n) => Spanned::new(
                LoweredExpr::Int(n),
                Origin::Direct(ast::Spanned::new(Expr::Int(n), span)),
            ),
            Expr::Float(f) => Spanned::new(
                LoweredExpr::Float(f.clone()),
                Origin::Direct(ast::Spanned::new(Expr::Float(f), span)),
            ),
            Expr::String(s) => Spanned::new(
                LoweredExpr::String(s.clone()),
                Origin::Direct(ast::Spanned::new(Expr::String(s), span)),
            ),
            Expr::Bool(b) => Spanned::new(
                LoweredExpr::Bool(b),
                Origin::Direct(ast::Spanned::new(Expr::Bool(b), span)),
            ),
            Expr::UnitLiteral { value, unit } => Spanned::new(
                LoweredExpr::UnitLiteral {
                    value: value.clone(),
                    unit,
                },
                Origin::Direct(ast::Spanned::new(Expr::UnitLiteral { value, unit }, span)),
            ),
            Expr::Ident(s) => Spanned::new(
                LoweredExpr::Ident(s.clone()),
                Origin::Direct(ast::Spanned::new(Expr::Ident(s), span)),
            ),
            Expr::Path(segments) => Spanned::new(
                LoweredExpr::Path(segments.clone()),
                Origin::Direct(ast::Spanned::new(Expr::Path(segments), span)),
            ),

            // Recursive cases: clone children for origin, move originals to recursive calls
            Expr::BinOp { op, left, right } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::BinOp {
                        op,
                        left: left.clone(),
                        right: right.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::BinOp {
                        op,
                        left: Box::new(self.desugar_expr(*left)),
                        right: Box::new(self.desugar_expr(*right)),
                    },
                    origin,
                )
            }

            Expr::UnaryOp { op, expr: inner } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::UnaryOp {
                        op,
                        expr: inner.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::UnaryOp {
                        op,
                        expr: Box::new(self.desugar_expr(*inner)),
                    },
                    origin,
                )
            }

            Expr::Field { expr: inner, field } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::Field {
                        expr: inner.clone(),
                        field: field.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::Field {
                        expr: Box::new(self.desugar_expr(*inner)),
                        field,
                    },
                    origin,
                )
            }

            Expr::OptionalField { expr: inner, field } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::OptionalField {
                        expr: inner.clone(),
                        field: field.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::OptionalField {
                        expr: Box::new(self.desugar_expr(*inner)),
                        field,
                    },
                    origin,
                )
            }

            Expr::Call { func, args } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::Call {
                        func: func.clone(),
                        args: args.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::Call {
                        func: Box::new(self.desugar_expr(*func)),
                        args: args.into_iter().map(|a| self.desugar_arg(a)).collect(),
                    },
                    origin,
                )
            }

            Expr::If {
                cond,
                then_block,
                else_block,
            } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::If {
                        cond: cond.clone(),
                        then_block: then_block.clone(),
                        else_block: else_block.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::If {
                        cond: Box::new(self.desugar_expr(*cond)),
                        then_block: then_block
                            .into_iter()
                            .map(|s| self.desugar_stmt(s))
                            .collect(),
                        else_block: else_block
                            .map(|stmts| stmts.into_iter().map(|s| self.desugar_stmt(s)).collect()),
                    },
                    origin,
                )
            }

            Expr::List(items) => {
                let origin = Origin::Direct(ast::Spanned::new(Expr::List(items.clone()), span));
                Spanned::new(
                    LoweredExpr::List(items.into_iter().map(|e| self.desugar_expr(e)).collect()),
                    origin,
                )
            }

            Expr::StructLit { name, fields } => {
                let origin = Origin::Direct(ast::Spanned::new(
                    Expr::StructLit {
                        name: name.clone(),
                        fields: fields.clone(),
                    },
                    span,
                ));
                Spanned::new(
                    LoweredExpr::StructLit {
                        name,
                        fields: fields
                            .into_iter()
                            .map(|f| self.desugar_struct_field(f))
                            .collect(),
                    },
                    origin,
                )
            }

            // The main desugaring: ListComp - uses Rc for sharing
            Expr::ListComp {
                expr: body_expr,
                var,
                iter,
                filter,
            } => {
                let rc = Rc::new(ast::Spanned::new(
                    Expr::ListComp {
                        expr: body_expr.clone(),
                        var: var.clone(),
                        iter: iter.clone(),
                        filter: filter.clone(),
                    },
                    span,
                ));
                self.desugar_list_comp(rc, *body_expr, var, *iter, filter.map(|f| *f))
            }
        }
    }

    /// Desugar a list comprehension into a block expression.
    ///
    /// `[expr for var in iter]` becomes:
    /// ```text
    /// {
    ///     let mut __result0 = MutableList;
    ///     for var in iter {
    ///         push(__result0, expr);
    ///     }
    ///     __result0
    /// }
    /// ```
    ///
    /// `[expr for var in iter if filter]` becomes:
    /// ```text
    /// {
    ///     let mut __result0 = MutableList;
    ///     for var in iter {
    ///         if filter {
    ///             push(__result0, expr);
    ///         }
    ///     }
    ///     __result0
    /// }
    /// ```
    fn desugar_list_comp(
        &mut self,
        list_comp_rc: Rc<ast::Spanned<Expr>>,
        body_expr: ast::Spanned<Expr>,
        var: String,
        iter: ast::Spanned<Expr>,
        filter: Option<ast::Spanned<Expr>>,
    ) -> Spanned<LoweredExpr> {
        let origin = Origin::ListComp(list_comp_rc);

        // Generate unique result variable name
        let result_var = self.fresh_name("result");

        // Desugar the iterator expression
        let lowered_iter = self.desugar_expr(iter);

        // Desugar the body expression
        let lowered_body_expr = self.desugar_expr(body_expr);

        // Create the push statement
        let push_stmt = Spanned::new(
            LoweredStmt::Push {
                list: result_var.clone(),
                value: lowered_body_expr,
            },
            origin.clone(),
        );

        // Build the for loop body
        let for_body = if let Some(filter_expr) = filter {
            // With filter: if cond { push(...) }
            let lowered_filter = self.desugar_expr(filter_expr);
            vec![Spanned::new(
                LoweredStmt::Expr(Spanned::new(
                    LoweredExpr::If {
                        cond: Box::new(lowered_filter),
                        then_block: vec![push_stmt],
                        else_block: None,
                    },
                    origin.clone(),
                )),
                origin.clone(),
            )]
        } else {
            // Without filter: just push
            vec![push_stmt]
        };

        // Create the for statement
        let for_stmt = Spanned::new(
            LoweredStmt::For {
                var,
                iter: lowered_iter,
                body: for_body,
            },
            origin.clone(),
        );

        // Create the let mut statement for result
        let let_mut_stmt = Spanned::new(
            LoweredStmt::LetMut {
                name: result_var.clone(),
                value: Spanned::new(LoweredExpr::MutableList, origin.clone()),
            },
            origin.clone(),
        );

        // Return the block expression
        Spanned::new(
            LoweredExpr::Block {
                stmts: vec![let_mut_stmt, for_stmt],
                result: Box::new(Spanned::new(LoweredExpr::Ident(result_var), origin.clone())),
            },
            origin,
        )
    }

    /// Desugar a statement.
    pub fn desugar_stmt(&mut self, stmt: ast::Spanned<Stmt>) -> Spanned<LoweredStmt> {
        // For statements, we need to create an origin. We'll use a synthetic
        // expression origin by wrapping the statement's expression if available.
        match stmt.node {
            Stmt::Let { name, value } => {
                let lowered_value = self.desugar_expr(value);
                let origin = lowered_value.origin.clone();
                Spanned::new(
                    LoweredStmt::Let {
                        name,
                        value: lowered_value,
                    },
                    origin,
                )
            }
            Stmt::Expr(expr) => {
                let lowered_expr = self.desugar_expr(expr);
                let origin = lowered_expr.origin.clone();
                Spanned::new(LoweredStmt::Expr(lowered_expr), origin)
            }
            Stmt::Return(expr) => {
                let lowered_expr = self.desugar_expr(expr);
                let origin = lowered_expr.origin.clone();
                Spanned::new(LoweredStmt::Return(lowered_expr), origin)
            }
        }
    }

    fn desugar_arg(&mut self, arg: ast::Spanned<Arg>) -> Spanned<LoweredArg> {
        match arg.node {
            Arg::Positional(expr) => {
                let lowered_expr = self.desugar_expr(expr);
                let origin = lowered_expr.origin.clone();
                Spanned::new(LoweredArg::Positional(lowered_expr), origin)
            }
            Arg::Named { name, value } => {
                let lowered_value = self.desugar_expr(value);
                let origin = lowered_value.origin.clone();
                Spanned::new(
                    LoweredArg::Named {
                        name,
                        value: lowered_value,
                    },
                    origin,
                )
            }
        }
    }

    fn desugar_struct_field(
        &mut self,
        field: ast::Spanned<StructField>,
    ) -> Spanned<LoweredStructField> {
        let span = field.span;
        match field.node {
            StructField::Field { name, value } => {
                let lowered_value = self.desugar_expr(value);
                let origin = lowered_value.origin.clone();
                Spanned::new(
                    LoweredStructField::Field {
                        name,
                        value: lowered_value,
                    },
                    origin,
                )
            }
            StructField::Inherit(name) => {
                // For inherit/spread, we need an origin but don't have an expression.
                // Create a dummy expression to satisfy the type requirement.
                // This is a limitation - we might want to extend Origin to handle
                // non-expression AST nodes in the future.
                let dummy_expr = ast::Spanned::new(Expr::Ident(name.clone()), span);
                let origin = Origin::Direct(dummy_expr);
                Spanned::new(LoweredStructField::Inherit(name), origin)
            }
            StructField::Spread(name) => {
                let dummy_expr = ast::Spanned::new(Expr::Ident(name.clone()), span);
                let origin = Origin::Direct(dummy_expr);
                Spanned::new(LoweredStructField::Spread(name), origin)
            }
        }
    }
}

/// Convenience function to desugar an expression.
pub fn desugar(expr: ast::Spanned<Expr>) -> Spanned<LoweredExpr> {
    Desugarer::new().desugar_expr(expr)
}

/// Convenience function to desugar a complete program.
pub fn desugar_program(program: ast::Spanned<ast::Program>) -> LoweredProgram {
    Desugarer::new().desugar_program(program)
}
