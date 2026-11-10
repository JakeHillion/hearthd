use serde::de::DeserializeOwned;

use crate::Context;
use crate::Definitions;
use crate::FromConfig;

/// Read a value that is written whole in one place: required, and in
/// conflict with itself if a second file sets it too.
pub fn scalar<T: DeserializeOwned>(
    definitions: Definitions<'_>,
    context: &mut Context,
) -> Option<T> {
    let Some(first) = definitions.nodes().first().copied() else {
        context.missing(&definitions);
        return None;
    };
    if definitions.nodes().len() > 1 {
        context.conflict(&definitions);
    }

    match first.read() {
        Ok(value) => Some(value),
        Err((span, message)) => {
            context.error_at(definitions.path(), first, span, message);
            None
        }
    }
}

macro_rules! scalars {
    ($($ty:ty),* $(,)?) => {
        $(
            impl FromConfig for $ty {
                fn from_config(definitions: Definitions<'_>, context: &mut Context) -> Option<Self> {
                    scalar(definitions, context)
                }
            }
        )*
    };
}

scalars!(bool, i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, String);
