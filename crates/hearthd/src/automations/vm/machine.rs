//! The register machine itself: operand decoding and instruction dispatch.

use std::sync::Arc;

use super::consts::VmConst;
use super::error::VmError;
use super::ops::call;
use super::ops::eval_binop;
use super::ops::field_access;
use super::suspension::Suspension;
use super::value::IterState;
use super::value::Pending;
use super::value::Value;
use crate::automations::repr::bytecode::*;
use crate::automations::repr::function::FunctionIdentity;
use crate::automations::repr::hir::HirBinOp;

/// Where [`Vm::poll`] stopped.
///
/// There is deliberately no "kept going" state: ordinary instructions never
/// leave [`Vm::poll`], so this is only ever the two outcomes a driver has to
/// make a decision about.
enum VmPoll {
    /// The function returned this value.
    Ready(Value),
    /// The function reached an `Await`. The operands are left undecoded at
    /// the program counter; only a driver that can suspend reads them.
    Awaiting,
}

/// One compiled function in the form opcodes read it: the blueprint a run is
/// instantiated from.
///
/// Everything here is fixed at construction and read-only during execution,
/// which is what lets concurrent runs of one function share a single copy.
#[derive(Debug)]
pub struct Program {
    /// The instruction stream. Boxed rather than borrowed so the VM outlives
    /// whatever compiled it, and boxed rather than a `Vec` because it is
    /// fixed at construction and never appended to.
    code: Box<[u8]>,
    /// The constant pool this stream indexes into, decoded at construction.
    consts: Box<[VmConst]>,
    /// Register each positional parameter binds into, in parameter order.
    ///
    /// This is all the VM needs from the bytecode's parameter list; the
    /// declared names and types are compile-time metadata that no opcode
    /// reads, so they are dropped rather than carried around at runtime.
    param_regs: Box<[u32]>,
    /// How many registers the function declared. A blueprint has no register
    /// file to read this back off, and [`Program::instance`] has to size one.
    num_regs: usize,
}

/// One run of a [`Program`]: a register machine over a shared blueprint.
///
/// Take one with [`Program::instance`]. A filter's is rebound and rerun per
/// event by [`Vm::run_sync`]; a body's is consumed by [`Vm::run_async`],
/// because a function that suspends owns its registers until it finishes.
///
/// Deliberately not `Clone`: a clone would copy whatever the register file
/// happens to hold mid-run. Taking another `instance` is what makes sense.
#[derive(Debug)]
pub struct Vm {
    /// Shared with every other run of the same function.
    program: Arc<Program>,
    /// Scratch slots, one per register the function declared. Allocated once
    /// at construction and reset in place between runs.
    regs: Vec<Value>,
    /// Byte offset of the next instruction in `program.code`.
    pc: usize,
}

impl Program {
    /// Compile `bytecode` into a blueprint, taking ownership of its code and
    /// constants.
    ///
    /// Fails if a unit literal does not fit its dimension's canonical unit.
    /// Decoding the constant pool up front is what lets that surface when an
    /// automation is deployed rather than on the event that first reaches
    /// the literal.
    pub fn new(bytecode: Bytecode) -> Result<Self, VmError> {
        let Bytecode {
            params,
            num_regs,
            consts,
            code,
        } = bytecode;

        let consts = consts
            .into_iter()
            .map(VmConst::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Program {
            code: code.into_boxed_slice(),
            consts: consts.into_boxed_slice(),
            param_regs: params.iter().map(|p| p.reg).collect(),
            num_regs: num_regs as usize,
        })
    }

    /// One run of this program: its own register file and program counter
    /// over the shared blueprint.
    ///
    /// This is how one compiled body runs several times at once. The runs
    /// share no state, so a firing that arrives while an older body is still
    /// suspended runs beside it.
    pub fn instance(self: &Arc<Self>) -> Vm {
        Vm {
            program: Arc::clone(self),
            regs: vec![Value::Unit; self.num_regs],
            pc: 0,
        }
    }
}

impl Vm {
    /// Run to completion synchronously, returning the value passed to
    /// `RETURN`. An `Await` is an error: filters must not suspend.
    ///
    /// # Panics
    ///
    /// Panics if `params` does not match the function's parameter count.
    /// Parameters are supplied by the runner, not by automation source, so a
    /// mismatch is a caller bug rather than something a filter can provoke.
    pub fn run_sync(&mut self, params: Vec<Value>) -> Result<Value, VmError> {
        self.bind(params);
        match self.poll()? {
            VmPoll::Ready(value) => Ok(value),
            VmPoll::Awaiting => Err(VmError::InvariantViolation(
                "await in a function the checker promised cannot suspend".into(),
            )),
        }
    }

    /// Run to completion, suspending the calling task at each `Await`.
    ///
    /// Consumes the `Vm` rather than borrowing it: a body that suspends owns
    /// its program counter and register file until it finishes, so it cannot
    /// be rebound the way a filter's is. A caller firing the same body again
    /// takes another [`Program::instance`], and the two run side by side.
    ///
    /// `suspension` serves the waits the body suspends on, and is the only
    /// say in whether a `sleep_unique` was superseded.
    ///
    /// # Panics
    ///
    /// Panics if `params` does not match the function's parameter count,
    /// as [`Vm::run_sync`] does.
    pub async fn run_async(
        mut self,
        params: Vec<Value>,
        suspension: &dyn Suspension,
    ) -> Result<Value, VmError> {
        self.bind(params);
        loop {
            match self.poll()? {
                VmPoll::Ready(value) => return Ok(value),
                VmPoll::Awaiting => {
                    // `poll` stops with the operands still at the program
                    // counter, so reading them is this driver's job.
                    let dst = self.read_index();
                    let src = self.read_index();
                    let pending = match &self.regs[src] {
                        Value::Future(pending) => *pending,
                        // The checker types `await` as taking a `Future`,
                        // so a register holding anything else is a broken
                        // compiler rather than a bad automation.
                        other => {
                            return Err(VmError::InvariantViolation(format!("await on {}", other)));
                        }
                    };
                    // The one place a suspension becomes a register value,
                    // so the type each builtin resolves to is fixed here
                    // rather than in every [`Suspension`].
                    let duration = pending.duration();
                    self.regs[dst] = match pending {
                        Pending::Sleep(_) => {
                            suspension.sleep(duration).await;
                            Value::Unit
                        }
                        Pending::SleepUnique(_) => {
                            Value::Bool(suspension.sleep_unique(duration).await)
                        }
                    };
                }
            }
        }
    }

    /// Reset execution state and load a fresh set of parameters.
    ///
    /// Every register is reset to `Unit` first. That is for correctness, not
    /// tidiness: without it a register this run never writes would still
    /// hold whatever the previous run left there.
    fn bind(&mut self, params: Vec<Value>) {
        assert_eq!(
            params.len(),
            self.program.param_regs.len(),
            "param count mismatch: expected {}, got {}",
            self.program.param_regs.len(),
            params.len(),
        );

        self.pc = 0;
        self.regs.fill(Value::Unit);
        for (reg, value) in self.program.param_regs.iter().zip(params) {
            self.regs[*reg as usize] = value;
        }
    }

    // ------------------------------------------------------------------
    // Operand decoding
    // ------------------------------------------------------------------

    /// Read the next byte and advance past it.
    ///
    /// # Panics
    ///
    /// Panics if the stream ends mid-instruction. Unlike an undecodable
    /// opcode, a truncated operand stream is not reported as an
    /// [`VmError::InvariantViolation`]: the code is produced in-process by
    /// the compiler and never parsed from an external source, so a short
    /// read is a broken compiler rather than anything a filter can provoke.
    fn read_u8(&mut self) -> u8 {
        let byte = self.program.code[self.pc];
        self.pc += 1;
        byte
    }

    /// Read the next little-endian `u32` operand and advance past it.
    ///
    /// # Panics
    ///
    /// Panics on a truncated stream, as [`Vm::read_u8`] does. The slice is
    /// always four bytes wide, so the `try_into` cannot fail once the
    /// indexing has succeeded.
    fn read_u32(&mut self) -> u32 {
        let bytes: [u8; 4] = self.program.code[self.pc..self.pc + 4]
            .try_into()
            .expect("a four-byte slice is always a [u8; 4]");
        self.pc += 4;
        u32::from_le_bytes(bytes)
    }

    /// Read a `u32` operand used as an index: a register, a constant-pool
    /// slot, or an absolute jump target.
    fn read_index(&mut self) -> usize {
        self.read_u32() as usize
    }

    /// Resolve a constant-pool slot expected to hold an identifier.
    ///
    /// Only `Ident` is accepted. Every index reaching here — a field name,
    /// enum name, variant name or struct name — is interned by the encoder
    /// through `intern_ident`, and the ident and string pools are keyed
    /// separately, so a string literal never lands in one of these slots.
    fn const_ident(&self, idx: usize) -> Result<&str, VmError> {
        match &self.program.consts[idx] {
            VmConst::Ident(s) => Ok(s.as_str()),
            _ => Err(VmError::InvariantViolation("const idx not Ident".into())),
        }
    }

    // ------------------------------------------------------------------
    // Execution
    // ------------------------------------------------------------------

    /// Execute instructions until the function returns or reaches an
    /// `Await`.
    ///
    /// Named for what it does to a `Future`: advance to the next suspension
    /// point and hand control back. The dispatch loop lives in here rather
    /// than in each driver, so the drivers differ only where they genuinely
    /// differ — at the `Await` — and an ordinary instruction never crosses a
    /// function boundary to say it made progress.
    fn poll(&mut self) -> Result<VmPoll, VmError> {
        loop {
            let opcode_byte = self.read_u8();
            let opcode = Opcode::from_repr(opcode_byte).ok_or(VmError::InvariantViolation(
                format!("undecodable opcode 0x{:02x}", opcode_byte),
            ))?;
            match opcode {
                Opcode::LoadConstInt => {
                    let dst = self.read_index();
                    let idx = self.read_index();
                    self.regs[dst] = match &self.program.consts[idx] {
                        VmConst::Int(n) => Value::Int(*n),
                        _ => return Err(VmError::InvariantViolation("const idx not Int".into())),
                    };
                }
                Opcode::LoadConstNode => {
                    let dst = self.read_index();
                    let idx = self.read_index();
                    self.regs[dst] = match &self.program.consts[idx] {
                        VmConst::Node(id) => Value::Node(*id),
                        _ => return Err(VmError::InvariantViolation("const idx not Node".into())),
                    };
                }
                Opcode::LoadConstFloat => {
                    let dst = self.read_index();
                    let idx = self.read_index();
                    self.regs[dst] = match &self.program.consts[idx] {
                        VmConst::Float(n) => Value::Float(*n),
                        _ => return Err(VmError::InvariantViolation("const idx not Float".into())),
                    };
                }
                Opcode::LoadConstString => {
                    let dst = self.read_index();
                    let idx = self.read_index();
                    self.regs[dst] = match &self.program.consts[idx] {
                        VmConst::String(s) => Value::String(s.clone()),
                        _ => {
                            return Err(VmError::InvariantViolation("const idx not String".into()));
                        }
                    };
                }
                Opcode::LoadConstBool => {
                    let dst = self.read_index();
                    let value = self.read_u8() != 0;
                    self.regs[dst] = Value::Bool(value);
                }
                Opcode::LoadConstUnit => {
                    // Already normalised to the dimension's canonical unit by
                    // `VmConst::try_from`, so two literals of one dimension are
                    // directly comparable however they were spelled. The
                    // dimension travels with the value: without it `1h` and
                    // `3600` would compare equal.
                    let dst = self.read_index();
                    let idx = self.read_index();
                    self.regs[dst] = match &self.program.consts[idx] {
                        VmConst::Quantity(q) => Value::Quantity(*q),
                        _ => {
                            return Err(VmError::InvariantViolation(
                                "const idx not UnitLit".into(),
                            ));
                        }
                    };
                }
                Opcode::Unit => {
                    let dst = self.read_index();
                    self.regs[dst] = Value::Unit;
                }
                Opcode::BinOp => {
                    let dst = self.read_index();
                    let tag = BinOpTag::from_repr(self.read_u8())
                        .ok_or(VmError::InvariantViolation("bad binop tag".into()))?;
                    let lhs = self.read_index();
                    let rhs = self.read_index();
                    let value = eval_binop(HirBinOp::from(tag), &self.regs[lhs], &self.regs[rhs])?;
                    self.regs[dst] = value;
                }
                Opcode::Neg => {
                    let dst = self.read_index();
                    let src = self.read_index();
                    let value = match &self.regs[src] {
                        // `-i64::MIN` is not representable; checked so it cannot
                        // panic on user-authored input.
                        Value::Int(n) => Value::Int(
                            n.checked_neg()
                                .ok_or_else(|| VmError::Overflow(format!("neg {}", n)))?,
                        ),
                        Value::Float(n) => Value::Float(-n),
                        other => {
                            return Err(VmError::InvariantViolation(format!("neg on {:?}", other)));
                        }
                    };
                    self.regs[dst] = value;
                }
                Opcode::Not => {
                    let dst = self.read_index();
                    let src = self.read_index();
                    let value = match &self.regs[src] {
                        Value::Bool(b) => Value::Bool(!b),
                        other => {
                            return Err(VmError::InvariantViolation(format!("not on {:?}", other)));
                        }
                    };
                    self.regs[dst] = value;
                }
                Opcode::Deref => {
                    // The DSL's `*` operator is currently a no-op at runtime;
                    // the checker uses it for typing, but values flow through
                    // unchanged.
                    let dst = self.read_index();
                    let src = self.read_index();
                    self.regs[dst] = self.regs[src].clone();
                }
                Opcode::Field | Opcode::OptionalField => {
                    // `OptionalField` behaves identically for now: `Value` has
                    // no `Option` representation for it to produce.
                    let dst = self.read_index();
                    let base = self.read_index();
                    let idx = self.read_index();
                    let value = field_access(&self.regs[base], self.const_ident(idx)?)?;
                    self.regs[dst] = value;
                }
                Opcode::Call => {
                    let dst = self.read_index();
                    let tag = FunctionTag::from_repr(self.read_u8()).ok_or_else(|| {
                        VmError::InvariantViolation("undecodable function tag".into())
                    })?;
                    let args = self.read_args();
                    let value = call(FunctionIdentity::from(tag), args)?;
                    self.regs[dst] = value;
                }
                Opcode::Variant => {
                    let dst = self.read_index();
                    let enum_idx = self.read_index();
                    let variant_idx = self.read_index();
                    let args = self.read_args();
                    let value = Value::Variant {
                        enum_name: self.const_ident(enum_idx)?.to_string(),
                        variant: self.const_ident(variant_idx)?.to_string(),
                        args,
                    };
                    self.regs[dst] = value;
                }
                Opcode::EmptyList => {
                    let dst = self.read_index();
                    self.regs[dst] = Value::List(Vec::new());
                }
                Opcode::List => {
                    let dst = self.read_index();
                    let elems = self.read_args();
                    self.regs[dst] = Value::List(elems);
                }
                Opcode::ListPush => {
                    let list = self.read_index();
                    let value = self.read_index();
                    let item = self.regs[value].clone();
                    match &mut self.regs[list] {
                        Value::List(items) => items.push(item),
                        other => {
                            return Err(VmError::InvariantViolation(format!(
                                "list_push on {:?}",
                                other
                            )));
                        }
                    }
                }
                Opcode::IterInit => {
                    let dst = self.read_index();
                    let src = self.read_index();
                    let list = match &self.regs[src] {
                        Value::List(items) => items.clone(),
                        other => {
                            return Err(VmError::InvariantViolation(format!(
                                "iter_init on {:?}",
                                other
                            )));
                        }
                    };
                    self.regs[dst] = Value::Iter(IterState::from(list));
                }
                Opcode::Struct => {
                    let dst = self.read_index();
                    let _name_idx = self.read_index();
                    let count = self.read_index();
                    let mut fields: std::collections::BTreeMap<String, Value> =
                        std::collections::BTreeMap::new();
                    for _ in 0..count {
                        let tag = StructFieldTag::from_repr(self.read_u8())
                            .ok_or(VmError::InvariantViolation("bad struct field tag".into()))?;
                        match tag {
                            StructFieldTag::Set => {
                                let field_idx = self.read_index();
                                let src = self.read_index();
                                let name = self.const_ident(field_idx)?.to_string();
                                fields.insert(name, self.regs[src].clone());
                            }
                            StructFieldTag::Spread => {
                                // Explicit fields already in the map win; a
                                // spread only supplies what is still missing.
                                let src = self.read_index();
                                match &self.regs[src] {
                                    Value::Struct(src_fields) => {
                                        for (name, value) in src_fields {
                                            fields
                                                .entry(name.clone())
                                                .or_insert_with(|| value.clone());
                                        }
                                    }
                                    other => {
                                        return Err(VmError::InvariantViolation(format!(
                                            "struct spread on {:?}",
                                            other
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    self.regs[dst] = Value::Struct(fields);
                }
                Opcode::Copy => {
                    let dst = self.read_index();
                    let src = self.read_index();
                    self.regs[dst] = self.regs[src].clone();
                }
                Opcode::Jump => {
                    self.pc = self.read_index();
                }
                Opcode::JumpIf => {
                    let cond = self.read_index();
                    let then_target = self.read_index();
                    let else_target = self.read_index();
                    let take_then = match &self.regs[cond] {
                        Value::Bool(b) => *b,
                        other => {
                            return Err(VmError::InvariantViolation(format!(
                                "jump_if on {:?}",
                                other
                            )));
                        }
                    };
                    self.pc = if take_then { then_target } else { else_target };
                }
                Opcode::IterNext => {
                    let iter = self.read_index();
                    let binding = self.read_index();
                    let body_target = self.read_index();
                    let exit_target = self.read_index();
                    let next = match &mut self.regs[iter] {
                        Value::Iter(state) => state.next(),
                        other => {
                            return Err(VmError::InvariantViolation(format!(
                                "iter_next on {:?}",
                                other
                            )));
                        }
                    };
                    match next {
                        Some(value) => {
                            self.regs[binding] = value;
                            self.pc = body_target;
                        }
                        None => self.pc = exit_target,
                    }
                }
                Opcode::Return => {
                    let src = self.read_index();
                    return Ok(VmPoll::Ready(self.regs[src].clone()));
                }
                Opcode::Await => return Ok(VmPoll::Awaiting),
            }
        }
    }

    /// Decode a length-prefixed run of register operands and clone the value
    /// out of each. Shared by `Call`, `Variant` and `List`.
    fn read_args(&mut self) -> Vec<Value> {
        let count = self.read_index();
        let mut args = Vec::with_capacity(count);
        for _ in 0..count {
            let reg = self.read_index();
            args.push(self.regs[reg].clone());
        }
        args
    }
}
