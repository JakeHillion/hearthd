//! Type checker for the HearthD Automations language.
//!
//! Consumes `LoweredProgram` and produces a `CheckResult` containing:
//! - A typed AST with resolved types on every expression
//! - Entity constraints for runtime validation
//! - Type errors (if any)

use std::collections::HashMap;

use chumsky::span::SimpleSpan;
use chumsky::span::Span;
use facet::Facet;

use super::repr::ast;
use super::repr::lowered;
use super::repr::typed::CheckResult;
use super::repr::typed::EntityConstraint;
use super::repr::typed::Ty;
use super::repr::typed::TypeError;
use super::repr::typed::TypedArg;
use super::repr::typed::TypedAutomation;
use super::repr::typed::TypedExpr;
use super::repr::typed::TypedExprKind;
use super::repr::typed::TypedProgram;
use super::repr::typed::TypedStmt;
use super::repr::typed::TypedStructField;
use crate::engine::state;

#[cfg(test)]
mod tests;

// =============================================================================
// TypeRegistry
// =============================================================================

/// Convert a facet shape to a DSL type.
fn shape_to_ty(shape: &facet::Shape) -> Ty {
    // Check struct types first
    if let facet::Type::User(facet::UserType::Struct(_)) = &shape.ty {
        return Ty::Named(shape.type_identifier.to_string());
    }

    // Check container definitions
    match &shape.def {
        facet::Def::Map(map_def) => Ty::Map {
            key: Box::new(shape_to_ty(map_def.k())),
            value: Box::new(shape_to_ty(map_def.v())),
        },
        facet::Def::List(list_def) => Ty::List(Box::new(shape_to_ty(list_def.t))),
        facet::Def::Option(opt_def) => Ty::Option(Box::new(shape_to_ty(opt_def.t()))),
        _ => {
            // Scalar / primitive types
            match shape.type_identifier {
                "i64" | "i32" | "i16" | "i8" | "u64" | "u32" | "u16" | "u8" | "usize" | "isize" => {
                    Ty::Int
                }
                "f64" | "f32" => Ty::Float,
                "bool" => Ty::Bool,
                name if name.contains("String") => Ty::String,
                name => Ty::Named(name.to_string()),
            }
        }
    }
}

/// Information about an enum type in the registry.
struct EnumInfo {
    /// Maps variant name -> variant fields (e.g. "LightStateChanged" -> { entity_id: String, ... })
    variants: HashMap<String, HashMap<String, Ty>>,
}

/// Registry of known types.
///
/// Struct types are resolved directly from facet's `&'static Shape` data
/// (already in `.rodata`), so no runtime HashMap is needed for them.
/// Only enums and entity registries require runtime state.
struct TypeRegistry {
    enums: HashMap<String, EnumInfo>,
    /// Types where field access produces EntityConstraints instead of
    /// looking up static fields. Maps type name -> inner entity type.
    entity_registries: HashMap<String, Ty>,
}

impl TypeRegistry {
    fn new() -> Self {
        let mut reg = Self {
            enums: HashMap::new(),
            entity_registries: HashMap::new(),
        };
        reg.register_enums();
        reg
    }

    /// Register enum types that can't be derived from facet reflection.
    fn register_enums(&mut self) {
        let mut variants = HashMap::new();
        variants.insert("LightStateChanged".into(), {
            let mut fields = HashMap::new();
            fields.insert("entity_id".into(), Ty::String);
            fields.insert("state".into(), Ty::Named("LightState".into()));
            fields
        });
        variants.insert("BinarySensorStateChanged".into(), {
            let mut fields = HashMap::new();
            fields.insert("entity_id".into(), Ty::String);
            fields.insert("state".into(), Ty::Named("BinarySensorState".into()));
            fields
        });
        self.enums.insert("Event".into(), EnumInfo { variants });
    }

    /// Map a DSL type name to its facet shape. Returns `None` for unknown types.
    fn shape_for_type(name: &str) -> Option<&'static facet::Shape> {
        match name {
            "State" => Some(state::State::SHAPE),
            "LightState" => Some(state::LightState::SHAPE),
            "BinarySensorState" => Some(state::BinarySensorState::SHAPE),
            _ => None,
        }
    }

    /// Look up a field on a type. Returns `None` if the type or field is unknown.
    fn lookup_field(&self, ty: &Ty, field: &str) -> Option<Ty> {
        match ty {
            Ty::Named(name) => {
                // Check entity registries first
                if let Some(inner) = self.entity_registries.get(name.as_str()) {
                    return Some(inner.clone());
                }

                // Query facet shape directly
                let shape = Self::shape_for_type(name)?;
                if let facet::Type::User(facet::UserType::Struct(st)) = &shape.ty {
                    for f in st.fields {
                        if f.name == field {
                            return Some(shape_to_ty(f.shape.get()));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Check if a named type is an entity registry.
    fn is_entity_registry(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Named(name) if self.entity_registries.contains_key(name.as_str()))
    }

    /// Get the domain name for an entity registry type.
    fn entity_registry_domain(&self, ty: &Ty) -> Option<String> {
        match ty {
            Ty::Named(name) if self.entity_registries.contains_key(name.as_str()) => {
                Some(name.to_lowercase())
            }
            _ => None,
        }
    }

    /// Resolve an enum variant path (e.g. ["Event", "LightStateChanged"]).
    fn resolve_enum_variant(
        &self,
        enum_name: &str,
        variant_name: &str,
    ) -> Option<&HashMap<String, Ty>> {
        self.enums
            .get(enum_name)
            .and_then(|e| e.variants.get(variant_name))
    }

    /// Check if a name refers to a known enum.
    fn is_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }

    /// Check if a name refers to a known struct type.
    fn is_struct(name: &str) -> bool {
        Self::shape_for_type(name).is_some()
    }

    /// Build a field map for a struct type on demand from facet shapes.
    fn struct_fields(name: &str) -> Option<HashMap<String, Ty>> {
        let shape = Self::shape_for_type(name)?;
        if let facet::Type::User(facet::UserType::Struct(st)) = &shape.ty {
            Some(
                st.fields
                    .iter()
                    .map(|f| (f.name.to_string(), shape_to_ty(f.shape.get())))
                    .collect(),
            )
        } else {
            None
        }
    }
}

// =============================================================================
// TypeEnv
// =============================================================================

/// Scoped variable environment for type checking.
struct TypeEnv {
    scopes: Vec<HashMap<String, Ty>>,
}

impl TypeEnv {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: String, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<&Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Update the type of an existing binding (for mutable variables).
    fn update(&mut self, name: &str, ty: Ty) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), ty);
                return;
            }
        }
    }
}

// =============================================================================
// TypeChecker
// =============================================================================

/// The type checker. Validates a lowered AST and produces a typed AST.
pub struct TypeChecker {
    registry: TypeRegistry,
    env: TypeEnv,
    errors: Vec<TypeError>,
    constraints: Vec<EntityConstraint>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            registry: TypeRegistry::new(),
            env: TypeEnv::new(),
            errors: Vec::new(),
            constraints: Vec::new(),
        }
    }

    fn error(&mut self, span: SimpleSpan, message: String) {
        self.errors.push(TypeError { message, span });
    }

    // =========================================================================
    // Program checking
    // =========================================================================

    pub fn check_program(mut self, program: &lowered::LoweredProgram) -> CheckResult {
        let typed = match program {
            lowered::LoweredProgram::Automation(auto) => {
                TypedProgram::Automation(Box::new(self.check_automation(auto)))
            }
            lowered::LoweredProgram::Template {
                params,
                automations,
            } => {
                // Bind template parameters
                self.env.push_scope();
                for param in params {
                    let ty = self.ast_type_to_ty(&param.node.ty);
                    self.env.bind(param.node.name.clone(), ty);
                }
                let typed_autos: Vec<_> = automations
                    .iter()
                    .map(|a| self.check_automation(a))
                    .collect();
                self.env.pop_scope();
                TypedProgram::Template {
                    params: params.clone(),
                    automations: typed_autos,
                }
            }
        };

        CheckResult {
            program: typed,
            constraints: self.constraints,
            errors: self.errors,
        }
    }

    fn check_automation(&mut self, auto: &lowered::LoweredAutomation) -> TypedAutomation {
        self.env.push_scope();

        // The automation input is conceptually `{ event: Event, state: State }`.
        // Check the pattern against this virtual input type.
        let input_fields: HashMap<String, Ty> = [
            ("event".into(), Ty::Named("Event".into())),
            ("state".into(), Ty::Named("State".into())),
        ]
        .into();
        self.check_pattern(&auto.pattern, &input_fields);

        let filter = auto.filter.as_ref().map(|f| {
            let typed = self.check_expr(f);
            if typed.ty != Ty::Bool && typed.ty != Ty::Error {
                self.error(f.span(), format!("filter must be Bool, found {}", typed.ty));
            }
            typed
        });

        let body: Vec<_> = auto.body.iter().map(|s| self.check_stmt(s)).collect();

        // Validate return type
        let body_ty = self.body_type(&body);
        match auto.kind {
            ast::AutomationKind::Observer => {
                if !self.is_event_list(&body_ty) && body_ty != Ty::Error && body_ty != Ty::Unit {
                    let span = auto
                        .body
                        .last()
                        .map(|s| s.span())
                        .unwrap_or(SimpleSpan::new((), 0..0));
                    self.error(
                        span,
                        format!("observer body must return [Event], found {}", body_ty),
                    );
                }
            }
            ast::AutomationKind::Mutator => {
                if !self.is_event_type(&body_ty) && body_ty != Ty::Error && body_ty != Ty::Unit {
                    let span = auto
                        .body
                        .last()
                        .map(|s| s.span())
                        .unwrap_or(SimpleSpan::new((), 0..0));
                    self.error(
                        span,
                        format!("mutator body must return Event, found {}", body_ty),
                    );
                }
            }
        }

        self.env.pop_scope();

        TypedAutomation {
            kind: auto.kind,
            pattern: auto.pattern.clone(),
            filter,
            body,
        }
    }

    fn is_event_list(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::List(inner) if self.is_event_type(inner))
    }

    fn is_event_type(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Named(n) if n == "Event")
            || matches!(ty, Ty::EnumVariant { enum_name, .. } if enum_name == "Event")
    }

    fn body_type(&self, body: &[TypedStmt]) -> Ty {
        if let Some(last) = body.last() {
            match last {
                TypedStmt::Expr(expr) => expr.ty.clone(),
                TypedStmt::Return(expr, _) => expr.ty.clone(),
                _ => Ty::Unit,
            }
        } else {
            Ty::Unit
        }
    }

    // =========================================================================
    // Pattern checking
    // =========================================================================

    fn check_pattern(
        &mut self,
        pattern: &ast::Spanned<ast::Pattern>,
        available_fields: &HashMap<String, Ty>,
    ) {
        match &pattern.node {
            ast::Pattern::Ident(name) => {
                // Bind the whole struct as a single variable -- use a generic named type
                self.env.bind(name.clone(), Ty::Named("Input".into()));
            }
            ast::Pattern::Struct { fields, .. } => {
                for field in fields {
                    let field_name = &field.node.name;
                    let field_ty = available_fields
                        .get(field_name.as_str())
                        .cloned()
                        .unwrap_or_else(|| {
                            self.error(
                                field.span,
                                format!("unknown field '{}' in pattern", field_name),
                            );
                            Ty::Error
                        });

                    if let Some(sub_pattern) = &field.node.pattern {
                        // Nested destructuring: look up fields of the field's type
                        let sub_fields = self.type_fields(&field_ty);
                        self.check_pattern(sub_pattern, &sub_fields);
                    } else {
                        // Simple binding: bind field name to its type
                        self.env.bind(field_name.clone(), field_ty);
                    }
                }
            }
        }
    }

    /// Get the fields of a type for nested pattern matching.
    fn type_fields(&self, ty: &Ty) -> HashMap<String, Ty> {
        match ty {
            Ty::Named(name) => TypeRegistry::struct_fields(name).unwrap_or_default(),
            _ => HashMap::new(),
        }
    }

    // =========================================================================
    // Statement checking
    // =========================================================================

    fn check_stmt(&mut self, stmt: &lowered::Spanned<lowered::LoweredStmt>) -> TypedStmt {
        match &stmt.node {
            lowered::LoweredStmt::Let { name, value } => {
                let typed_value = self.check_expr(value);
                self.env.bind(name.clone(), typed_value.ty.clone());
                TypedStmt::Let {
                    name: name.clone(),
                    value: typed_value,
                    origin: stmt.origin.clone(),
                }
            }
            lowered::LoweredStmt::LetMut { name, value } => {
                let typed_value = self.check_expr(value);
                // Mutable list starts as List(Error), refined by Push
                let ty = typed_value.ty.clone();
                self.env.bind(name.clone(), ty);
                TypedStmt::LetMut {
                    name: name.clone(),
                    value: typed_value,
                    origin: stmt.origin.clone(),
                }
            }
            lowered::LoweredStmt::Expr(expr) => TypedStmt::Expr(self.check_expr(expr)),
            lowered::LoweredStmt::Return(expr) => {
                let typed = self.check_expr(expr);
                TypedStmt::Return(typed, stmt.origin.clone())
            }
            lowered::LoweredStmt::For { var, iter, body } => {
                let typed_iter = self.check_expr(iter);
                let elem_ty = match &typed_iter.ty {
                    Ty::List(inner) => *inner.clone(),
                    Ty::Set(inner) => *inner.clone(),
                    Ty::Map { key, .. } => *key.clone(),
                    Ty::Error => Ty::Error,
                    other => {
                        self.error(iter.span(), format!("cannot iterate over {}", other));
                        Ty::Error
                    }
                };

                self.env.push_scope();
                self.env.bind(var.clone(), elem_ty);
                let typed_body: Vec<_> = body.iter().map(|s| self.check_stmt(s)).collect();
                self.env.pop_scope();

                TypedStmt::For {
                    var: var.clone(),
                    iter: typed_iter,
                    body: typed_body,
                    origin: stmt.origin.clone(),
                }
            }
            lowered::LoweredStmt::Push { list, value } => {
                let typed_value = self.check_expr(value);

                // Refine the mutable list's element type
                if let Some(list_ty) = self.env.lookup(list).cloned() {
                    match &list_ty {
                        Ty::List(inner) if **inner == Ty::Error => {
                            // First push: refine from List(Error) to List(value_ty)
                            self.env
                                .update(list, Ty::List(Box::new(typed_value.ty.clone())));
                        }
                        _ => {}
                    }
                }

                TypedStmt::Push {
                    list: list.clone(),
                    value: typed_value,
                    origin: stmt.origin.clone(),
                }
            }
        }
    }

    // =========================================================================
    // Expression checking
    // =========================================================================

    fn check_expr(&mut self, expr: &lowered::Spanned<lowered::LoweredExpr>) -> TypedExpr {
        let origin = expr.origin.clone();
        let span = expr.span();

        match &expr.node {
            // Literals
            lowered::LoweredExpr::Int(n) => TypedExpr {
                kind: TypedExprKind::Int(*n),
                ty: Ty::Int,
                origin,
            },
            lowered::LoweredExpr::Float(s) => {
                let value = s.parse::<f64>().unwrap_or_else(|_| {
                    self.error(span, format!("invalid float literal '{}'", s));
                    0.0
                });
                TypedExpr {
                    kind: TypedExprKind::Float(value),
                    ty: Ty::Float,
                    origin,
                }
            }
            lowered::LoweredExpr::String(s) => TypedExpr {
                kind: TypedExprKind::String(s.clone()),
                ty: Ty::String,
                origin,
            },
            lowered::LoweredExpr::Bool(b) => TypedExpr {
                kind: TypedExprKind::Bool(*b),
                ty: Ty::Bool,
                origin,
            },

            // Unit literals
            lowered::LoweredExpr::UnitLiteral { value, unit } => {
                let ty = match unit {
                    ast::UnitType::Seconds
                    | ast::UnitType::Minutes
                    | ast::UnitType::Hours
                    | ast::UnitType::Days => Ty::Duration,
                    ast::UnitType::Degrees | ast::UnitType::Radians => Ty::Angle,
                    ast::UnitType::Celsius | ast::UnitType::Fahrenheit | ast::UnitType::Kelvin => {
                        Ty::Temperature
                    }
                };
                TypedExpr {
                    kind: TypedExprKind::UnitLiteral {
                        value: value.clone(),
                        unit: *unit,
                    },
                    ty,
                    origin,
                }
            }

            // Identifiers
            lowered::LoweredExpr::Ident(name) => {
                let ty = if let Some(ty) = self.env.lookup(name) {
                    ty.clone()
                } else {
                    self.error(span, format!("undefined variable '{}'", name));
                    Ty::Error
                };
                TypedExpr {
                    kind: TypedExprKind::Ident(name.clone()),
                    ty,
                    origin,
                }
            }

            // Paths (e.g. Event::LightOff)
            lowered::LoweredExpr::Path(segments) => self.check_path(segments, span, origin),

            // Binary operations
            lowered::LoweredExpr::BinOp { op, left, right } => {
                let typed_left = self.check_expr(left);
                let typed_right = self.check_expr(right);
                let ty = self.check_binop(*op, &typed_left.ty, &typed_right.ty, span);
                TypedExpr {
                    kind: TypedExprKind::BinOp {
                        op: *op,
                        left: Box::new(typed_left),
                        right: Box::new(typed_right),
                    },
                    ty,
                    origin,
                }
            }

            // Unary operations
            lowered::LoweredExpr::UnaryOp { op, expr: inner } => {
                let typed_inner = self.check_expr(inner);
                let ty = self.check_unaryop(*op, &typed_inner.ty, span);
                TypedExpr {
                    kind: TypedExprKind::UnaryOp {
                        op: *op,
                        expr: Box::new(typed_inner),
                    },
                    ty,
                    origin,
                }
            }

            // Field access
            lowered::LoweredExpr::Field { expr: inner, field } => {
                let typed_inner = self.check_expr(inner);
                let ty = self.check_field_access(&typed_inner.ty, field, span);
                TypedExpr {
                    kind: TypedExprKind::Field {
                        expr: Box::new(typed_inner),
                        field: field.clone(),
                    },
                    ty,
                    origin,
                }
            }

            // Optional field access
            lowered::LoweredExpr::OptionalField { expr: inner, field } => {
                let typed_inner = self.check_expr(inner);
                let inner_ty = match &typed_inner.ty {
                    Ty::Option(inner) => *inner.clone(),
                    other => other.clone(),
                };
                let field_ty = self.check_field_access(&inner_ty, field, span);
                let ty = Ty::Option(Box::new(field_ty));
                TypedExpr {
                    kind: TypedExprKind::OptionalField {
                        expr: Box::new(typed_inner),
                        field: field.clone(),
                    },
                    ty,
                    origin,
                }
            }

            // Function calls
            lowered::LoweredExpr::Call { func, args } => self.check_call(func, args, span, origin),

            // If expressions
            lowered::LoweredExpr::If {
                cond,
                then_block,
                else_block,
            } => {
                let typed_cond = self.check_expr(cond);
                if typed_cond.ty != Ty::Bool && typed_cond.ty != Ty::Error {
                    self.error(
                        cond.span(),
                        format!("if condition must be Bool, found {}", typed_cond.ty),
                    );
                }

                self.env.push_scope();
                let typed_then: Vec<_> = then_block.iter().map(|s| self.check_stmt(s)).collect();
                self.env.pop_scope();

                let typed_else = else_block.as_ref().map(|stmts| {
                    self.env.push_scope();
                    let typed: Vec<_> = stmts.iter().map(|s| self.check_stmt(s)).collect();
                    self.env.pop_scope();
                    typed
                });

                let then_ty = self.body_type(&typed_then);
                let ty = if let Some(ref else_stmts) = typed_else {
                    let else_ty = self.body_type(else_stmts);
                    self.unify(&then_ty, &else_ty)
                } else {
                    Ty::Unit
                };

                TypedExpr {
                    kind: TypedExprKind::If {
                        cond: Box::new(typed_cond),
                        then_block: typed_then,
                        else_block: typed_else,
                    },
                    ty,
                    origin,
                }
            }

            // List literals
            lowered::LoweredExpr::List(items) => {
                if items.is_empty() {
                    TypedExpr {
                        kind: TypedExprKind::List(vec![]),
                        ty: Ty::List(Box::new(Ty::Error)),
                        origin,
                    }
                } else {
                    let typed_items: Vec<_> = items.iter().map(|e| self.check_expr(e)).collect();
                    let elem_ty = typed_items
                        .iter()
                        .map(|e| &e.ty)
                        .find(|t| **t != Ty::Error)
                        .cloned()
                        .unwrap_or(Ty::Error);
                    TypedExpr {
                        kind: TypedExprKind::List(typed_items),
                        ty: Ty::List(Box::new(elem_ty)),
                        origin,
                    }
                }
            }

            // Struct literals
            lowered::LoweredExpr::StructLit { name, fields } => {
                self.check_struct_lit(name, fields, span, origin)
            }

            // Block expressions
            lowered::LoweredExpr::Block { stmts, result } => {
                self.env.push_scope();
                let typed_stmts: Vec<_> = stmts.iter().map(|s| self.check_stmt(s)).collect();
                let typed_result = self.check_expr(result);
                let ty = typed_result.ty.clone();
                self.env.pop_scope();

                TypedExpr {
                    kind: TypedExprKind::Block {
                        stmts: typed_stmts,
                        result: Box::new(typed_result),
                    },
                    ty,
                    origin,
                }
            }

            // Mutable list (empty)
            lowered::LoweredExpr::MutableList => TypedExpr {
                kind: TypedExprKind::MutableList,
                ty: Ty::List(Box::new(Ty::Error)),
                origin,
            },
        }
    }

    // =========================================================================
    // Binary / Unary operators
    // =========================================================================

    fn check_binop(&mut self, op: ast::BinOp, left: &Ty, right: &Ty, span: SimpleSpan) -> Ty {
        // Error propagation
        if *left == Ty::Error || *right == Ty::Error {
            return Ty::Error;
        }

        match op {
            // Arithmetic
            ast::BinOp::Add
            | ast::BinOp::Sub
            | ast::BinOp::Mul
            | ast::BinOp::Div
            | ast::BinOp::Mod => {
                if self.is_numeric(left) && self.is_numeric(right) {
                    // Float contaminates
                    if *left == Ty::Float || *right == Ty::Float {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                } else {
                    self.error(
                        span,
                        format!(
                            "arithmetic operator '{}' requires numeric operands, found {} and {}",
                            op, left, right
                        ),
                    );
                    Ty::Error
                }
            }

            // Comparison
            ast::BinOp::Lt | ast::BinOp::Le | ast::BinOp::Gt | ast::BinOp::Ge => {
                if self.is_numeric(left) && self.is_numeric(right) {
                    Ty::Bool
                } else {
                    self.error(
                        span,
                        format!(
                            "comparison operator '{}' requires numeric operands, found {} and {}",
                            op, left, right
                        ),
                    );
                    Ty::Error
                }
            }

            // Equality
            ast::BinOp::Eq | ast::BinOp::Ne => Ty::Bool,

            // Membership
            ast::BinOp::In => match right {
                Ty::List(_) | Ty::Set(_) | Ty::Map { .. } => Ty::Bool,
                _ => {
                    self.error(
                        span,
                        format!("'in' requires collection on right side, found {}", right),
                    );
                    Ty::Error
                }
            },

            // Logical
            ast::BinOp::And | ast::BinOp::Or => {
                if *left == Ty::Bool && *right == Ty::Bool {
                    Ty::Bool
                } else {
                    self.error(
                        span,
                        format!(
                            "logical operator '{}' requires Bool operands, found {} and {}",
                            op, left, right
                        ),
                    );
                    Ty::Error
                }
            }
        }
    }

    fn check_unaryop(&mut self, op: ast::UnaryOp, operand: &Ty, span: SimpleSpan) -> Ty {
        if *operand == Ty::Error {
            return Ty::Error;
        }

        match op {
            ast::UnaryOp::Neg => {
                if self.is_numeric(operand) {
                    operand.clone()
                } else {
                    self.error(
                        span,
                        format!("negation requires numeric type, found {}", operand),
                    );
                    Ty::Error
                }
            }
            ast::UnaryOp::Not => {
                if *operand == Ty::Bool {
                    Ty::Bool
                } else {
                    self.error(
                        span,
                        format!("logical not requires Bool, found {}", operand),
                    );
                    Ty::Error
                }
            }
            ast::UnaryOp::Await => match operand {
                Ty::Future(inner) => *inner.clone(),
                _ => {
                    self.error(
                        span,
                        format!("await requires Future type, found {}", operand),
                    );
                    Ty::Error
                }
            },
            // Deref is permissive for now
            ast::UnaryOp::Deref => operand.clone(),
        }
    }

    fn is_numeric(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::Float)
    }

    // =========================================================================
    // Field access
    // =========================================================================

    fn check_field_access(&mut self, ty: &Ty, field: &str, span: SimpleSpan) -> Ty {
        if *ty == Ty::Error {
            return Ty::Error;
        }

        // Event field access is permissive (deferred)
        if matches!(ty, Ty::Named(n) if n == "Event") {
            return Ty::Error;
        }

        // Check entity registry
        if self.registry.is_entity_registry(ty) {
            if let Some(domain) = self.registry.entity_registry_domain(ty) {
                self.constraints.push(EntityConstraint {
                    domain,
                    entity: field.to_string(),
                    span,
                });
            }
        }

        if let Some(field_ty) = self.registry.lookup_field(ty, field) {
            field_ty
        } else {
            self.error(span, format!("no field '{}' on type {}", field, ty));
            Ty::Error
        }
    }

    // =========================================================================
    // Path resolution
    // =========================================================================

    fn check_path(
        &mut self,
        segments: &[String],
        span: SimpleSpan,
        origin: lowered::Origin,
    ) -> TypedExpr {
        if segments.len() == 2 {
            let enum_name = &segments[0];
            let variant_name = &segments[1];

            if self.registry.is_enum(enum_name) {
                if self
                    .registry
                    .resolve_enum_variant(enum_name, variant_name)
                    .is_some()
                {
                    return TypedExpr {
                        kind: TypedExprKind::Path(segments.to_vec()),
                        ty: Ty::EnumVariant {
                            enum_name: enum_name.clone(),
                            variant_name: variant_name.clone(),
                        },
                        origin,
                    };
                } else {
                    self.error(
                        span,
                        format!("unknown variant '{}' on enum '{}'", variant_name, enum_name),
                    );
                }
            } else {
                self.error(span, format!("unknown type '{}'", enum_name));
            }
        } else {
            self.error(
                span,
                format!("unsupported path with {} segments", segments.len()),
            );
        }

        TypedExpr {
            kind: TypedExprKind::Path(segments.to_vec()),
            ty: Ty::Error,
            origin,
        }
    }

    // =========================================================================
    // Function calls
    // =========================================================================

    fn check_call(
        &mut self,
        func: &lowered::Spanned<lowered::LoweredExpr>,
        args: &[lowered::Spanned<lowered::LoweredArg>],
        span: SimpleSpan,
        origin: lowered::Origin,
    ) -> TypedExpr {
        // Check if this is a call to an enum variant constructor
        if let lowered::LoweredExpr::Path(segments) = &func.node {
            if segments.len() == 2 {
                let enum_name = &segments[0];
                let variant_name = &segments[1];
                if let Some(_variant_fields) = self
                    .registry
                    .resolve_enum_variant(enum_name, variant_name)
                    .cloned()
                {
                    let typed_func = self.check_path(segments, func.span(), func.origin.clone());
                    let typed_args = self.check_args(args);

                    // For enum variant constructors, the result is a Named type
                    // of the enum (e.g. Event::LightOff(...) has type Event)
                    let ty = Ty::Named(enum_name.clone());

                    return TypedExpr {
                        kind: TypedExprKind::Call {
                            func: Box::new(typed_func),
                            args: typed_args,
                        },
                        ty,
                        origin,
                    };
                }
            }
        }

        // Check if this is a builtin function call
        if let lowered::LoweredExpr::Ident(name) = &func.node {
            let typed_args = self.check_args(args);
            let arg_types: Vec<_> = typed_args
                .iter()
                .map(|a| match a {
                    TypedArg::Positional(e) => e.ty.clone(),
                    TypedArg::Named { value, .. } => value.ty.clone(),
                })
                .collect();

            if let Some(ret_ty) = self.resolve_builtin_call(name, &arg_types, span) {
                let typed_func = TypedExpr {
                    kind: TypedExprKind::Ident(name.clone()),
                    ty: Ty::Error, // function identifier type not meaningful
                    origin: func.origin.clone(),
                };
                return TypedExpr {
                    kind: TypedExprKind::Call {
                        func: Box::new(typed_func),
                        args: typed_args,
                    },
                    ty: ret_ty,
                    origin,
                };
            }

            // Not a builtin - check if it's a variable that's callable
            let func_ty = self.env.lookup(name).cloned();
            let typed_func = TypedExpr {
                kind: TypedExprKind::Ident(name.clone()),
                ty: func_ty.clone().unwrap_or(Ty::Error),
                origin: func.origin.clone(),
            };

            if func_ty.is_none() {
                self.error(span, format!("undefined function '{}'", name));
            }

            return TypedExpr {
                kind: TypedExprKind::Call {
                    func: Box::new(typed_func),
                    args: typed_args,
                },
                ty: Ty::Error,
                origin,
            };
        }

        // Generic call expression
        let typed_func = self.check_expr(func);
        let typed_args = self.check_args(args);

        TypedExpr {
            kind: TypedExprKind::Call {
                func: Box::new(typed_func),
                args: typed_args,
            },
            ty: Ty::Error,
            origin,
        }
    }

    fn check_args(&mut self, args: &[lowered::Spanned<lowered::LoweredArg>]) -> Vec<TypedArg> {
        args.iter()
            .map(|a| match &a.node {
                lowered::LoweredArg::Positional(expr) => {
                    TypedArg::Positional(self.check_expr(expr))
                }
                lowered::LoweredArg::Named { name, value } => TypedArg::Named {
                    name: name.clone(),
                    value: self.check_expr(value),
                },
            })
            .collect()
    }

    /// Resolve a call to a built-in function. Returns `Some(return_type)` if
    /// the name is a known builtin, `None` otherwise.
    fn resolve_builtin_call(
        &mut self,
        name: &str,
        arg_types: &[Ty],
        span: SimpleSpan,
    ) -> Option<Ty> {
        match name {
            "sleep" => {
                if arg_types.len() != 1 {
                    self.error(span, "sleep() takes exactly 1 argument".into());
                } else if arg_types[0] != Ty::Duration && arg_types[0] != Ty::Error {
                    self.error(
                        span,
                        format!("sleep() requires Duration, found {}", arg_types[0]),
                    );
                }
                Some(Ty::Future(Box::new(Ty::Unit)))
            }
            "sleep_unique" => {
                if arg_types.len() != 1 {
                    self.error(span, "sleep_unique() takes exactly 1 argument".into());
                } else if arg_types[0] != Ty::Duration && arg_types[0] != Ty::Error {
                    self.error(
                        span,
                        format!("sleep_unique() requires Duration, found {}", arg_types[0]),
                    );
                }
                Some(Ty::Future(Box::new(Ty::Bool)))
            }
            "keys" => {
                if arg_types.len() != 1 {
                    self.error(span, "keys() takes exactly 1 argument".into());
                    return Some(Ty::Error);
                }
                match &arg_types[0] {
                    Ty::Map { key, .. } => Some(Ty::List(key.clone())),
                    Ty::Error => Some(Ty::List(Box::new(Ty::Error))),
                    other => {
                        self.error(span, format!("keys() requires Map, found {}", other));
                        Some(Ty::Error)
                    }
                }
            }
            "values" => {
                if arg_types.len() != 1 {
                    self.error(span, "values() takes exactly 1 argument".into());
                    return Some(Ty::Error);
                }
                match &arg_types[0] {
                    Ty::Map { value, .. } => Some(Ty::List(value.clone())),
                    Ty::Error => Some(Ty::List(Box::new(Ty::Error))),
                    other => {
                        self.error(span, format!("values() requires Map, found {}", other));
                        Some(Ty::Error)
                    }
                }
            }
            "len" => {
                if arg_types.len() != 1 {
                    self.error(span, "len() takes exactly 1 argument".into());
                } else {
                    match &arg_types[0] {
                        Ty::List(_) | Ty::Set(_) | Ty::Map { .. } | Ty::String | Ty::Error => {}
                        other => {
                            self.error(
                                span,
                                format!("len() requires collection or String, found {}", other),
                            );
                        }
                    }
                }
                Some(Ty::Int)
            }
            "abs" => {
                if arg_types.len() != 1 {
                    self.error(span, "abs() takes exactly 1 argument".into());
                    return Some(Ty::Error);
                }
                if self.is_numeric(&arg_types[0]) || arg_types[0] == Ty::Error {
                    Some(arg_types[0].clone())
                } else {
                    self.error(
                        span,
                        format!("abs() requires numeric type, found {}", arg_types[0]),
                    );
                    Some(Ty::Error)
                }
            }
            "min" | "max" => {
                if arg_types.len() != 2 {
                    self.error(span, format!("{}() takes exactly 2 arguments", name));
                    return Some(Ty::Error);
                }
                if (self.is_numeric(&arg_types[0]) || arg_types[0] == Ty::Error)
                    && (self.is_numeric(&arg_types[1]) || arg_types[1] == Ty::Error)
                {
                    if arg_types[0] == Ty::Float || arg_types[1] == Ty::Float {
                        Some(Ty::Float)
                    } else {
                        Some(Ty::Int)
                    }
                } else {
                    self.error(
                        span,
                        format!(
                            "{}() requires numeric arguments, found {} and {}",
                            name, arg_types[0], arg_types[1]
                        ),
                    );
                    Some(Ty::Error)
                }
            }
            "clamp" => {
                if arg_types.len() != 3 {
                    self.error(span, "clamp() takes exactly 3 arguments".into());
                    return Some(Ty::Error);
                }
                let all_numeric = arg_types
                    .iter()
                    .all(|t| self.is_numeric(t) || *t == Ty::Error);
                if all_numeric {
                    if arg_types.contains(&Ty::Float) {
                        Some(Ty::Float)
                    } else {
                        Some(Ty::Int)
                    }
                } else {
                    self.error(span, "clamp() requires numeric arguments".into());
                    Some(Ty::Error)
                }
            }
            "filter" => {
                if arg_types.len() != 2 {
                    self.error(span, "filter() takes exactly 2 arguments".into());
                    return Some(Ty::Error);
                }
                match &arg_types[0] {
                    Ty::List(_) => Some(arg_types[0].clone()),
                    Ty::Error => Some(Ty::Error),
                    other => {
                        self.error(
                            span,
                            format!("filter() first argument must be a list, found {}", other),
                        );
                        Some(Ty::Error)
                    }
                }
            }
            "wait" => {
                // wait is an alias / variant of sleep with named args
                Some(Ty::Future(Box::new(Ty::Unit)))
            }
            _ => None,
        }
    }

    // =========================================================================
    // Struct literals
    // =========================================================================

    fn check_struct_lit(
        &mut self,
        name: &str,
        fields: &[lowered::Spanned<lowered::LoweredStructField>],
        span: SimpleSpan,
        origin: lowered::Origin,
    ) -> TypedExpr {
        let is_known = TypeRegistry::is_struct(name) || self.registry.is_enum(name);

        let typed_fields: Vec<_> = fields
            .iter()
            .map(|f| match &f.node {
                lowered::LoweredStructField::Field { name: fname, value } => {
                    let typed_value = self.check_expr(value);
                    TypedStructField::Field {
                        name: fname.clone(),
                        value: typed_value,
                    }
                }
                lowered::LoweredStructField::Inherit(iname) => {
                    TypedStructField::Inherit(iname.clone())
                }
                lowered::LoweredStructField::Spread(sname) => {
                    TypedStructField::Spread(sname.clone())
                }
            })
            .collect();

        let ty = if is_known {
            Ty::Named(name.to_string())
        } else {
            self.error(span, format!("unknown struct type '{}'", name));
            Ty::Error
        };

        TypedExpr {
            kind: TypedExprKind::StructLit {
                name: name.to_string(),
                fields: typed_fields,
            },
            ty,
            origin,
        }
    }

    // =========================================================================
    // Type unification
    // =========================================================================

    fn unify(&self, a: &Ty, b: &Ty) -> Ty {
        if *a == Ty::Error {
            return b.clone();
        }
        if *b == Ty::Error {
            return a.clone();
        }
        if a == b {
            return a.clone();
        }
        // Int/Float coerce to Float
        if (*a == Ty::Int && *b == Ty::Float) || (*a == Ty::Float && *b == Ty::Int) {
            return Ty::Float;
        }
        // Named types that are both Event variants unify to Event
        if self.is_event_type(a) && self.is_event_type(b) {
            return Ty::Named("Event".into());
        }
        // List unification
        if let (Ty::List(inner_a), Ty::List(inner_b)) = (a, b) {
            return Ty::List(Box::new(self.unify(inner_a, inner_b)));
        }
        // Fall back to first type (could emit error, but for now be permissive)
        a.clone()
    }

    // =========================================================================
    // AST type -> Ty conversion
    // =========================================================================

    fn ast_type_to_ty(&self, ty: &ast::Type) -> Ty {
        match ty {
            ast::Type::Named(name) => match name.as_str() {
                "Int" | "i64" => Ty::Int,
                "Float" | "f64" => Ty::Float,
                "Bool" | "bool" => Ty::Bool,
                "String" => Ty::String,
                "Duration" => Ty::Duration,
                "Angle" => Ty::Angle,
                "Temperature" => Ty::Temperature,
                _ => Ty::Named(name.clone()),
            },
            ast::Type::List(inner) => Ty::List(Box::new(self.ast_type_to_ty(inner))),
            ast::Type::Set(inner) => Ty::Set(Box::new(self.ast_type_to_ty(inner))),
            ast::Type::Map { key, value } => Ty::Map {
                key: Box::new(self.ast_type_to_ty(key)),
                value: Box::new(self.ast_type_to_ty(value)),
            },
            ast::Type::Option(inner) => Ty::Option(Box::new(self.ast_type_to_ty(inner))),
        }
    }
}

/// Convenience function: parse, desugar, and type-check a program.
pub fn check_program(program: &lowered::LoweredProgram) -> CheckResult {
    TypeChecker::new().check_program(program)
}

/// Render type errors as pretty diagnostics using ariadne.
///
/// Each error becomes an ariadne `Report` with a labeled source span,
/// producing output with line numbers, source context, and colored carets.
pub fn format_type_errors(errors: &[TypeError], source: &str, filename: &str) -> String {
    use ariadne::Color;
    use ariadne::Label;
    use ariadne::Report;
    use ariadne::ReportKind;
    use ariadne::Source;

    let mut output = Vec::new();
    for error in errors {
        let span = error.span.start..error.span.end;
        let report = Report::build(ReportKind::Error, (filename, span.clone()))
            .with_message(&error.message)
            .with_label(
                Label::new((filename, span))
                    .with_message(&error.message)
                    .with_color(Color::Red),
            )
            .finish();

        report
            .write((filename, Source::from(source)), &mut output)
            .ok();
    }
    String::from_utf8_lossy(&output).to_string()
}
