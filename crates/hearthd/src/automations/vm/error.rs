//! Failures the VM reports, split by who is at fault.

use crate::automations::repr::function::FunctionIdentity;

/// An error returned by the VM.
///
/// The variants are split by who is at fault, so a failure never leaves it
/// ambiguous whether the automation did something the language permits or
/// something upstream is broken:
///
/// - [`VmError::DivideByZero`] and [`VmError::Overflow`] are the automation's
///   own doing. Well-typed source can produce them and the runner should
///   report them against the automation.
/// - [`VmError::NotImplemented`] is this VM lagging the checker: the
///   function exists in the language but this VM does not implement it.
/// - [`VmError::InvariantViolation`] is a bug somewhere above the VM. It
///   should be unreachable, and reaching it means the compiler emitted
///   bytecode the checker should have rejected, or the runner bound a
///   parameter whose value contradicts its declared type.
#[derive(Debug, Clone)]
pub enum VmError {
    /// Integer division or remainder by zero. Float division by zero is
    /// not an error — it follows IEEE 754 and yields an infinity or NaN.
    DivideByZero,
    /// An integer operation left the `i64` range. Reported rather than
    /// panicking: filter source is user-authored, and a daemon must not
    /// abort because an automation overflowed.
    Overflow(String),
    /// A function the language defines that this VM does not implement.
    ///
    /// Only `keys` and `values` today, both of which need a `Map`
    /// representation no `Value` has yet. Carries the resolved identity
    /// rather than a name: calls are early-bound, so the VM cannot fail to
    /// resolve one. This is only ever the VM lagging the checker, never the
    /// automation author's fault.
    NotImplemented(FunctionIdentity),
    /// A condition the compiler and runner are supposed to make impossible:
    /// a register holding a type no opcode should have put there, a constant
    /// pool slot of the wrong kind, an undecodable opcode or tag, or an
    /// `Await` in a function the checker promised could not suspend.
    ///
    /// Never the automation author's fault. Treat it as a defect report.
    InvariantViolation(String),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::DivideByZero => write!(f, "divide by zero"),
            VmError::Overflow(s) => write!(f, "integer overflow: {}", s),
            VmError::NotImplemented(function) => write!(f, "{} is not implemented", function),
            VmError::InvariantViolation(s) => write!(f, "VM invariant violated: {}", s),
        }
    }
}

impl std::error::Error for VmError {}
