//! Bytecode VM for the HearthD Automations language.
//!
//! Two entry points share a single step function: [`run_sync`] for filter
//! evaluation (cheap, no suspension) and [`run_async`] for body
//! evaluation (supports `await`).
//!
//! The VM is a flat register machine: each `Bytecode` declares
//! `num_regs`, the VM allocates that many `Value` slots, and instructions
//! read and write them by index. Decoded operands match the layout in
//! [`super::super::repr::bytecode`].

use std::time::Duration;

use super::value::FutureKind;
use super::value::IterState;
use super::value::Value;
use crate::automations::repr::bytecode::*;
use crate::automations::repr::hir::HirBinOp;

#[cfg(test)]
mod tests;

/// Synchronously run a `Bytecode` function with the given positional
/// parameters. Returns the value passed to `RETURN`.
///
/// `params` must be the same length and order as `bc.params`; each
/// param `Value` is loaded into its declared register before execution
/// begins.
pub fn run_sync(bc: &Bytecode, params: Vec<Value>) -> Result<Value, VmError> {
    assert_eq!(
        params.len(),
        bc.params.len(),
        "param count mismatch: expected {}, got {}",
        bc.params.len(),
        params.len(),
    );

    let mut regs = vec![Value::Unit; bc.num_regs as usize];
    for (slot, value) in bc.params.iter().zip(params) {
        regs[slot.reg as usize] = value;
    }

    let mut pc = 0usize;
    loop {
        match step(bc, &mut regs, &mut pc)? {
            StepResult::Continue => {}
            StepResult::Return(v) => return Ok(v),
            StepResult::Suspend { .. } => return Err(VmError::AwaitInSync),
        }
    }
}

/// Asynchronously run a `Bytecode` function with the given positional
/// parameters. Suspends the executing tokio task at each `await` opcode
/// (e.g. for `sleep` / `sleep_unique`).
pub async fn run_async(bc: &Bytecode, params: Vec<Value>) -> Result<Value, VmError> {
    assert_eq!(
        params.len(),
        bc.params.len(),
        "param count mismatch: expected {}, got {}",
        bc.params.len(),
        params.len(),
    );

    let mut regs = vec![Value::Unit; bc.num_regs as usize];
    for (slot, value) in bc.params.iter().zip(params) {
        regs[slot.reg as usize] = value;
    }

    let mut pc = 0usize;
    loop {
        match step(bc, &mut regs, &mut pc)? {
            StepResult::Continue => {}
            StepResult::Return(v) => return Ok(v),
            StepResult::Suspend { dst, kind, args } => {
                let resume = perform_await(kind, args).await?;
                regs[dst] = resume;
            }
        }
    }
}

async fn perform_await(kind: FutureKind, args: Vec<Value>) -> Result<Value, VmError> {
    let duration = match args.as_slice() {
        [Value::Int(ms)] => Duration::from_millis((*ms).max(0) as u64),
        [Value::Float(s)] => Duration::from_secs_f64((*s).max(0.0)),
        other => {
            return Err(VmError::TypeMismatch(format!(
                "expected one numeric duration argument, got {:?}",
                other
            )));
        }
    };
    match kind {
        FutureKind::Sleep | FutureKind::SleepUnique => {
            tokio::time::sleep(duration).await;
            Ok(Value::Bool(true))
        }
    }
}

/// An error returned by the VM.
#[derive(Debug, Clone)]
pub enum VmError {
    AwaitInSync,
    TypeMismatch(String),
    UnknownField(String),
    UnknownBuiltin(String),
    UnknownOpcode(u8),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::AwaitInSync => write!(f, "await opcode encountered in sync VM"),
            VmError::TypeMismatch(s) => write!(f, "type mismatch: {}", s),
            VmError::UnknownField(s) => write!(f, "unknown field: {}", s),
            VmError::UnknownBuiltin(s) => write!(f, "unknown builtin function: {}", s),
            VmError::UnknownOpcode(b) => write!(f, "unknown opcode 0x{:02x}", b),
        }
    }
}

impl std::error::Error for VmError {}

enum StepResult {
    Continue,
    Return(Value),
    /// The `Await` opcode fired: caller must drive the future to
    /// completion and store the resulting `Value` into register `dst`
    /// before resuming.
    Suspend {
        dst: usize,
        kind: FutureKind,
        args: Vec<Value>,
    },
}

fn step(bc: &Bytecode, regs: &mut [Value], pc: &mut usize) -> Result<StepResult, VmError> {
    let opcode_byte = bc.code[*pc];
    let opcode = Opcode::from_u8(opcode_byte).ok_or(VmError::UnknownOpcode(opcode_byte))?;
    *pc += 1;
    match opcode {
        Opcode::LoadConstInt => {
            let dst = read_u32(&bc.code, pc) as usize;
            let idx = read_u32(&bc.code, pc) as usize;
            regs[dst] = match &bc.consts[idx] {
                Const::Int(n) => Value::Int(*n),
                _ => return Err(VmError::TypeMismatch("const idx not Int".into())),
            };
        }
        Opcode::LoadConstFloat => {
            let dst = read_u32(&bc.code, pc) as usize;
            let idx = read_u32(&bc.code, pc) as usize;
            regs[dst] = match &bc.consts[idx] {
                Const::Float(n) => Value::Float(*n),
                _ => return Err(VmError::TypeMismatch("const idx not Float".into())),
            };
        }
        Opcode::LoadConstString => {
            let dst = read_u32(&bc.code, pc) as usize;
            let idx = read_u32(&bc.code, pc) as usize;
            regs[dst] = match &bc.consts[idx] {
                Const::String(s) => Value::String(s.clone()),
                _ => return Err(VmError::TypeMismatch("const idx not String".into())),
            };
        }
        Opcode::LoadConstBool => {
            let dst = read_u32(&bc.code, pc) as usize;
            let b = bc.code[*pc] != 0;
            *pc += 1;
            regs[dst] = Value::Bool(b);
        }
        Opcode::LoadConstUnit => {
            // Unit literals normalise to milliseconds for durations (e.g.
            // `5min` -> 300000) so the await opcode can consume them as
            // plain numbers. Angles and temperatures fall through as raw
            // numbers; we'll introduce typed wrappers when something
            // actually consumes them.
            let dst = read_u32(&bc.code, pc) as usize;
            let idx = read_u32(&bc.code, pc) as usize;
            regs[dst] = match &bc.consts[idx] {
                Const::UnitLit { value, unit } => normalize_unit_literal(value, *unit)?,
                _ => return Err(VmError::TypeMismatch("const idx not UnitLit".into())),
            };
        }
        Opcode::Unit => {
            let dst = read_u32(&bc.code, pc) as usize;
            regs[dst] = Value::Unit;
        }
        Opcode::BinOp => {
            let dst = read_u32(&bc.code, pc) as usize;
            let tag = BinOpTag::from_u8(bc.code[*pc])
                .ok_or(VmError::TypeMismatch("bad binop tag".into()))?;
            *pc += 1;
            let lhs = read_u32(&bc.code, pc) as usize;
            let rhs = read_u32(&bc.code, pc) as usize;
            regs[dst] = eval_binop(tag.to_hir(), &regs[lhs], &regs[rhs])?;
        }
        Opcode::Neg => {
            let dst = read_u32(&bc.code, pc) as usize;
            let src = read_u32(&bc.code, pc) as usize;
            regs[dst] = match &regs[src] {
                Value::Int(n) => Value::Int(-n),
                Value::Float(n) => Value::Float(-n),
                v => return Err(VmError::TypeMismatch(format!("neg on {:?}", v))),
            };
        }
        Opcode::Not => {
            let dst = read_u32(&bc.code, pc) as usize;
            let src = read_u32(&bc.code, pc) as usize;
            regs[dst] = match &regs[src] {
                Value::Bool(b) => Value::Bool(!b),
                v => return Err(VmError::TypeMismatch(format!("not on {:?}", v))),
            };
        }
        Opcode::Deref => {
            // The DSL's `*` operator is currently a no-op at runtime;
            // the checker uses it for typing, but values flow through
            // unchanged.
            let dst = read_u32(&bc.code, pc) as usize;
            let src = read_u32(&bc.code, pc) as usize;
            regs[dst] = regs[src].clone();
        }
        Opcode::Field => {
            let dst = read_u32(&bc.code, pc) as usize;
            let base = read_u32(&bc.code, pc) as usize;
            let idx = read_u32(&bc.code, pc) as usize;
            let field = const_ident(bc, idx)?;
            regs[dst] = field_access(&regs[base], field)?;
        }
        Opcode::OptionalField => {
            let dst = read_u32(&bc.code, pc) as usize;
            let base = read_u32(&bc.code, pc) as usize;
            let idx = read_u32(&bc.code, pc) as usize;
            let field = const_ident(bc, idx)?;
            regs[dst] = field_access(&regs[base], field)?;
        }
        Opcode::Call => {
            let dst = read_u32(&bc.code, pc) as usize;
            let name_idx = read_u32(&bc.code, pc) as usize;
            let n = read_u32(&bc.code, pc) as usize;
            let arg_regs: Vec<usize> = (0..n).map(|_| read_u32(&bc.code, pc) as usize).collect();
            let args: Vec<Value> = arg_regs.iter().map(|r| regs[*r].clone()).collect();
            let name = const_ident(bc, name_idx)?;
            regs[dst] = call_builtin(name, args)?;
        }
        Opcode::Variant => {
            let dst = read_u32(&bc.code, pc) as usize;
            let enum_idx = read_u32(&bc.code, pc) as usize;
            let variant_idx = read_u32(&bc.code, pc) as usize;
            let n = read_u32(&bc.code, pc) as usize;
            let arg_regs: Vec<usize> = (0..n).map(|_| read_u32(&bc.code, pc) as usize).collect();
            let args: Vec<Value> = arg_regs.iter().map(|r| regs[*r].clone()).collect();
            regs[dst] = Value::Variant {
                enum_name: const_ident(bc, enum_idx)?.to_string(),
                variant: const_ident(bc, variant_idx)?.to_string(),
                args,
            };
        }
        Opcode::EmptyList => {
            let dst = read_u32(&bc.code, pc) as usize;
            regs[dst] = Value::List(Vec::new());
        }
        Opcode::List => {
            let dst = read_u32(&bc.code, pc) as usize;
            let n = read_u32(&bc.code, pc) as usize;
            let mut elems = Vec::with_capacity(n);
            for _ in 0..n {
                let r = read_u32(&bc.code, pc) as usize;
                elems.push(regs[r].clone());
            }
            regs[dst] = Value::List(elems);
        }
        Opcode::ListPush => {
            let list = read_u32(&bc.code, pc) as usize;
            let value = read_u32(&bc.code, pc) as usize;
            let v = regs[value].clone();
            match &mut regs[list] {
                Value::List(items) => items.push(v),
                other => return Err(VmError::TypeMismatch(format!("list_push on {:?}", other))),
            }
        }
        Opcode::IterInit => {
            let dst = read_u32(&bc.code, pc) as usize;
            let src = read_u32(&bc.code, pc) as usize;
            let list = match &regs[src] {
                Value::List(items) => items.clone(),
                other => return Err(VmError::TypeMismatch(format!("iter_init on {:?}", other))),
            };
            regs[dst] = Value::Iter(IterState::new(list));
        }
        Opcode::Struct => {
            let dst = read_u32(&bc.code, pc) as usize;
            let _name_idx = read_u32(&bc.code, pc) as usize;
            let n = read_u32(&bc.code, pc) as usize;
            let mut fields: std::collections::BTreeMap<String, Value> =
                std::collections::BTreeMap::new();
            for _ in 0..n {
                let tag = bc.code[*pc];
                *pc += 1;
                match tag {
                    0 => {
                        // Set { name, value }
                        let field_idx = read_u32(&bc.code, pc) as usize;
                        let value = read_u32(&bc.code, pc) as usize;
                        let name = const_ident(bc, field_idx)?.to_string();
                        fields.insert(name, regs[value].clone());
                    }
                    1 => {
                        // Spread(src) — flatten source struct fields.
                        let src = read_u32(&bc.code, pc) as usize;
                        if let Value::Struct(src_fields) = &regs[src] {
                            for (k, v) in src_fields {
                                fields.entry(k.clone()).or_insert_with(|| v.clone());
                            }
                        } else {
                            return Err(VmError::TypeMismatch(format!(
                                "struct spread on {:?}",
                                regs[src]
                            )));
                        }
                    }
                    _ => return Err(VmError::TypeMismatch("bad struct field tag".into())),
                }
            }
            regs[dst] = Value::Struct(fields);
        }
        Opcode::Copy => {
            let dst = read_u32(&bc.code, pc) as usize;
            let src = read_u32(&bc.code, pc) as usize;
            regs[dst] = regs[src].clone();
        }
        Opcode::Jump => {
            let target = read_u32(&bc.code, pc) as usize;
            *pc = target;
        }
        Opcode::JumpIf => {
            let cond = read_u32(&bc.code, pc) as usize;
            let then_t = read_u32(&bc.code, pc) as usize;
            let else_t = read_u32(&bc.code, pc) as usize;
            let take_then = match &regs[cond] {
                Value::Bool(b) => *b,
                other => return Err(VmError::TypeMismatch(format!("jump_if on {:?}", other))),
            };
            *pc = if take_then { then_t } else { else_t };
        }
        Opcode::IterNext => {
            let iter = read_u32(&bc.code, pc) as usize;
            let value = read_u32(&bc.code, pc) as usize;
            let body = read_u32(&bc.code, pc) as usize;
            let exit = read_u32(&bc.code, pc) as usize;
            let next = match &mut regs[iter] {
                Value::Iter(state) => state.advance(),
                other => return Err(VmError::TypeMismatch(format!("iter_next on {:?}", other))),
            };
            match next {
                Some(v) => {
                    regs[value] = v;
                    *pc = body;
                }
                None => *pc = exit,
            }
        }
        Opcode::Return => {
            let src = read_u32(&bc.code, pc) as usize;
            return Ok(StepResult::Return(regs[src].clone()));
        }
        Opcode::Await => {
            let dst = read_u32(&bc.code, pc) as usize;
            let src = read_u32(&bc.code, pc) as usize;
            match &regs[src] {
                Value::Future(kind, args) => {
                    return Ok(StepResult::Suspend {
                        dst,
                        kind: *kind,
                        args: args.clone(),
                    });
                }
                other => {
                    return Err(VmError::TypeMismatch(format!("await on {:?}", other)));
                }
            }
        }
    }
    Ok(StepResult::Continue)
}

fn read_u32(code: &[u8], pc: &mut usize) -> u32 {
    let bytes: [u8; 4] = code[*pc..*pc + 4].try_into().expect("short read");
    *pc += 4;
    u32::from_le_bytes(bytes)
}

fn const_ident(bc: &Bytecode, idx: usize) -> Result<&str, VmError> {
    match &bc.consts[idx] {
        Const::Ident(s) | Const::String(s) => Ok(s.as_str()),
        _ => Err(VmError::TypeMismatch("const idx not Ident".into())),
    }
}

fn field_access(base: &Value, field: &str) -> Result<Value, VmError> {
    match base {
        Value::Struct(fields) => fields
            .get(field)
            .cloned()
            .ok_or_else(|| VmError::UnknownField(field.to_string())),
        Value::Variant { args, .. } if args.len() == 1 => {
            // Single-arg variants behave like tuple structs: field access
            // delegates to the inner value (e.g. `event.attributes`).
            field_access(&args[0], field)
        }
        other => Err(VmError::TypeMismatch(format!(
            "field access `.{}` on {:?}",
            field, other
        ))),
    }
}

fn eval_binop(op: HirBinOp, lhs: &Value, rhs: &Value) -> Result<Value, VmError> {
    use HirBinOp::*;
    match (op, lhs, rhs) {
        (Add, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Sub, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
        (Mul, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
        (Div, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
        (Mod, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
        (Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
        (Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
        (Div, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
        (Eq, a, b) => Ok(Value::Bool(a == b)),
        (Ne, a, b) => Ok(Value::Bool(a != b)),
        (Lt, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
        (Le, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
        (Gt, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
        (Ge, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
        (Lt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
        (Le, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
        (Gt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
        (Ge, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
        (op, a, b) => Err(VmError::TypeMismatch(format!(
            "binop {:?} on {:?}, {:?}",
            op, a, b
        ))),
    }
}

fn normalize_unit_literal(
    value: &str,
    unit: crate::automations::repr::ast::UnitType,
) -> Result<Value, VmError> {
    use crate::automations::repr::ast::UnitType;
    let scale_ms: Option<f64> = match unit {
        UnitType::Seconds => Some(1000.0),
        UnitType::Minutes => Some(60_000.0),
        UnitType::Hours => Some(3_600_000.0),
        UnitType::Days => Some(86_400_000.0),
        // Non-duration units pass through unscaled for now.
        UnitType::Degrees
        | UnitType::Radians
        | UnitType::Celsius
        | UnitType::Fahrenheit
        | UnitType::Kelvin => None,
    };
    if let Some(scale) = scale_ms {
        let raw: f64 = value
            .parse::<f64>()
            .map_err(|_| VmError::TypeMismatch(format!("bad duration literal {}", value)))?;
        let ms = raw * scale;
        // Prefer Int when the result is whole.
        if ms.fract() == 0.0 {
            Ok(Value::Int(ms as i64))
        } else {
            Ok(Value::Float(ms))
        }
    } else if let Ok(n) = value.parse::<i64>() {
        Ok(Value::Int(n))
    } else if let Ok(n) = value.parse::<f64>() {
        Ok(Value::Float(n))
    } else {
        Err(VmError::TypeMismatch(format!("bad unit literal {}", value)))
    }
}

fn call_builtin(name: &str, args: Vec<Value>) -> Result<Value, VmError> {
    match name {
        "len" => match args.as_slice() {
            [Value::List(items)] => Ok(Value::Int(items.len() as i64)),
            [Value::String(s)] => Ok(Value::Int(s.len() as i64)),
            other => Err(VmError::TypeMismatch(format!("len({:?})", other))),
        },
        "abs" => match args.as_slice() {
            [Value::Int(n)] => Ok(Value::Int(n.abs())),
            [Value::Float(n)] => Ok(Value::Float(n.abs())),
            other => Err(VmError::TypeMismatch(format!("abs({:?})", other))),
        },
        // Async builtins return a `Future` value; the subsequent `Await`
        // opcode drives them. `sleep_unique` looks identical here — the
        // re-trigger cancellation contract is enforced by the runner via
        // task abort, not by the VM.
        "sleep" => Ok(Value::Future(FutureKind::Sleep, args)),
        "sleep_unique" => Ok(Value::Future(FutureKind::SleepUnique, args)),
        other => Err(VmError::UnknownBuiltin(other.to_string())),
    }
}
