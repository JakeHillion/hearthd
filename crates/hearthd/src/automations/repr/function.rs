//! Function identities for the HearthD Automations language.
//!
//! The checker resolves every call to a [`FunctionIdentity`] and that
//! identity is what flows through the typed AST, HIR, LIR and bytecode. No
//! stage below the checker looks a function up by name, so the VM cannot
//! fail to resolve one.
//!
//! Every function the language defines is a variant of its own, named for the
//! function rather than for the fact that it is built in. That is what makes
//! the set closed: adding one fails to compile everywhere it must be handled,
//! including the VM's dispatch. If user-defined functions arrive they are
//! expected to be inlined before reaching the VM, so nothing here has to
//! represent them; should that change, an indexed variant slots in without
//! renaming what is already here.

use strum::Display;
use strum::EnumString;

/// Which function a call targets, as resolved by the checker.
///
/// The source spelling of each variant is derived rather than written out, so
/// the name the checker resolves and the name diagnostics print cannot drift
/// apart, and adding a variant needs no conversion table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum FunctionIdentity {
    /// `len(collection)` — element or character count.
    Len,
    /// `abs(n)` — absolute value.
    Abs,
    /// `min(a, b)`.
    Min,
    /// `max(a, b)`.
    Max,
    /// `clamp(n, lo, hi)`.
    Clamp,
    /// `keys(map)`.
    Keys,
    /// `values(map)`.
    Values,
    /// `sleep(duration)` — suspends, always completes.
    Sleep,
    /// `sleep_unique(duration)` — suspends, cancellable by a newer instance.
    SleepUnique,
}
