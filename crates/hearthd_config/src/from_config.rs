use std::path::PathBuf;

use crate::Diagnostic;
use crate::Diagnostics;
use crate::Document;
use crate::Error;
use crate::Node;
use crate::Sources;
use crate::ValidationError;
use crate::Warning;

/// A type that can be read from configuration.
///
/// It is handed every place its path was written, across all files, and
/// decides for itself what several definitions mean: one value conflicts
/// with another, while tables merge key by key.
pub trait FromConfig: Sized {
    /// Returns `None` once an error has been reported that leaves no value
    /// to return.
    fn from_config(definitions: Definitions<'_>, context: &mut Context) -> Option<Self>;
}

/// What has been found wrong so far.
#[derive(Debug, Default)]
pub struct Context {
    pub diagnostics: Vec<Diagnostic>,
}

impl Context {
    /// Report `message` against what was written at `node`.
    pub fn error(&mut self, path: &str, node: Node<'_>, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::Error(Error::Validation(ValidationError {
                field_path: path.to_string(),
                message: message.into(),
                span: Some(node.span()),
                source: Some(node.source().clone()),
            })));
    }
}

/// Every place one path is given a value, in the order the files were
/// loaded. Empty when no file mentions the path.
#[derive(Debug, Clone)]
pub struct Definitions<'a> {
    path: String,
    nodes: Vec<Node<'a>>,
}

impl<'a> Definitions<'a> {
    fn root(documents: &'a [Document<'a>]) -> Self {
        Self {
            path: String::new(),
            nodes: documents.iter().map(Document::root).collect(),
        }
    }

    /// Dotted path from the top of the configuration, for messages.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn nodes(&self) -> &[Node<'a>] {
        &self.nodes
    }

    /// These definitions as one table, unless some are not tables, which is
    /// reported.
    pub fn tables(self, context: &mut Context) -> Option<Tables<'a>> {
        let mut all = true;
        for node in self.nodes.iter().filter(|node| !node.is_table()) {
            context.error(&self.path, *node, "expected a table");
            all = false;
        }
        all.then_some(Tables {
            path: self.path,
            tables: self.nodes,
        })
    }
}

/// One table as written across any number of files.
#[derive(Debug, Clone)]
pub struct Tables<'a> {
    path: String,
    tables: Vec<Node<'a>>,
}

impl<'a> Tables<'a> {
    /// Everywhere `name` is set in this table.
    pub fn field(&self, name: &str) -> Definitions<'a> {
        let path = if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{}.{name}", self.path)
        };
        Definitions {
            path,
            nodes: self
                .tables
                .iter()
                .filter_map(|table| table.get(name))
                .collect(),
        }
    }
}

/// Read `paths` and everything they import as one `T`.
pub fn load<T: FromConfig>(paths: &[PathBuf]) -> (Option<T>, Diagnostics) {
    match Sources::load(paths) {
        Ok(sources) => from_sources(&sources),
        Err(error) => (None, error.into()),
    }
}

/// Read files that are already in memory as one `T`.
pub fn from_sources<T: FromConfig>(sources: &Sources) -> (Option<T>, Diagnostics) {
    let mut documents = match sources.parse() {
        Ok(documents) => documents,
        Err(errors) => return (None, errors),
    };

    let mut context = Context::default();
    for document in &mut documents {
        if document.is_empty() {
            context
                .diagnostics
                .push(Diagnostic::Warning(Warning::EmptyConfig {
                    file_path: document.source().file_path.clone(),
                }));
        }
        // Already followed when the sources were read, and not something a
        // config type reads.
        document.remove("imports");
    }

    let value = T::from_config(Definitions::root(&documents), &mut context);
    (value, Diagnostics(context.diagnostics))
}
