use hearthd_config::FromConfig;
use hearthd_config::SourceInfo;
use hearthd_config::Sources;
use hearthd_config::SubConfig;
use hearthd_config::TryFromPartial;
use hearthd_config::format_diagnostics_plain;
use hearthd_config::from_sources;
use serde::Deserialize;

/// Reads `files` as a `T`, returning it with the rendered diagnostics.
fn load<T: FromConfig>(files: &[(&str, &str)]) -> (Option<T>, String) {
    let sources = Sources::from(
        files
            .iter()
            .map(|(path, content)| SourceInfo {
                file_path: path.into(),
                content: content.to_string(),
            })
            .collect::<Vec<_>>(),
    );
    let (value, diagnostics) = from_sources(&sources);
    (value, format_diagnostics_plain(&diagnostics.0))
}

#[derive(Debug, FromConfig)]
struct Config {
    http: Http,
    logging: Logging,
    services: Services,
}

#[derive(Debug, FromConfig)]
struct Services {
    http: Http,
}

mod partial {
    use super::*;

    fn default_port() -> u16 {
        8565
    }

    #[derive(Debug, PartialEq, Deserialize, TryFromPartial, SubConfig)]
    pub struct Http {
        #[config(default = "default_port")]
        pub port: u16,
        pub host: Option<String>,
    }

    #[derive(Debug, PartialEq, Deserialize, TryFromPartial, SubConfig)]
    pub struct Logging {
        pub level: Option<String>,
    }
}
use partial::Http;
use partial::Logging;

#[test]
fn a_table_written_in_several_files_is_read_as_one() {
    let (config, diagnostics) = load::<Config>(&[
        ("a.toml", "[http]\nhost = \"localhost\"\n"),
        (
            "b.toml",
            "[http]\nport = 80\n\n[services.http]\nport = 81\n",
        ),
    ]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert_eq!(
        config.http,
        Http {
            port: 80,
            host: Some("localhost".to_string()),
        }
    );
    assert_eq!(config.logging, Logging { level: None });
    assert_eq!(
        config.services.http,
        Http {
            port: 81,
            host: None,
        }
    );
}

#[test]
fn a_value_set_in_two_files_conflicts() {
    let (config, diagnostics) = load::<Config>(&[
        ("a.toml", "[services.http]\nport = 80\n"),
        ("b.toml", "[services.http]\nport = 81\n"),
    ]);

    assert!(config.is_some());
    insta::assert_snapshot!(diagnostics, @"
    Error: Merge conflict in field 'services.http.port'
       ╭─[ a.toml:2:8 ]
       │
     2 │ port = 80
       │        ─┬  
       │         ╰── first definition here
       │
       ├─[ b.toml:2:8 ]
       │
     2 │ port = 81
       │        ─┬  
       │         ╰── conflicts with this definition
       │ 
       │ Note: Field 'services.http.port' defined in multiple config files
    ───╯
    ");
}

#[test]
fn a_mistake_in_one_section_does_not_hide_another() {
    let (config, diagnostics) =
        load::<Config>(&[("a.toml", "[http]\nport = \"80\"\n\n[logging]\nlevel = 3\n")]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Failed to parse config file
       ╭─[ a.toml:2:8 ]
       │
     2 │ port = "80"
       │        ──┬─  
       │          ╰─── invalid type: string "80", expected u16
    ───╯
    Error: Failed to parse config file
       ╭─[ a.toml:5:9 ]
       │
     5 │ level = 3
       │         ┬  
       │         ╰── invalid type: integer `3`, expected a string
    ───╯
    "#);
}

#[test]
fn a_section_must_be_a_table() {
    let (config, diagnostics) = load::<Config>(&[("a.toml", "services = \"http\"\n")]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Validation error in 'services'
       ╭─[ a.toml:1:12 ]
       │
     1 │ services = "http"
       │            ───┬──  
       │               ╰──── expected a table
    ───╯
    "#);
}

#[test]
fn a_file_that_sets_nothing_is_warned_about() {
    let (config, diagnostics) = load::<Config>(&[
        ("empty.toml", "# nothing yet\n"),
        ("imports.toml", "imports = []\n"),
    ]);

    assert!(config.is_some());
    insta::assert_snapshot!(diagnostics, @"
    Warning: Empty configuration file
      ┌─ empty.toml:1:1
      │
      = Config file 'empty.toml' is empty and has no effect
    ");
}

#[derive(Debug, FromConfig)]
struct Integrations {
    http: Option<Http>,
    logging: Option<Logging>,
}

#[test]
fn an_optional_section_is_none_when_no_file_mentions_it() {
    let (config, diagnostics) = load::<Integrations>(&[("a.toml", "[http]\nport = 80\n")]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert_eq!(
        config.http,
        Some(Http {
            port: 80,
            host: None,
        })
    );
    assert_eq!(config.logging, None);
}

#[test]
fn an_optional_section_that_is_written_must_be_valid() {
    let (config, diagnostics) = load::<Integrations>(&[("a.toml", "[logging]\nlevel = 3\n")]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @"
    Error: Failed to parse config file
       ╭─[ a.toml:2:9 ]
       │
     2 │ level = 3
       │         ┬  
       │         ╰── invalid type: integer `3`, expected a string
    ───╯
    ");
}
