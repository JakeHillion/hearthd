//! Lowering pass: typed AST → HIR basic blocks.
//!
//! Transforms the tree-structured typed AST into a directed graph of basic
//! blocks. Variable names are replaced with numbered temporaries. Entity
//! references remain symbolic for later linking.

use std::collections::HashMap;

use facet::Facet as _;

use crate::automations::repr::ast;
use crate::automations::repr::hir::*;
use crate::automations::repr::typed::*;
use crate::engine::state;

#[cfg(test)]
mod tests;

// ============================================================================
// Facet reflection helpers (duplicated from check for module decoupling)
// ============================================================================

fn shape_to_ty(shape: &facet::Shape) -> Ty {
    if let facet::Type::User(facet::UserType::Struct(_)) = &shape.ty {
        return Ty::Named(shape.type_identifier.to_string());
    }
    match &shape.def {
        facet::Def::Map(map_def) => Ty::Map {
            key: Box::new(shape_to_ty(map_def.k())),
            value: Box::new(shape_to_ty(map_def.v())),
        },
        facet::Def::List(list_def) => Ty::List(Box::new(shape_to_ty(list_def.t))),
        facet::Def::Option(opt_def) => Ty::Option(Box::new(shape_to_ty(opt_def.t()))),
        _ => match shape.type_identifier {
            "i64" | "i32" | "i16" | "i8" | "u64" | "u32" | "u16" | "u8" | "usize" | "isize" => {
                Ty::Int
            }
            "f64" | "f32" => Ty::Float,
            "bool" => Ty::Bool,
            name if name.contains("String") => Ty::String,
            name => Ty::Named(name.to_string()),
        },
    }
}

fn struct_fields_for_type(name: &str) -> HashMap<String, Ty> {
    let shape = match name {
        "State" => state::State::SHAPE,
        "LightState" => state::LightState::SHAPE,
        "BinarySensorState" => state::BinarySensorState::SHAPE,
        _ => return HashMap::new(),
    };
    if let facet::Type::User(facet::UserType::Struct(st)) = &shape.ty {
        st.fields
            .iter()
            .map(|f| (f.name.to_string(), shape_to_ty(f.shape.get())))
            .collect()
    } else {
        HashMap::new()
    }
}

fn lower_binop(op: ast::BinOp) -> HirBinOp {
    match op {
        ast::BinOp::Add => HirBinOp::Add,
        ast::BinOp::Sub => HirBinOp::Sub,
        ast::BinOp::Mul => HirBinOp::Mul,
        ast::BinOp::Div => HirBinOp::Div,
        ast::BinOp::Mod => HirBinOp::Mod,
        ast::BinOp::Eq => HirBinOp::Eq,
        ast::BinOp::Ne => HirBinOp::Ne,
        ast::BinOp::Lt => HirBinOp::Lt,
        ast::BinOp::Le => HirBinOp::Le,
        ast::BinOp::Gt => HirBinOp::Gt,
        ast::BinOp::Ge => HirBinOp::Ge,
        ast::BinOp::In => HirBinOp::In,
        ast::BinOp::And | ast::BinOp::Or => unreachable!("short-circuit ops handled separately"),
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Lower a type-checked program to HIR.
pub fn lower_program(result: &CheckResult) -> HirProgram {
    match &result.program {
        TypedProgram::Automation(auto) => HirProgram::Automation(lower_automation(auto)),
        TypedProgram::Template {
            params,
            automations,
        } => HirProgram::Template {
            params: params.clone(),
            automations: automations.iter().map(lower_automation).collect(),
        },
    }
}

fn lower_automation(auto: &TypedAutomation) -> HirAutomation {
    let mut lowerer = Lowerer::new();

    // Lower pattern to params and initial bindings.
    let params = lowerer.lower_pattern(&auto.pattern);

    // Lower filter (if present): branch to body or exit.
    if let Some(filter) = &auto.filter {
        let body_entry = lowerer.fresh_block();
        let exit_block = lowerer.fresh_block();

        let cond = lowerer.lower_expr(filter);
        lowerer.set_terminator(Terminator::Branch {
            cond,
            then_block: body_entry,
            else_block: exit_block,
        });

        // Exit block: return default value for automation kind.
        lowerer.switch_to(exit_block);
        match auto.kind {
            ast::AutomationKind::Observer => {
                let empty =
                    lowerer.emit(Op::EmptyList, Ty::List(Box::new(Ty::Named("Event".into()))));
                lowerer.set_terminator(Terminator::Return(empty));
            }
            ast::AutomationKind::Mutator => {
                let event_tmp = lowerer.lookup("event");
                lowerer.set_terminator(Terminator::Return(event_tmp));
            }
        }

        lowerer.switch_to(body_entry);
    }

    // Lower body: the last expression becomes the return value.
    let result = lowerer.lower_stmts_result(&auto.body);
    lowerer.set_terminator(Terminator::Return(result));

    HirAutomation {
        kind: auto.kind,
        params,
        blocks: lowerer.blocks,
    }
}

// ============================================================================
// Lowerer
// ============================================================================

struct Lowerer {
    blocks: Vec<BasicBlock>,
    current_block: BlockId,
    tmp_counter: usize,
    scopes: Vec<HashMap<String, Tmp>>,
}

impl Lowerer {
    fn new() -> Self {
        let entry = BasicBlock {
            id: BlockId(0),
            instructions: Vec::new(),
            terminator: Terminator::Jump(BlockId(0)), // placeholder
        };
        Self {
            blocks: vec![entry],
            current_block: BlockId(0),
            tmp_counter: 0,
            scopes: vec![HashMap::new()],
        }
    }

    fn fresh_tmp(&mut self) -> Tmp {
        let t = Tmp(self.tmp_counter);
        self.tmp_counter += 1;
        t
    }

    fn fresh_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len());
        self.blocks.push(BasicBlock {
            id,
            instructions: Vec::new(),
            terminator: Terminator::Jump(BlockId(0)), // placeholder
        });
        id
    }

    fn emit(&mut self, op: Op, ty: Ty) -> Tmp {
        let dst = self.fresh_tmp();
        self.blocks[self.current_block.0]
            .instructions
            .push(Instruction { dst, op, ty });
        dst
    }

    /// Emit an instruction with a pre-allocated destination (non-SSA merge).
    fn emit_into(&mut self, dst: Tmp, op: Op, ty: Ty) {
        self.blocks[self.current_block.0]
            .instructions
            .push(Instruction { dst, op, ty });
    }

    fn set_terminator(&mut self, term: Terminator) {
        self.blocks[self.current_block.0].terminator = term;
    }

    fn switch_to(&mut self, block: BlockId) {
        self.current_block = block;
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: &str, tmp: Tmp) {
        self.scopes
            .last_mut()
            .expect("scope stack empty")
            .insert(name.to_string(), tmp);
    }

    fn lookup(&self, name: &str) -> Tmp {
        for scope in self.scopes.iter().rev() {
            if let Some(&tmp) = scope.get(name) {
                return tmp;
            }
        }
        panic!(
            "undefined variable '{}' in lowerer (type checker should have caught this)",
            name
        );
    }

    // ========================================================================
    // Pattern lowering
    // ========================================================================

    fn lower_pattern(&mut self, pattern: &ast::Spanned<ast::Pattern>) -> Vec<Param> {
        let input_fields: HashMap<String, Ty> = [
            ("event".into(), Ty::Named("Event".into())),
            ("state".into(), Ty::Named("State".into())),
        ]
        .into();

        let mut params = Vec::new();
        self.lower_pattern_inner(pattern, &input_fields, None, &mut params);
        params
    }

    fn lower_pattern_inner(
        &mut self,
        pattern: &ast::Spanned<ast::Pattern>,
        available_fields: &HashMap<String, Ty>,
        parent: Option<Tmp>,
        params: &mut Vec<Param>,
    ) {
        match &pattern.node {
            ast::Pattern::Ident(name) => {
                let tmp = self.fresh_tmp();
                let ty = Ty::Named("Input".into());
                params.push(Param {
                    name: name.clone(),
                    tmp,
                    ty,
                });
                self.bind(name, tmp);
            }
            ast::Pattern::Struct { fields, .. } => {
                for field in fields {
                    let field_name = &field.node.name;
                    let field_ty = available_fields
                        .get(field_name.as_str())
                        .cloned()
                        .unwrap_or(Ty::Error);

                    let tmp = if let Some(parent_tmp) = parent {
                        // Nested: emit Field instruction to extract from parent.
                        self.emit(
                            Op::Field {
                                base: parent_tmp,
                                field: field_name.clone(),
                            },
                            field_ty.clone(),
                        )
                    } else {
                        // Top-level: create as input param.
                        let t = self.fresh_tmp();
                        params.push(Param {
                            name: field_name.clone(),
                            tmp: t,
                            ty: field_ty.clone(),
                        });
                        t
                    };

                    if let Some(sub_pattern) = &field.node.pattern {
                        // Nested destructuring: don't bind parent, recurse.
                        let sub_fields = match &field_ty {
                            Ty::Named(name) => struct_fields_for_type(name),
                            _ => HashMap::new(),
                        };
                        self.lower_pattern_inner(sub_pattern, &sub_fields, Some(tmp), params);
                    } else {
                        // Simple binding.
                        self.bind(field_name, tmp);
                    }
                }
            }
        }
    }

    // ========================================================================
    // Expression lowering
    // ========================================================================

    fn lower_expr(&mut self, expr: &TypedExpr) -> Tmp {
        match &expr.kind {
            TypedExprKind::Int(n) => self.emit(Op::ConstInt(*n), expr.ty.clone()),
            TypedExprKind::Float(n) => self.emit(Op::ConstFloat(*n), expr.ty.clone()),
            TypedExprKind::String(s) => self.emit(Op::ConstString(s.clone()), expr.ty.clone()),
            TypedExprKind::Bool(b) => self.emit(Op::ConstBool(*b), expr.ty.clone()),
            TypedExprKind::UnitLiteral { value, unit } => self.emit(
                Op::ConstUnit {
                    value: value.clone(),
                    unit: *unit,
                },
                expr.ty.clone(),
            ),

            TypedExprKind::Ident(name) => self.lookup(name),

            TypedExprKind::Path(segments) => {
                // Standalone enum variant reference (not called).
                if segments.len() == 2 {
                    self.emit(
                        Op::Variant {
                            enum_name: segments[0].clone(),
                            variant: segments[1].clone(),
                            args: vec![],
                        },
                        expr.ty.clone(),
                    )
                } else {
                    self.emit(Op::Unit, Ty::Error)
                }
            }

            TypedExprKind::BinOp { op, left, right } => match op {
                ast::BinOp::And => self.lower_and(left, right),
                ast::BinOp::Or => self.lower_or(left, right),
                _ => {
                    let left_tmp = self.lower_expr(left);
                    let right_tmp = self.lower_expr(right);
                    self.emit(
                        Op::BinOp {
                            op: lower_binop(*op),
                            left: left_tmp,
                            right: right_tmp,
                        },
                        expr.ty.clone(),
                    )
                }
            },

            TypedExprKind::UnaryOp { op, expr: inner } => {
                let tmp = self.lower_expr(inner);
                let hir_op = match op {
                    ast::UnaryOp::Neg => Op::Neg(tmp),
                    ast::UnaryOp::Not => Op::Not(tmp),
                    ast::UnaryOp::Deref => Op::Deref(tmp),
                    ast::UnaryOp::Await => Op::Await(tmp),
                };
                self.emit(hir_op, expr.ty.clone())
            }

            TypedExprKind::Field { expr: inner, field } => {
                let base = self.lower_expr(inner);
                self.emit(
                    Op::Field {
                        base,
                        field: field.clone(),
                    },
                    expr.ty.clone(),
                )
            }

            TypedExprKind::OptionalField { expr: inner, field } => {
                let base = self.lower_expr(inner);
                self.emit(
                    Op::OptionalField {
                        base,
                        field: field.clone(),
                    },
                    expr.ty.clone(),
                )
            }

            TypedExprKind::Call { func, args } => self.lower_call(func, args, &expr.ty),

            TypedExprKind::If {
                cond,
                then_block,
                else_block,
            } => self.lower_if(cond, then_block, else_block.as_deref(), &expr.ty),

            TypedExprKind::List(items) => {
                if items.is_empty() {
                    self.emit(Op::EmptyList, expr.ty.clone())
                } else {
                    let tmps: Vec<Tmp> = items.iter().map(|item| self.lower_expr(item)).collect();
                    self.emit(Op::List(tmps), expr.ty.clone())
                }
            }

            TypedExprKind::StructLit { name, fields } => {
                self.lower_struct_lit(name, fields, &expr.ty)
            }

            TypedExprKind::Block { stmts, result } => {
                self.push_scope();
                self.lower_stmts(stmts);
                let result_tmp = self.lower_expr(result);
                self.pop_scope();
                result_tmp
            }

            TypedExprKind::MutableList => self.emit(Op::EmptyList, expr.ty.clone()),
        }
    }

    // ========================================================================
    // Short-circuit lowering
    // ========================================================================

    /// Lower `a && b` to branches: eval a, if true eval b, else short-circuit.
    fn lower_and(&mut self, left: &TypedExpr, right: &TypedExpr) -> Tmp {
        let result = self.fresh_tmp();

        let left_tmp = self.lower_expr(left);
        let bb_rhs = self.fresh_block();
        let bb_false = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.set_terminator(Terminator::Branch {
            cond: left_tmp,
            then_block: bb_rhs,
            else_block: bb_false,
        });

        // False branch: short-circuit.
        self.switch_to(bb_false);
        self.emit_into(result, Op::ConstBool(false), Ty::Bool);
        self.set_terminator(Terminator::Jump(bb_merge));

        // RHS branch: evaluate right operand.
        self.switch_to(bb_rhs);
        let right_tmp = self.lower_expr(right);
        self.emit_into(result, Op::Copy(right_tmp), Ty::Bool);
        self.set_terminator(Terminator::Jump(bb_merge));

        self.switch_to(bb_merge);
        result
    }

    /// Lower `a || b` to branches: eval a, if true short-circuit, else eval b.
    fn lower_or(&mut self, left: &TypedExpr, right: &TypedExpr) -> Tmp {
        let result = self.fresh_tmp();

        let left_tmp = self.lower_expr(left);
        let bb_true = self.fresh_block();
        let bb_rhs = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.set_terminator(Terminator::Branch {
            cond: left_tmp,
            then_block: bb_true,
            else_block: bb_rhs,
        });

        // True branch: short-circuit.
        self.switch_to(bb_true);
        self.emit_into(result, Op::ConstBool(true), Ty::Bool);
        self.set_terminator(Terminator::Jump(bb_merge));

        // RHS branch: evaluate right operand.
        self.switch_to(bb_rhs);
        let right_tmp = self.lower_expr(right);
        self.emit_into(result, Op::Copy(right_tmp), Ty::Bool);
        self.set_terminator(Terminator::Jump(bb_merge));

        self.switch_to(bb_merge);
        result
    }

    // ========================================================================
    // If/else lowering
    // ========================================================================

    fn lower_if(
        &mut self,
        cond: &TypedExpr,
        then_stmts: &[TypedStmt],
        else_stmts: Option<&[TypedStmt]>,
        result_ty: &Ty,
    ) -> Tmp {
        let result = self.fresh_tmp();
        let cond_tmp = self.lower_expr(cond);

        let bb_then = self.fresh_block();
        let bb_else = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.set_terminator(Terminator::Branch {
            cond: cond_tmp,
            then_block: bb_then,
            else_block: bb_else,
        });

        // Then branch.
        self.switch_to(bb_then);
        self.push_scope();
        let then_result = self.lower_stmts_result(then_stmts);
        self.pop_scope();
        self.emit_into(result, Op::Copy(then_result), result_ty.clone());
        self.set_terminator(Terminator::Jump(bb_merge));

        // Else branch.
        self.switch_to(bb_else);
        if let Some(stmts) = else_stmts {
            self.push_scope();
            let else_result = self.lower_stmts_result(stmts);
            self.pop_scope();
            self.emit_into(result, Op::Copy(else_result), result_ty.clone());
        } else {
            self.emit_into(result, Op::Unit, Ty::Unit);
        }
        self.set_terminator(Terminator::Jump(bb_merge));

        self.switch_to(bb_merge);
        result
    }

    // ========================================================================
    // Call lowering
    // ========================================================================

    fn lower_call(&mut self, func: &TypedExpr, args: &[TypedArg], result_ty: &Ty) -> Tmp {
        // Enum variant constructor: Call to a Path with EnumVariant type.
        if let TypedExprKind::Path(segments) = &func.kind {
            if segments.len() == 2 {
                let lowered_args = self.lower_args(args);
                return self.emit(
                    Op::Variant {
                        enum_name: segments[0].clone(),
                        variant: segments[1].clone(),
                        args: lowered_args,
                    },
                    result_ty.clone(),
                );
            }
        }

        // Regular (builtin) function call.
        let name = match &func.kind {
            TypedExprKind::Ident(name) => name.clone(),
            _ => "<unknown>".into(),
        };

        let lowered_args = self.lower_args(args);
        self.emit(
            Op::Call {
                name,
                args: lowered_args,
            },
            result_ty.clone(),
        )
    }

    fn lower_args(&mut self, args: &[TypedArg]) -> Vec<Tmp> {
        args.iter()
            .map(|arg| match arg {
                TypedArg::Positional(expr) => self.lower_expr(expr),
                TypedArg::Named { value, .. } => self.lower_expr(value),
            })
            .collect()
    }

    // ========================================================================
    // For loop lowering
    // ========================================================================

    fn lower_for(&mut self, var: &str, iter: &TypedExpr, body: &[TypedStmt]) {
        let iter_tmp = self.lower_expr(iter);
        let iter_state = self.emit(Op::IterInit(iter_tmp), iter.ty.clone());

        let bb_header = self.fresh_block();
        let bb_body = self.fresh_block();
        let bb_exit = self.fresh_block();

        self.set_terminator(Terminator::Jump(bb_header));

        // Header: advance iterator or exit.
        self.switch_to(bb_header);
        let value_tmp = self.fresh_tmp();
        self.set_terminator(Terminator::IterNext {
            iter: iter_state,
            value: value_tmp,
            body: bb_body,
            exit: bb_exit,
        });

        // Body.
        self.switch_to(bb_body);
        self.push_scope();
        self.bind(var, value_tmp);
        self.lower_stmts(body);
        self.pop_scope();
        self.set_terminator(Terminator::Jump(bb_header));

        // Continue in exit block.
        self.switch_to(bb_exit);
    }

    // ========================================================================
    // Struct literal lowering
    // ========================================================================

    fn lower_struct_lit(&mut self, name: &str, fields: &[TypedStructField], ty: &Ty) -> Tmp {
        let hir_fields: Vec<HirStructField> = fields
            .iter()
            .map(|f| match f {
                TypedStructField::Field { name, value } => {
                    let tmp = self.lower_expr(value);
                    HirStructField::Set {
                        name: name.clone(),
                        value: tmp,
                    }
                }
                TypedStructField::Inherit(var_name) => {
                    let tmp = self.lookup(var_name);
                    HirStructField::Set {
                        name: var_name.clone(),
                        value: tmp,
                    }
                }
                TypedStructField::Spread(var_name) => {
                    let tmp = self.lookup(var_name);
                    HirStructField::Spread(tmp)
                }
            })
            .collect();

        self.emit(
            Op::Struct {
                name: name.into(),
                fields: hir_fields,
            },
            ty.clone(),
        )
    }

    // ========================================================================
    // Statement lowering
    // ========================================================================

    fn lower_stmts(&mut self, stmts: &[TypedStmt]) {
        for stmt in stmts {
            self.lower_stmt(stmt);
        }
    }

    fn lower_stmt(&mut self, stmt: &TypedStmt) {
        match stmt {
            TypedStmt::Let { name, value, .. } | TypedStmt::LetMut { name, value, .. } => {
                let tmp = self.lower_expr(value);
                self.bind(name, tmp);
            }
            TypedStmt::Expr(expr) => {
                self.lower_expr(expr);
            }
            TypedStmt::Return(expr, _) => {
                let tmp = self.lower_expr(expr);
                self.set_terminator(Terminator::Return(tmp));
                let dead = self.fresh_block();
                self.switch_to(dead);
            }
            TypedStmt::For {
                var, iter, body, ..
            } => {
                self.lower_for(var, iter, body);
            }
            TypedStmt::Push { list, value, .. } => {
                let list_tmp = self.lookup(list);
                let val_tmp = self.lower_expr(value);
                self.emit(
                    Op::ListPush {
                        list: list_tmp,
                        value: val_tmp,
                    },
                    Ty::Unit,
                );
            }
        }
    }

    /// Lower a sequence of statements and return the last expression's Tmp.
    fn lower_stmts_result(&mut self, stmts: &[TypedStmt]) -> Tmp {
        let mut last_tmp = None;
        for stmt in stmts {
            match stmt {
                TypedStmt::Expr(expr) => {
                    last_tmp = Some(self.lower_expr(expr));
                }
                TypedStmt::Return(expr, _) => {
                    let tmp = self.lower_expr(expr);
                    self.set_terminator(Terminator::Return(tmp));
                    let dead = self.fresh_block();
                    self.switch_to(dead);
                    return tmp;
                }
                other => {
                    self.lower_stmt(other);
                }
            }
        }
        last_tmp.unwrap_or_else(|| self.emit(Op::Unit, Ty::Unit))
    }
}
