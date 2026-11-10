use std::fmt::Display;

use crate::Context;
use crate::Definitions;

/// A check on a config value as a whole, run once all of its fields have
/// been read.
///
/// Implement it for a struct that derives `FromConfig` when its fields can
/// each be fine and still be wrong together. A struct with no implementation
/// is simply not checked.
pub trait Validate {
    fn validate(&self, validation: &mut Validation<'_>);
}

/// Where the value being validated was written, for reporting against it.
pub struct Validation<'a> {
    context: &'a mut Context,
    at: Definitions<'a>,
    key: bool,
}

impl<'a> Validation<'a> {
    pub fn new(context: &'a mut Context, at: Definitions<'a>) -> Self {
        Self {
            context,
            at,
            key: false,
        }
    }

    /// The value of `name` in this table: a field, or an entry of a map.
    pub fn field(&mut self, name: impl Display) -> Validation<'_> {
        Validation {
            context: self.context,
            at: self.at.get(&name.to_string()),
            key: false,
        }
    }

    /// The key `name` in this table, as opposed to what it is set to.
    pub fn key(&mut self, name: impl Display) -> Validation<'_> {
        Validation {
            key: true,
            ..self.field(name)
        }
    }

    /// Report `message` here. Something that is not written in any file,
    /// such as a field left to its default, is reported against the table
    /// it belongs in.
    pub fn error(&mut self, message: impl Into<String>) {
        match self.at.nodes().first() {
            Some(node) if self.key => self.context.key_error(self.at.path(), *node, message),
            _ => self.context.invalid(&self.at, message),
        }
    }
}

/// Lets the `FromConfig` derive call `Validate` on the structs that
/// implement it and do nothing for the rest. Method lookup prefers
/// `WithValidate`, which needs no extra reference, wherever it applies.
#[doc(hidden)]
pub struct Probe<'v, T>(pub &'v T);

#[doc(hidden)]
pub trait WithValidate {
    fn run(&self, validation: &mut Validation<'_>);
}

impl<T: Validate> WithValidate for Probe<'_, T> {
    fn run(&self, validation: &mut Validation<'_>) {
        self.0.validate(validation);
    }
}

#[doc(hidden)]
pub trait WithoutValidate {
    fn run(&self, validation: &mut Validation<'_>);
}

impl<T> WithoutValidate for &Probe<'_, T> {
    fn run(&self, _validation: &mut Validation<'_>) {}
}
