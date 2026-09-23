//! Operations over [`Value`]s alone.
//!
//! None of these touch VM state, so they stay free functions: they are
//! independently testable and cannot reach into the register file.

use super::error::VmError;
use super::quantity::Quantity;
use super::value::Pending;
use super::value::Value;
use crate::automations::repr::function::FunctionIdentity;

pub(super) fn field_access(base: &Value, field: &str) -> Result<Value, VmError> {
    match base {
        Value::Struct(fields) => fields
            .get(field)
            .cloned()
            .ok_or_else(|| VmError::InvariantViolation(format!("unknown field `{}`", field))),
        Value::Variant { args, .. } if args.len() == 1 => {
            // Single-arg variants behave like tuple structs: field access
            // delegates to the inner value (e.g. `event.attributes`).
            field_access(&args[0], field)
        }
        other => Err(VmError::InvariantViolation(format!(
            "field access `.{}` on {:?}",
            field, other
        ))),
    }
}

/// Wrap a `checked_*` result, naming the operation and operands in the
/// error so a failing filter is diagnosable from the message alone.
fn checked_int(result: Option<i64>, op: &str, a: i64, b: i64) -> Result<Value, VmError> {
    result
        .map(Value::Int)
        .ok_or_else(|| VmError::Overflow(format!("{} {} {}", a, op, b)))
}

/// Equality with the checker's numeric promotion applied, so `1 == 1.0`
/// holds. Everything else compares structurally.
///
/// The promotion deliberately does not extend to [`Value::Quantity`]: a
/// quantity carries a dimension, so `1h == 3600` is false rather than
/// comparing a magnitude against a bare number. Quantities of the same
/// dimension compare within a tolerance — see [`Quantity`].
///
/// Promoting through `f64` loses precision for integers beyond 2^53, which
/// is the usual cost of a language where a `Float` contaminates a
/// comparison.
///
/// A `Struct` or `Variant` on either side is reported rather than
/// answered. `Value::Struct` is a bag of fields with the type name
/// discarded at construction, so comparing one structurally would let two
/// distinct named types with the same shape hold.
///
/// The checker rejects equality on every named type, but not yet on every
/// path to one: `Event` field access is deferred and types as `Ty::Error`,
/// which short-circuits the equality check. So
/// `event.attributes == event.attributes` reaches here today. The
/// `InvariantViolation` is the right answer — the defect is that checker
/// gap, not the automation — but this is not yet the unreachable assertion
/// it becomes once `Event` field access is typed.
pub(super) fn values_equal(lhs: &Value, rhs: &Value) -> Result<bool, VmError> {
    match (lhs, rhs) {
        (Value::Int(a), Value::Float(b)) => Ok((*a as f64) == *b),
        (Value::Float(a), Value::Int(b)) => Ok(*a == (*b as f64)),

        // The checker rejects equality on `Future`. Reaching here would
        // answer a meaningless question: two futures standing for the same
        // wait are still distinct suspensions.
        (Value::Future(_), _) | (_, Value::Future(_)) => Err(VmError::InvariantViolation(
            "equality on an unawaited future".into(),
        )),

        (Value::Struct(_) | Value::Variant { .. }, _)
        | (_, Value::Struct(_) | Value::Variant { .. }) => {
            Err(VmError::InvariantViolation(format!(
                "equality on a value carrying no identity: {} and {}",
                lhs, rhs
            )))
        }

        // Lists recurse rather than deferring to `PartialEq`, so the
        // promotion reaches nested values too. Without this `1 == 1.0`
        // would hold while `[1] == [1.0]` did not, making equality depend
        // on how deeply the numbers are buried.
        (Value::List(a), Value::List(b)) => {
            if a.len() != b.len() {
                return Ok(false);
            }
            for (a, b) in std::iter::zip(a, b) {
                if !values_equal(a, b)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }

        _ => Ok(lhs == rhs),
    }
}

/// Membership. The checker accepts a `List`, `Set` or `Map` on the right,
/// but only `List` is representable as a `Value` today.
///
/// Comparison goes through [`values_equal`], so membership inherits both
/// the numeric promotion and the refusal to compare values carrying no
/// identity.
pub(super) fn eval_in(needle: &Value, haystack: &Value) -> Result<Value, VmError> {
    match haystack {
        Value::List(items) => {
            for item in items {
                if values_equal(item, needle)? {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        other => Err(VmError::InvariantViolation(format!("in on {:?}", other))),
    }
}

/// Read a numeric `Value` as `f64`. `None` for anything non-numeric.
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Int(n) => Some(*n as f64),
        Value::Float(n) => Some(*n),
        _ => None,
    }
}

pub(super) fn add_int(a: i64, b: i64) -> Result<Value, VmError> {
    checked_int(a.checked_add(b), "add", a, b)
}

pub(super) fn sub_int(a: i64, b: i64) -> Result<Value, VmError> {
    checked_int(a.checked_sub(b), "sub", a, b)
}

pub(super) fn mul_int(a: i64, b: i64) -> Result<Value, VmError> {
    checked_int(a.checked_mul(b), "mul", a, b)
}

pub(super) fn div_int(a: i64, b: i64) -> Result<Value, VmError> {
    if b == 0 {
        return Err(VmError::DivideByZero);
    }
    checked_int(a.checked_div(b), "div", a, b)
}

pub(super) fn mod_int(a: i64, b: i64) -> Result<Value, VmError> {
    if b == 0 {
        return Err(VmError::DivideByZero);
    }
    // `wrapping_rem` rather than `checked_rem`: the only pair
    // `checked_rem` rejects is `i64::MIN % -1`, where the true remainder
    // is 0 and representable. It is `checked_div` that genuinely
    // overflows on that pair, so only division reports it.
    Ok(Value::Int(a.wrapping_rem(b)))
}

/// Dispatch a resolved call.
///
/// The callee is a [`FunctionIdentity`], not a name, so there is no
/// resolution to fail — every function the language defines is either
/// handled here or explicitly reported as unimplemented, and the match is
/// exhaustive so adding one cannot be forgotten.
pub(super) fn call(function: FunctionIdentity, args: Vec<Value>) -> Result<Value, VmError> {
    /// The checker validates arity and argument types, so an argument list
    /// that does not match is a broken compiler rather than bad automation
    /// source.
    fn bad_args(function: FunctionIdentity, args: &[Value]) -> VmError {
        VmError::InvariantViolation(format!("{}{:?}", function, args))
    }

    match function {
        FunctionIdentity::Len => match args.as_slice() {
            [Value::List(items)] => Ok(Value::Int(items.len() as i64)),
            // Characters, not bytes: `len` is documented as a count, and
            // an automation comparing `len(node.name)` against a number
            // means the length the author can see, not its UTF-8 encoding.
            [Value::String(s)] => Ok(Value::Int(s.chars().count() as i64)),
            other => Err(bad_args(function, other)),
        },
        FunctionIdentity::Abs => match args.as_slice() {
            // `i64::MIN.abs()` is not representable; checked like `Neg`.
            [Value::Int(n)] => Ok(Value::Int(
                n.checked_abs()
                    .ok_or_else(|| VmError::Overflow(format!("abs {}", n)))?,
            )),
            [Value::Float(n)] => Ok(Value::Float(n.abs())),
            other => Err(bad_args(function, other)),
        },
        // `min`/`max`/`clamp` follow the same promotion rule as the binary
        // operators: an all-`Int` call stays exact, anything with a `Float`
        // yields a `Float`.
        FunctionIdentity::Min | FunctionIdentity::Max => {
            let take_min = function == FunctionIdentity::Min;
            match args.as_slice() {
                [Value::Int(a), Value::Int(b)] => Ok(Value::Int(if take_min {
                    (*a).min(*b)
                } else {
                    (*a).max(*b)
                })),
                [a, b] => match (as_f64(a), as_f64(b)) {
                    (Some(a), Some(b)) => {
                        Ok(Value::Float(if take_min { a.min(b) } else { a.max(b) }))
                    }
                    _ => Err(bad_args(function, args.as_slice())),
                },
                other => Err(bad_args(function, other)),
            }
        }
        FunctionIdentity::Clamp => match args.as_slice() {
            [Value::Int(value), Value::Int(a), Value::Int(b)] => {
                let (low, high) = if a <= b { (a, b) } else { (b, a) };
                Ok(Value::Int((*value).clamp(*low, *high)))
            }
            [value, a, b] => match (as_f64(value), as_f64(a), as_f64(b)) {
                (Some(value), Some(a), Some(b)) => Ok(Value::Float(clamp_f64(value, a, b))),
                _ => Err(bad_args(function, args.as_slice())),
            },
            other => Err(bad_args(function, other)),
        },
        // `Value` has no `Map`, and nothing produces one until the runner
        // projects engine state, so there is nothing to take keys or values
        // of yet.
        FunctionIdentity::Keys | FunctionIdentity::Values => Err(VmError::NotImplemented(function)),
        // A future is constructed strictly, like any other call result, and
        // is opaque once built: only the async driver's `Await` may look at
        // one. The synchronous driver builds them and never actualises one,
        // because the checker keeps `await` out of a filter.
        // Only a duration is accepted; the checker types both parameters
        // that way.
        FunctionIdentity::Sleep | FunctionIdentity::SleepUnique => {
            let unique = function == FunctionIdentity::SleepUnique;
            match args.as_slice() {
                [Value::Quantity(Quantity::Duration(ns))] => Ok(Value::Future(if unique {
                    Pending::SleepUnique(*ns)
                } else {
                    Pending::Sleep(*ns)
                })),
                other => Err(bad_args(function, other)),
            }
        }
    }
}

/// `clamp` over floats, total for every input.
///
/// Deliberately not `f64::clamp`, which panics on inverted or NaN bounds.
/// Automation source supplies these bounds, so every combination needs a
/// defined answer instead: the bounds are used in whichever order puts the
/// smaller first, a NaN bound constrains nothing, and a NaN value passes
/// through because it compares false against everything.
fn clamp_f64(value: f64, a: f64, b: f64) -> f64 {
    let (low, high) = match (a.is_nan(), b.is_nan()) {
        (true, true) => return value,
        (true, false) => (f64::NEG_INFINITY, b),
        (false, true) => (a, f64::INFINITY),
        (false, false) if a <= b => (a, b),
        (false, false) => (b, a),
    };

    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}
