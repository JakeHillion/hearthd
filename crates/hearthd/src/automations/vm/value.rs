//! Runtime values handled by the bytecode VM.
//!
//! `Value` is the dynamic representation that VM registers hold. It mirrors
//! the type system the checker enforces and otherwise carries none of the
//! static type information at runtime — the bytecode has already
//! type-checked. [`Quantity`] is the one exception: its variant names the
//! dimension, because the checker types `==` as `Bool` for every operand
//! pair, so the VM is the only thing that can keep `1h` from equalling
//! `3600` or `90deg`.
//!
//! Domain-specific cluster snapshots (`OnOffCluster`, `OccupancySensingCluster`,
//! …) and engine `Node` references are deliberately not modeled yet; they
//! arrive once the runner starts feeding real engine state to the VM in
//! later commits.

use super::quantity::Quantity;

/// One register's worth of runtime value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The unit / void value (e.g. the result of a statement-expression).
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),

    /// A list. Used for observer return values and intermediate
    /// collections during list comprehensions.
    List(Vec<Value>),

    /// An iterator over a list, with a cursor.
    Iter(IterState),

    /// An enum variant (e.g. `Event::OccupancySensingChanged { … }`),
    /// carrying its constructor arguments.
    Variant {
        enum_name: String,
        variant: String,
        args: Vec<Value>,
    },

    /// An anonymous record/struct, e.g. a cluster snapshot or the
    /// destructured `state.lights` group. Fields are looked up by name.
    Struct(std::collections::BTreeMap<String, Value>),

    /// A unit literal (`5min`, `20c`) normalised to its dimension's
    /// canonical unit: nanoseconds, tenths of a degree, or hundredths of a
    /// degree Celsius.
    ///
    /// Normalising at decode is what makes `1h == 60min` hold; keeping the
    /// dimension is what keeps `1h == 90deg` and `1h == 3600` false instead
    /// of comparing bare magnitudes.
    Quantity(Quantity),

    /// An unawaited future, as produced by `sleep` / `sleep_unique`.
    ///
    /// Deliberately opaque and payload-free. The checker rejects equality
    /// on `Future` and rejects `await` in a filter, so nothing the
    /// synchronous VM can reach inspects one: a filter may construct a
    /// future and discard it, never observe it. The async driver needs the
    /// duration in order to suspend, and is what gives this a payload.
    Future,
}

/// A list iterator.
///
/// A newtype over [`std::vec::IntoIter`] rather than a list and a cursor, so
/// [`Iterator::next`] moves each element out instead of cloning it. The
/// wrapper exists only to supply `PartialEq`, which `Value` derives and
/// `IntoIter` does not implement.
#[derive(Debug, Clone)]
pub struct IterState(std::vec::IntoIter<Value>);

/// Two iterators are equal when the elements they have yet to yield are.
///
/// Nothing reaches this from automation source — the language has no
/// iterator type, so a `Value::Iter` exists only between an `IterInit` and
/// its `IterNext`s, and can never be compared or returned. It is here to
/// satisfy `Value`'s derive.
impl PartialEq for IterState {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_slice() == other.0.as_slice()
    }
}

impl From<Vec<Value>> for IterState {
    fn from(list: Vec<Value>) -> Self {
        Self(list.into_iter())
    }
}

impl Iterator for IterState {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        self.0.next()
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Unit => f.write_str("()"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            // `{:?}` so a whole float keeps its decimal point (`2.0`, not
            // `2`) and stays distinguishable from an `Int`.
            Value::Float(n) => write!(f, "{:?}", n),
            // Quoted and escaped, so padding or emptiness stays visible.
            Value::String(s) => write!(f, "{:?}", s),
            Value::List(items) => {
                f.write_str("[")?;
                write_comma_separated(f, items)?;
                f.write_str("]")
            }
            // The elements still to be yielded; a consumed prefix is gone.
            Value::Iter(state) => {
                f.write_str("iter([")?;
                write_comma_separated(f, state.0.as_slice())?;
                f.write_str("])")
            }
            Value::Variant {
                enum_name,
                variant,
                args,
            } => {
                write!(f, "{}::{}", enum_name, variant)?;
                if !args.is_empty() {
                    f.write_str("(")?;
                    write_comma_separated(f, args)?;
                    f.write_str(")")?;
                }
                Ok(())
            }
            Value::Struct(fields) => {
                f.write_str("{")?;
                for (i, (name, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}: {}", name, value)?;
                }
                f.write_str("}")
            }
            Value::Quantity(q) => write!(f, "{}", q),
            Value::Future => f.write_str("<future>"),
        }
    }
}

fn write_comma_separated(f: &mut std::fmt::Formatter<'_>, values: &[Value]) -> std::fmt::Result {
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", value)?;
    }
    Ok(())
}
