use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;

use crate::Diagnostic;
use crate::Diagnostics;
use crate::Document;
use crate::Error;
use crate::MergeConflictLocation;
use crate::MergeError;
use crate::Node;
use crate::Sources;
use crate::Validation;
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

/// Left out of every file, an optional value is `None`.
impl<T: FromConfig> FromConfig for Option<T> {
    fn from_config(definitions: Definitions<'_>, context: &mut Context) -> Option<Self> {
        if definitions.is_absent() {
            Some(None)
        } else {
            T::from_config(definitions, context).map(Some)
        }
    }
}

/// A table read as a map, entry by entry: files may each add entries, and
/// an entry written in several files is read as the one value.
impl<K, V> FromConfig for HashMap<K, V>
where
    K: FromStr + Eq + Hash,
    K::Err: Display,
    V: FromConfig,
{
    fn from_config(definitions: Definitions<'_>, context: &mut Context) -> Option<Self> {
        let tables = definitions.tables(context)?;
        let mut entries: Vec<(&str, K, Option<V>)> = Vec::new();
        let mut complete = true;

        for name in tables.keys() {
            let entry = tables.field(name);
            let path = entry.path.clone();
            let written = entry.nodes[0];
            let value = V::from_config(entry, context);
            complete &= value.is_some();

            let problem = match name.parse::<K>() {
                Ok(key) => match entries.iter().find(|(_, other, _)| *other == key) {
                    Some((other, ..)) => format!("the same key as `{other}`"),
                    None => {
                        entries.push((name, key, value));
                        continue;
                    }
                },
                Err(error) => format!("invalid key: {error}"),
            };
            context.key_error(&path, written, problem);
            complete = false;
        }

        complete.then(|| {
            entries
                .into_iter()
                .filter_map(|(_, key, value)| Some((key, value?)))
                .collect()
        })
    }
}

/// Lists written in several files are joined, in the order the files were
/// loaded.
impl<T: FromConfig> FromConfig for Vec<T> {
    fn from_config(definitions: Definitions<'_>, context: &mut Context) -> Option<Self> {
        let mut items = Vec::new();
        let mut complete = true;

        for node in &definitions.nodes {
            let Some(elements) = node.elements() else {
                context.error(&definitions.path, *node, "expected a list");
                complete = false;
                continue;
            };
            for (index, element) in elements.enumerate() {
                let item = Definitions {
                    path: format!("{}[{index}]", definitions.path),
                    nodes: vec![element],
                    within: definitions.within.clone(),
                    claimed: &[],
                };
                match T::from_config(item, context) {
                    Some(item) => items.push(item),
                    None => complete = false,
                }
            }
        }

        complete.then_some(items)
    }
}

/// What has been found wrong so far.
#[derive(Debug, Default)]
pub struct Context {
    pub diagnostics: Vec<Diagnostic>,
}

impl Context {
    /// Report `message` against what was written at `node`.
    pub fn error(&mut self, path: &str, node: Node<'_>, message: impl Into<String>) {
        self.error_at(path, node, node.span(), message);
    }

    /// Report `message` against the key that names `node`.
    pub fn key_error(&mut self, path: &str, node: Node<'_>, message: impl Into<String>) {
        self.error_at(path, node, node.key_span(), message);
    }

    /// Report `message` against `span` in the file `node` is in.
    pub fn error_at(
        &mut self,
        path: &str,
        node: Node<'_>,
        span: Range<usize>,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(Diagnostic::Error(Error::Validation(ValidationError {
                field_path: path.to_string(),
                message: message.into(),
                span: Some(span),
                source: Some(node.source().clone()),
            })));
    }

    /// Report `message` about a path as a whole: against the first place it
    /// is written or, if no file writes it, the table it belongs in.
    pub fn invalid(&mut self, definitions: &Definitions<'_>, message: impl Into<String>) {
        let place = definitions.nodes.first().or(definitions.within.first());
        self.diagnostics
            .push(Diagnostic::Error(Error::Validation(ValidationError {
                field_path: definitions.path.clone(),
                message: message.into(),
                span: place.filter(|node| !node.is_file()).map(Node::span),
                source: place.map(|node| node.source().clone()),
            })));
    }

    /// Report that nothing sets a value that has to be set.
    pub fn missing(&mut self, definitions: &Definitions<'_>) {
        let name = definitions.path.rsplit('.').next().unwrap_or_default();
        self.invalid(definitions, format!("{name} is required"));
    }

    /// Report that a value which can only be set once is set more than once.
    pub fn conflict(&mut self, definitions: &Definitions<'_>) {
        self.diagnostics
            .push(Diagnostic::Error(Error::Merge(MergeError {
                field_path: definitions.path.clone(),
                message: format!(
                    "Field '{}' defined in multiple config files",
                    definitions.path
                ),
                conflicts: definitions
                    .nodes
                    .iter()
                    .map(|node| MergeConflictLocation {
                        file_path: node.source().file_path.clone(),
                        span: node.span(),
                        content: node.source().content.clone(),
                    })
                    .collect(),
            })));
    }
}

/// Every place one path is given a value, in the order the files were
/// loaded. Empty when no file mentions the path.
#[derive(Debug, Clone)]
pub struct Definitions<'a> {
    path: String,
    nodes: Vec<Node<'a>>,
    /// The nearest tables around this path that some file does write.
    within: Vec<Node<'a>>,
    claimed: &'static [&'static str],
}

impl<'a> Definitions<'a> {
    fn root(documents: &'a [Document<'a>]) -> Self {
        Self {
            path: String::new(),
            nodes: documents.iter().map(Document::root).collect(),
            within: Vec::new(),
            claimed: &[],
        }
    }

    /// Dotted path from the top of the configuration, for messages.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn nodes(&self) -> &[Node<'a>] {
        &self.nodes
    }

    /// Whether no file gives this path a value.
    pub fn is_absent(&self) -> bool {
        self.nodes.is_empty()
    }

    /// What the tables among these definitions set `name` to.
    pub fn get(&self, name: &str) -> Definitions<'a> {
        let tables: Vec<_> = self.nodes.iter().copied().filter(Node::is_table).collect();
        Definitions {
            path: join(&self.path, name),
            nodes: tables.iter().filter_map(|table| table.get(name)).collect(),
            within: if tables.is_empty() {
                self.within.clone()
            } else {
                tables
            },
            claimed: &[],
        }
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
            within: if self.nodes.is_empty() {
                self.within
            } else {
                self.nodes.clone()
            },
            tables: self.nodes,
            claimed: self.claimed,
        })
    }
}

/// One table as written across any number of files.
#[derive(Debug, Clone)]
pub struct Tables<'a> {
    path: String,
    tables: Vec<Node<'a>>,
    within: Vec<Node<'a>>,
    claimed: &'static [&'static str],
}

impl<'a> Tables<'a> {
    /// Everywhere `name` is set in this table.
    pub fn field(&self, name: &str) -> Definitions<'a> {
        Definitions {
            path: join(&self.path, name),
            nodes: self
                .tables
                .iter()
                .filter_map(|table| table.get(name))
                .collect(),
            within: self.within.clone(),
            claimed: &[],
        }
    }

    /// A handle for reporting problems with this table as a whole.
    pub fn validation<'c>(&'c self, context: &'c mut Context) -> Validation<'c> {
        Validation::new(context, self.rest(&[]))
    }

    /// Warn about every key written here that is not in `known`. Left as a
    /// warning so that one file can serve versions with different options.
    pub fn warn_unknown(&self, known: &[&str], context: &mut Context) {
        for table in &self.tables {
            for key in table.keys().filter(|key| !known.contains(key)) {
                let Some(value) = table.get(key) else {
                    continue;
                };
                context
                    .diagnostics
                    .push(Diagnostic::Warning(Warning::UnknownKey {
                        field_path: self.field(key).path,
                        span: value.key_span(),
                        source: table.source().clone(),
                    }));
            }
        }
    }

    /// The keys set in any file, in order.
    pub fn keys(&self) -> BTreeSet<&'a str> {
        self.tables
            .iter()
            .flat_map(Node::keys)
            .filter(|key| !self.claimed.contains(key))
            .collect()
    }

    /// This table without the keys in `claimed`, for a map that takes
    /// whatever the fields beside it do not.
    pub fn rest(&self, claimed: &'static [&'static str]) -> Definitions<'a> {
        Definitions {
            path: self.path.clone(),
            nodes: self.tables.clone(),
            within: self.within.clone(),
            claimed,
        }
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
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
