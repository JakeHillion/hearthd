//! Runtime values handled by the bytecode VM.
//!
//! `Value` is the dynamic representation that VM registers hold. It mirrors
//! the type system the checker enforces, but tracks none of the static
//! type information at runtime — the bytecode has already type-checked.
//!
//! Domain-specific cluster snapshots (`OnOffCluster`, `OccupancySensingCluster`,
//! …) and engine `Node` references are deliberately not modeled yet; they
//! arrive once the runner starts feeding real engine state to the VM in
//! later commits.

use crate::matter::NodeId;

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

    /// A reference to a Matter node in the engine state. Field accesses
    /// (`node.id`, `node.entity_id`, …) resolve against the engine
    /// snapshot when read.
    Node(NodeId),
}

/// A list iterator: the source list and a 0-based cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct IterState {
    pub list: Vec<Value>,
    pub cursor: usize,
}

impl IterState {
    pub fn new(list: Vec<Value>) -> Self {
        Self { list, cursor: 0 }
    }

    /// Advance the cursor and return the next element, or `None` if the
    /// iterator is exhausted.
    pub fn advance(&mut self) -> Option<Value> {
        if self.cursor < self.list.len() {
            let v = self.list[self.cursor].clone();
            self.cursor += 1;
            Some(v)
        } else {
            None
        }
    }
}
