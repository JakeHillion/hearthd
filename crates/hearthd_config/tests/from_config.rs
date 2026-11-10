use std::collections::HashMap;

use hearthd_config::ConfigValue;
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

#[derive(Debug, FromConfig)]
struct Network {
    hosts: HashMap<String, Http>,
    outlets: HashMap<u8, Logging>,
}

#[test]
fn map_entries_are_gathered_from_every_file() {
    let (config, diagnostics) = load::<Network>(&[
        (
            "a.toml",
            "[hosts.nas]\nport = 80\n\n[hosts.router]\nport = 81\n",
        ),
        (
            "b.toml",
            "[hosts.nas]\nhost = \"nas.lan\"\n\n[hosts.printer]\nport = 82\n",
        ),
    ]);

    assert_eq!(diagnostics, "");
    let hosts = config.unwrap().hosts;
    assert_eq!(hosts.len(), 3);
    assert_eq!(
        hosts["nas"],
        Http {
            port: 80,
            host: Some("nas.lan".to_string()),
        }
    );
    assert_eq!(hosts["printer"].port, 82);
}

#[test]
fn a_map_no_file_mentions_is_empty() {
    let (config, diagnostics) = load::<Network>(&[("a.toml", "[outlets.3]\n")]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert!(config.hosts.is_empty());
    assert_eq!(config.outlets[&3], Logging { level: None });
}

#[test]
fn a_map_key_must_parse_as_the_key_type() {
    let (config, diagnostics) = load::<Network>(&[(
        "a.toml",
        "[outlets.three]\nlevel = \"info\"\n\n[outlets.300]\n",
    )]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @"
    Error: Validation error in 'outlets.300'
       ╭─[ a.toml:4:10 ]
       │
     4 │ [outlets.300]
       │          ─┬─  
       │           ╰─── invalid key: number too large to fit in target type
    ───╯
    Error: Validation error in 'outlets.three'
       ╭─[ a.toml:1:10 ]
       │
     1 │ [outlets.three]
       │          ──┬──  
       │            ╰──── invalid key: invalid digit found in string
    ───╯
    ");
}

#[test]
fn two_spellings_of_one_key_are_rejected() {
    let (config, diagnostics) = load::<Network>(&[("a.toml", "[outlets.3]\n\n[outlets.03]\n")]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @"
    Error: Validation error in 'outlets.3'
       ╭─[ a.toml:1:10 ]
       │
     1 │ [outlets.3]
       │          ┬  
       │          ╰── the same key as `03`
    ───╯
    ");
}

#[test]
fn a_mistake_in_a_map_entry_names_the_entry() {
    let (config, diagnostics) = load::<Network>(&[
        ("a.toml", "[hosts.nas]\nport = 80\n"),
        ("b.toml", "[hosts.nas]\nport = 81\n"),
    ]);

    assert!(config.is_some());
    insta::assert_snapshot!(diagnostics, @"
    Error: Merge conflict in field 'hosts.nas.port'
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
       │ Note: Field 'hosts.nas.port' defined in multiple config files
    ───╯
    ");
}

#[derive(Debug, FromConfig)]
struct Servers {
    primary: Option<Http>,
    #[config(flatten)]
    others: HashMap<String, Http>,
}

#[test]
fn a_flattened_map_takes_the_keys_no_field_claims() {
    let (config, diagnostics) = load::<Servers>(&[
        ("a.toml", "[primary]\nport = 80\n\n[backup]\nport = 81\n"),
        ("b.toml", "[backup]\nhost = \"backup.lan\"\n"),
    ]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert_eq!(config.primary.unwrap().port, 80);
    assert_eq!(config.others.len(), 1);
    assert_eq!(
        config.others["backup"],
        Http {
            port: 81,
            host: Some("backup.lan".to_string()),
        }
    );
}

#[test]
fn a_flattened_entry_is_named_without_the_field_holding_it() {
    let (config, diagnostics) = load::<Servers>(&[
        ("a.toml", "[backup]\nport = 81\n"),
        ("b.toml", "[backup]\nport = 82\n"),
    ]);

    assert!(config.is_some());
    insta::assert_snapshot!(diagnostics, @"
    Error: Merge conflict in field 'backup.port'
       ╭─[ a.toml:2:8 ]
       │
     2 │ port = 81
       │        ─┬  
       │         ╰── first definition here
       │
       ├─[ b.toml:2:8 ]
       │
     2 │ port = 82
       │        ─┬  
       │         ╰── conflicts with this definition
       │ 
       │ Note: Field 'backup.port' defined in multiple config files
    ───╯
    ");
}

#[derive(Debug, PartialEq, FromConfig)]
struct Server {
    host: String,
    port: Option<u16>,
    tls: Option<bool>,
    weight: Option<f64>,
}

#[derive(Debug, FromConfig)]
struct Cluster {
    server: Server,
    replicas: HashMap<String, Server>,
}

#[test]
fn values_are_read_where_they_are_written() {
    let (config, diagnostics) = load::<Cluster>(&[(
        "a.toml",
        "[server]\nhost = \"a.lan\"\n\n[replicas.b]\nhost = \"b.lan\"\nport = 81\ntls = true\nweight = 2\n",
    )]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert_eq!(
        config.server,
        Server {
            host: "a.lan".to_string(),
            port: None,
            tls: None,
            weight: None,
        }
    );
    assert_eq!(
        config.replicas["b"],
        Server {
            host: "b.lan".to_string(),
            port: Some(81),
            tls: Some(true),
            weight: Some(2.0),
        }
    );
}

#[test]
fn a_missing_value_points_at_the_table_it_belongs_in() {
    let (config, diagnostics) = load::<Cluster>(&[
        (
            "a.toml",
            "[server]\nhost = \"a.lan\"\n\n[replicas.b]\nport = 81\n",
        ),
        ("b.toml", "[replicas.b]\ntls = true\n"),
    ]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @"
    Error: Validation error in 'replicas.b.host'
       ╭─[ a.toml:4:1 ]
       │
     4 │ [replicas.b]
       │ ──────┬─────  
       │       ╰─────── host is required
    ───╯
    ");
}

#[test]
fn a_missing_value_in_a_section_nobody_wrote_names_the_file() {
    let (config, diagnostics) = load::<Cluster>(&[("a.toml", "[replicas]\n")]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @"
    Error: Validation error in 'server.host'
      ┌─ a.toml:1:1
      │
      = host is required
    ");
}

#[test]
fn every_value_of_the_wrong_type_is_reported() {
    let (config, diagnostics) = load::<Cluster>(&[(
        "a.toml",
        "[server]\nhost = 4\nport = \"80\"\ntls = \"yes\"\n",
    )]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Validation error in 'server.host'
       ╭─[ a.toml:2:8 ]
       │
     2 │ host = 4
       │        ┬  
       │        ╰── invalid type: integer `4`, expected a string
    ───╯
    Error: Validation error in 'server.port'
       ╭─[ a.toml:3:8 ]
       │
     3 │ port = "80"
       │        ──┬─  
       │          ╰─── invalid type: string "80", expected u16
    ───╯
    Error: Validation error in 'server.tls'
       ╭─[ a.toml:4:7 ]
       │
     4 │ tls = "yes"
       │       ──┬──  
       │         ╰──── invalid type: string "yes", expected a boolean
    ───╯
    "#);
}

#[test]
fn a_value_written_in_two_files_conflicts() {
    let (config, diagnostics) = load::<Cluster>(&[
        ("a.toml", "[server]\nhost = \"a.lan\"\n"),
        ("b.toml", "[server]\nhost = \"b.lan\"\n"),
    ]);

    assert_eq!(config.unwrap().server.host, "a.lan");
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Merge conflict in field 'server.host'
       ╭─[ a.toml:2:8 ]
       │
     2 │ host = "a.lan"
       │        ───┬───  
       │           ╰───── first definition here
       │
       ├─[ b.toml:2:8 ]
       │
     2 │ host = "b.lan"
       │        ───┬───  
       │           ╰───── conflicts with this definition
       │ 
       │ Note: Field 'server.host' defined in multiple config files
    ───╯
    "#);
}

const DEFAULT_PORT: u16 = 8565;

#[derive(Debug, PartialEq, FromConfig)]
struct Listener {
    #[config(default = "127.0.0.1")]
    address: String,
    #[config(default = DEFAULT_PORT)]
    port: u16,
    #[config(default = 1.5)]
    weight: f64,
}

#[derive(Debug, FromConfig)]
struct Listeners {
    main: Listener,
    admin: Listener,
}

#[test]
fn an_absent_value_takes_its_default() {
    let (config, diagnostics) = load::<Listeners>(&[("a.toml", "[admin]\nport = 9000\n")]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert_eq!(
        config.main,
        Listener {
            address: "127.0.0.1".to_string(),
            port: 8565,
            weight: 1.5,
        }
    );
    assert_eq!(
        config.admin,
        Listener {
            address: "127.0.0.1".to_string(),
            port: 9000,
            weight: 1.5,
        }
    );
}

#[test]
fn a_default_does_not_excuse_a_wrong_value() {
    let (config, diagnostics) = load::<Listeners>(&[("a.toml", "[main]\nport = \"http\"\n")]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Validation error in 'main.port'
       ╭─[ a.toml:2:8 ]
       │
     2 │ port = "http"
       │        ───┬──  
       │           ╰──── invalid type: string "http", expected u16
    ───╯
    "#);
}

#[derive(Debug, PartialEq, Deserialize, ConfigValue)]
#[serde(rename_all = "snake_case")]
enum Model {
    ApcAp7920,
    ApcAp7921,
}

/// A value that is written as a string and parsed.
#[derive(Debug, PartialEq, Deserialize, ConfigValue)]
#[serde(try_from = "String")]
struct Percent(u8);

impl TryFrom<String> for Percent {
    type Error = String;

    fn try_from(written: String) -> Result<Self, Self::Error> {
        written
            .strip_suffix('%')
            .and_then(|number| number.parse().ok())
            .filter(|percent| *percent <= 100)
            .map(Percent)
            .ok_or_else(|| format!("`{written}` is not a percentage such as `80%`"))
    }
}

#[derive(Debug, FromConfig)]
struct Pdu {
    model: Model,
    #[config(default = Model::ApcAp7920)]
    spare: Model,
    alarm_at: Option<Percent>,
    racks: HashMap<String, Model>,
}

#[test]
fn a_value_type_is_read_by_its_own_deserializer() {
    let (config, diagnostics) = load::<Pdu>(&[(
        "a.toml",
        "model = \"apc_ap7921\"\nalarm_at = \"80%\"\n\n[racks]\nnorth = \"apc_ap7920\"\n",
    )]);

    assert_eq!(diagnostics, "");
    let config = config.unwrap();
    assert_eq!(config.model, Model::ApcAp7921);
    assert_eq!(config.spare, Model::ApcAp7920);
    assert_eq!(config.alarm_at, Some(Percent(80)));
    assert_eq!(config.racks["north"], Model::ApcAp7920);
}

#[test]
fn a_value_its_type_rejects_is_reported_where_it_is_written() {
    let (config, diagnostics) = load::<Pdu>(&[(
        "a.toml",
        "model = \"apc_ap9999\"\nalarm_at = \"loud\"\n\n[racks]\nnorth = 7920\n",
    )]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Validation error in 'model'
       ╭─[ a.toml:1:9 ]
       │
     1 │ model = "apc_ap9999"
       │         ──────┬─────  
       │               ╰─────── unknown variant `apc_ap9999`, expected `apc_ap7920` or `apc_ap7921`
    ───╯
    Error: Validation error in 'alarm_at'
       ╭─[ a.toml:2:12 ]
       │
     2 │ alarm_at = "loud"
       │            ───┬──  
       │               ╰──── `loud` is not a percentage such as `80%`
    ───╯
    Error: Validation error in 'racks.north'
       ╭─[ a.toml:5:9 ]
       │
     5 │ north = 7920
       │         ──┬─  
       │           ╰─── wanted string or table
    ───╯
    "#);
}

#[test]
fn a_value_type_is_required_and_set_once_like_any_scalar() {
    let (config, diagnostics) = load::<Pdu>(&[
        ("a.toml", "[racks]\nnorth = \"apc_ap7920\"\n"),
        ("b.toml", "[racks]\nnorth = \"apc_ap7921\"\n"),
    ]);

    assert!(config.is_none());
    insta::assert_snapshot!(diagnostics, @r#"
    Error: Validation error in 'model'
      ┌─ a.toml:1:1
      │
      = model is required

    Error: Merge conflict in field 'racks.north'
       ╭─[ a.toml:2:9 ]
       │
     2 │ north = "apc_ap7920"
       │         ──────┬─────  
       │               ╰─────── first definition here
       │
       ├─[ b.toml:2:9 ]
       │
     2 │ north = "apc_ap7921"
       │         ──────┬─────  
       │               ╰─────── conflicts with this definition
       │ 
       │ Note: Field 'racks.north' defined in multiple config files
    ───╯
    "#);
}
