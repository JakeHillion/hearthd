use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::DeString;
use toml::de::DeTable;
use toml::de::DeValue;
use toml::de::ValueDeserializer;

use crate::Diagnostic;
use crate::Diagnostics;
use crate::Error;
use crate::LoadError;
use crate::SourceInfo;

/// The files that make up one configuration, each after the files it
/// imports.
#[derive(Debug)]
pub struct Sources(Vec<SourceInfo>);

impl From<Vec<SourceInfo>> for Sources {
    fn from(files: Vec<SourceInfo>) -> Self {
        Self(files)
    }
}

impl Sources {
    /// Read `paths` and everything they import.
    pub fn load(paths: &[PathBuf]) -> Result<Self, LoadError> {
        let mut files = Vec::new();
        let mut importing = Vec::new();
        for path in paths {
            read(path, &mut importing, &mut files)?;
        }
        Ok(Self(files))
    }

    /// Parse every file, or report every syntax error in all of them.
    pub fn parse(&self) -> Result<Vec<Document<'_>>, Diagnostics> {
        let mut documents = Vec::new();
        let mut errors = Vec::new();
        for source in &self.0 {
            match Document::parse(source) {
                Ok(document) => documents.push(document),
                Err(found) => errors.extend(found),
            }
        }

        if errors.is_empty() {
            Ok(documents)
        } else {
            Err(Diagnostics(errors))
        }
    }
}

fn read(
    path: &Path,
    importing: &mut Vec<PathBuf>,
    files: &mut Vec<SourceInfo>,
) -> Result<(), LoadError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if importing.contains(&canonical) {
        return Err(LoadError::ImportCycle {
            path: canonical,
            cycle: importing.clone(),
        });
    }

    let content = std::fs::read_to_string(path).map_err(|e| LoadError::Io {
        path: path.to_path_buf(),
        error: e.to_string(),
    })?;
    let source = SourceInfo {
        file_path: path.to_path_buf(),
        content,
    };

    importing.push(canonical);
    for import in imports(&source)? {
        let import = PathBuf::from(import);
        let resolved = if import.is_absolute() {
            import
        } else {
            path.parent().unwrap_or(Path::new(".")).join(import)
        };
        read(&resolved, importing, files)?;
    }
    importing.pop();

    files.push(source);
    Ok(())
}

/// The `imports` a file lists. Syntax errors elsewhere in the file are left
/// for [`Sources::parse`] to report.
fn imports(source: &SourceInfo) -> Result<Vec<String>, LoadError> {
    let (root, _) = DeTable::parse_recoverable(&source.content);
    let Some(imports) = root.get_ref().get("imports") else {
        return Ok(Vec::new());
    };
    Vec::deserialize(ValueDeserializer::from(imports.clone()))
        .map_err(|error| parse_error(source, &error, imports.span()))
}

fn parse_error(source: &SourceInfo, error: &toml::de::Error, fallback: Range<usize>) -> LoadError {
    LoadError::Parse {
        source: source.clone(),
        span: error.span().unwrap_or(fallback),
        message: error.message().to_string(),
    }
}

/// One file, parsed, with the position of every key and value in it.
#[derive(Debug)]
pub struct Document<'s> {
    source: &'s SourceInfo,
    root: Spanned<DeValue<'s>>,
}

impl<'s> Document<'s> {
    fn parse(source: &'s SourceInfo) -> Result<Self, Vec<Diagnostic>> {
        let (root, errors) = DeTable::parse_recoverable(&source.content);
        if errors.is_empty() {
            let span = root.span();
            return Ok(Self {
                source,
                root: Spanned::new(span, DeValue::Table(root.into_inner())),
            });
        }

        // The parser keeps going after a mistake and often reports the same
        // one several times over, so only the first error on a line is kept.
        let mut lines = HashSet::new();
        Err(errors
            .iter()
            .filter(|error| match error.span() {
                Some(span) => lines.insert(source.content[..span.start].matches('\n').count()),
                None => true,
            })
            .map(|error| Diagnostic::Error(Error::Load(parse_error(source, error, 0..0))))
            .collect())
    }

    pub fn source(&self) -> &'s SourceInfo {
        self.source
    }

    /// The file's top-level table.
    pub fn root(&self) -> Node<'_> {
        Node {
            source: self.source,
            key: None,
            value: &self.root,
            file: true,
        }
    }

    /// Whether the file sets nothing at all.
    pub fn is_empty(&self) -> bool {
        matches!(self.root.get_ref(), DeValue::Table(table) if table.is_empty())
    }

    /// Drop a top-level key, so that nothing reading the file sees it.
    pub fn remove(&mut self, key: &str) {
        if let DeValue::Table(table) = self.root.get_mut() {
            table.remove(key);
        }
    }
}

/// One value as written in one file.
#[derive(Debug, Clone, Copy)]
pub struct Node<'a> {
    source: &'a SourceInfo,
    key: Option<&'a Spanned<DeString<'a>>>,
    value: &'a Spanned<DeValue<'a>>,
    file: bool,
}

impl<'a> Node<'a> {
    pub fn source(&self) -> &'a SourceInfo {
        self.source
    }

    pub fn span(&self) -> Range<usize> {
        self.value.span()
    }

    /// Whether this is the whole of a file, not something written in it.
    pub fn is_file(&self) -> bool {
        self.file
    }

    /// Where the key naming this value is written. The top of a file has
    /// none, and the value's own position stands in for it.
    pub fn key_span(&self) -> Range<usize> {
        self.key.map_or_else(|| self.span(), Spanned::span)
    }

    pub fn is_table(&self) -> bool {
        self.value.get_ref().is_table()
    }

    /// The keys this table sets, if it is a table.
    pub fn keys(&self) -> impl Iterator<Item = &'a str> {
        self.value
            .get_ref()
            .as_table()
            .into_iter()
            .flat_map(|table| table.keys())
            .map(|key| key.get_ref().as_ref())
    }

    /// The items of this list, if it is a list.
    pub fn elements(&self) -> Option<impl Iterator<Item = Node<'a>> + use<'a>> {
        let source = self.source;
        let items = self.value.get_ref().as_array()?;
        Some(items.iter().map(move |value| Node {
            source,
            key: None,
            value,
            file: false,
        }))
    }

    /// What this table sets `key` to, if it is a table and sets it.
    pub fn get(&self, key: &str) -> Option<Node<'a>> {
        let (key, value) = self.value.get_ref().as_table()?.get_key_value(key)?;
        Some(Node {
            source: self.source,
            key: Some(key),
            value,
            file: false,
        })
    }

    /// Read this one value with serde, as if it were a whole file.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, Diagnostic> {
        self.read().map_err(|(span, message)| {
            Diagnostic::Error(Error::Load(LoadError::Parse {
                source: self.source.clone(),
                span,
                message,
            }))
        })
    }

    /// Read this one value with serde. On failure, where it went wrong and
    /// why.
    pub fn read<T: DeserializeOwned>(&self) -> Result<T, (Range<usize>, String)> {
        T::deserialize(ValueDeserializer::from(self.value.clone())).map_err(|error| {
            (
                error.span().unwrap_or(self.span()),
                error.message().to_string(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::format_diagnostics_plain;

    fn sources(files: &[(&str, &str)]) -> Sources {
        files
            .iter()
            .map(|(path, content)| SourceInfo {
                file_path: PathBuf::from(path),
                content: content.to_string(),
            })
            .collect::<Vec<_>>()
            .into()
    }

    #[test]
    fn every_syntax_error_in_every_file_is_reported() {
        let sources = sources(&[
            ("a.toml", "[http]\nport = \n\n[logging]\nlevel = info\n"),
            ("b.toml", "[locations.home\nlatitude = 59.9\n"),
        ]);
        let errors = sources.parse().unwrap_err();

        insta::assert_snapshot!(format_diagnostics_plain(&errors.0), @"
        Error: Failed to parse config file
           ╭─[ a.toml:2:8 ]
           │
         2 │ port =
           │        │ 
           │        ╰─ string values must be quoted, expected literal string
        ───╯
        Error: Failed to parse config file
           ╭─[ a.toml:5:9 ]
           │
         5 │ level = info
           │         ──┬─  
           │           ╰─── invalid float, expected `inf`
        ───╯
        Error: Failed to parse config file
           ╭─[ b.toml:1:16 ]
           │
         1 │ [locations.home
           │                │ 
           │                ╰─ unclosed table, expected `]`
        ───╯
        ");
    }

    #[test]
    fn one_mistake_is_reported_once() {
        let sources = sources(&[("a.toml", "a = [1, 2\nb = 3\n")]);
        let errors = sources.parse().unwrap_err();

        insta::assert_snapshot!(format_diagnostics_plain(&errors.0), @"
        Error: Failed to parse config file
           ╭─[ a.toml:2:1 ]
           │
         2 │ b = 3
           │ │ 
           │ ╰─ missing comma between array elements, expected `,`
        ───╯
        ");
    }

    #[test]
    fn a_value_of_the_wrong_type_is_located() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Http {
            port: u16,
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Root {
            http: Http,
        }

        let sources = sources(&[("a.toml", "[http]\nport = \"8565\"\n")]);
        let documents = sources.parse().unwrap();
        let error = documents[0].root().deserialize::<Root>().unwrap_err();

        insta::assert_snapshot!(format_diagnostics_plain(&[error]), @r#"
        Error: Failed to parse config file
           ╭─[ a.toml:2:8 ]
           │
         2 │ port = "8565"
           │        ───┬──  
           │           ╰──── invalid type: string "8565", expected u16
        ───╯
        "#);
    }

    #[test]
    fn imports_load_before_the_file_that_names_them() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("extra")).unwrap();
        fs::write(dir.path().join("extra/base.toml"), "a = 1\n").unwrap();
        fs::write(
            dir.path().join("main.toml"),
            "imports = [\"extra/base.toml\"]\nb = 2\n",
        )
        .unwrap();

        let sources = Sources::load(&[dir.path().join("main.toml")]).unwrap();
        let names: Vec<_> = sources
            .0
            .iter()
            .map(|source| source.file_path.strip_prefix(dir.path()).unwrap())
            .collect();

        assert_eq!(
            names,
            [Path::new("extra/base.toml"), Path::new("main.toml")]
        );
    }

    #[test]
    fn an_import_cycle_is_an_error() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.toml"), "imports = [\"b.toml\"]\n").unwrap();
        fs::write(dir.path().join("b.toml"), "imports = [\"a.toml\"]\n").unwrap();

        let error = Sources::load(&[dir.path().join("a.toml")]).unwrap_err();

        assert!(
            matches!(&error, LoadError::ImportCycle { cycle, .. } if cycle.len() == 2),
            "{error:?}"
        );
    }

    #[test]
    fn a_missing_import_names_the_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.toml"), "imports = [\"gone.toml\"]\n").unwrap();

        let error = Sources::load(&[dir.path().join("a.toml")]).unwrap_err();

        assert!(
            matches!(&error, LoadError::Io { path, .. } if path.ends_with("gone.toml")),
            "{error:?}"
        );
    }

    #[test]
    fn imports_must_be_a_list_of_paths() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.toml");
        fs::write(&path, "imports = \"b.toml\"\n").unwrap();

        let error = Sources::load(std::slice::from_ref(&path)).unwrap_err();
        let rendered = format_diagnostics_plain(&[Diagnostic::Error(Error::Load(error))])
            .replace(&path.display().to_string(), "a.toml");

        insta::assert_snapshot!(rendered, @r#"
        Error: Failed to parse config file
           ╭─[ a.toml:1:11 ]
           │
         1 │ imports = "b.toml"
           │           ────┬───  
           │               ╰───── invalid type: string "b.toml", expected a sequence
        ───╯
        "#);
    }
}
