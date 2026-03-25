use std::collections::BTreeMap;

use serde::Deserialize;
use serde_yaml::Value;

#[derive(Debug, Clone, Default)]
pub struct ComposeProject {
    pub name: Option<String>,
    pub services: BTreeMap<String, ComposeService>,
    pub unsupported: Vec<String>,
    pub includes: Vec<ComposeInclude>,
}

/// A single `include` entry extracted from the Compose file.
///
/// Supports both the short form (`- path.yml`) and the long form
/// (`path`, `project_directory`, `env_file`).
#[derive(Debug, Clone)]
pub struct ComposeInclude {
    /// One or more Compose file paths to merge for this include.
    pub paths: Vec<String>,
    /// Optional project directory override (informational only in this tool).
    pub project_directory: Option<String>,
    /// Optional env-file path(s) (informational only in this tool).
    pub env_files: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ComposeService {
    pub image: Option<String>,
    pub build: Option<ComposeBuild>,
    pub environment: BTreeMap<String, String>,
    pub ports: Vec<ComposePort>,
    pub volumes: Vec<ComposeVolumeMount>,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub working_dir: Option<String>,
    pub depends_on: Vec<String>,
    pub post_start: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ComposeBuild {
    pub context: Option<String>,
    pub dockerfile: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposePort {
    pub host: Option<String>,
    pub container: String,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeVolumeMount {
    pub source: Option<String>,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Deserialize)]
struct ComposeRaw {
    name: Option<String>,
    #[serde(default)]
    services: BTreeMap<String, ServiceRaw>,
    include: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct ServiceRaw {
    image: Option<String>,
    build: Option<Value>,
    environment: Option<Value>,
    ports: Option<Value>,
    volumes: Option<Value>,
    command: Option<Value>,
    entrypoint: Option<Value>,
    working_dir: Option<String>,
    depends_on: Option<Value>,
    post_start: Option<Value>,
}

pub fn parse_compose_documents(input: &str) -> Result<Vec<ComposeProject>, String> {
    let mut out = Vec::new();
    let docs = serde_yaml::Deserializer::from_str(input);

    for (idx, doc) in docs.into_iter().enumerate() {
        let value = Value::deserialize(doc)
            .map_err(|err| format!("Compose YAML parse error in document {}: {err}", idx + 1))?;

        if matches!(value, Value::Null) {
            continue;
        }

        let parsed = serde_yaml::from_value::<ComposeRaw>(value.clone())
            .map_err(|err| format!("Compose YAML parse error in document {}: {err}", idx + 1))?;
        out.push(normalize(parsed, &value, idx + 1));
    }

    Ok(out)
}

fn normalize(raw: ComposeRaw, raw_value: &Value, document_index: usize) -> ComposeProject {
    let mut project = ComposeProject {
        name: raw.name,
        services: BTreeMap::new(),
        unsupported: collect_top_level_unsupported(raw_value, document_index),
        includes: parse_includes(raw.include),
    };

    for (name, svc_raw) in raw.services {
        let build = parse_build(svc_raw.build.clone());
        let service = ComposeService {
            image: svc_raw.image,
            build: build.clone(),
            environment: parse_environment(svc_raw.environment),
            ports: parse_ports(svc_raw.ports),
            volumes: parse_volumes(svc_raw.volumes),
            command: parse_command_like(svc_raw.command),
            entrypoint: parse_command_like(svc_raw.entrypoint),
            working_dir: svc_raw.working_dir,
            depends_on: parse_depends_on(svc_raw.depends_on),
            post_start: parse_post_start(svc_raw.post_start),
        };

        project.unsupported.extend(collect_service_unsupported(
            raw_value,
            document_index,
            &name,
        ));

        if build.is_some() {
            project.unsupported.push(format!(
                "Document {document_index}, service `{name}` uses `build`; build contexts are parsed but not converted yet."
            ));
        }

        project.services.insert(name, service);
    }

    project
}

fn parse_environment(value: Option<Value>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(raw) = value else {
        return map;
    };

    match raw {
        Value::Mapping(m) => {
            for (k, v) in m {
                if let Some(key) = k.as_str() {
                    let val = match v {
                        Value::Null => String::new(),
                        Value::String(s) => s,
                        other => to_scalar_string(&other),
                    };
                    map.insert(key.to_string(), val);
                }
            }
        }
        Value::Sequence(seq) => {
            for item in seq {
                if let Some(raw_item) = item.as_str() {
                    if let Some((k, v)) = raw_item.split_once('=') {
                        map.insert(k.to_string(), v.to_string());
                    } else {
                        map.insert(raw_item.to_string(), String::new());
                    }
                }
            }
        }
        _ => {}
    }

    map
}

fn parse_build(value: Option<Value>) -> Option<ComposeBuild> {
    let raw = value?;

    match raw {
        Value::String(context) => Some(ComposeBuild {
            context: Some(context),
            dockerfile: None,
        }),
        Value::Mapping(map) => {
            let context = map
                .get(Value::String(String::from("context")))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let dockerfile = map
                .get(Value::String(String::from("dockerfile")))
                .and_then(Value::as_str)
                .map(ToString::to_string);

            Some(ComposeBuild {
                context,
                dockerfile,
            })
        }
        _ => None,
    }
}

fn parse_ports(value: Option<Value>) -> Vec<ComposePort> {
    let Some(raw) = value else {
        return Vec::new();
    };

    match raw {
        Value::Sequence(seq) => seq
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => parse_port_short_syntax(&s),
                Value::Mapping(m) => parse_port_long_syntax(&m),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_volumes(value: Option<Value>) -> Vec<ComposeVolumeMount> {
    let Some(raw) = value else {
        return Vec::new();
    };

    match raw {
        Value::Sequence(seq) => seq
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => parse_volume_short_syntax(&s),
                Value::Mapping(m) => parse_volume_long_syntax(&m),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_command_like(value: Option<Value>) -> Vec<String> {
    let Some(raw) = value else {
        return Vec::new();
    };

    match raw {
        Value::String(s) => vec![s],
        Value::Sequence(seq) => seq
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Parse Docker Compose `post_start` which can be:
/// - A string: `post_start: "./bin/migrate"`
/// - A list of strings: `post_start: ["./run1", "./run2"]`
/// - A list of maps with `command` key (Docker Compose spec):
///   ```yaml
///   post_start:
///     - command: ./bin/migrate
///   ```
fn parse_post_start(value: Option<Value>) -> Vec<String> {
    let Some(raw) = value else {
        return Vec::new();
    };

    match raw {
        Value::String(s) => vec![s],
        Value::Sequence(seq) => seq
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                Value::Number(n) => Some(n.to_string()),
                Value::Mapping(m) => m
                    .get(Value::String("command".to_string()))
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_depends_on(value: Option<Value>) -> Vec<String> {
    let Some(raw) = value else {
        return Vec::new();
    };

    match raw {
        Value::Sequence(seq) => seq
            .into_iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
        Value::Mapping(map) => map
            .into_iter()
            .filter_map(|(k, _)| k.as_str().map(ToString::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Parse the top-level `include` key.
///
/// Accepts:
/// ```yaml
/// include:
///   - path/to/file.yml              # short form
///   - path: ./other.yml             # long form, single path
///   - path:                         # long form, multiple paths
///       - ./base.yml
///       - ./overlay.yml
///     project_directory: ./dir
///     env_file: .env                # string or list
/// ```
fn parse_includes(value: Option<Value>) -> Vec<ComposeInclude> {
    let Some(raw) = value else {
        return Vec::new();
    };

    let Value::Sequence(seq) = raw else {
        return Vec::new();
    };

    seq.into_iter()
        .filter_map(|item| match item {
            Value::String(path) => Some(ComposeInclude {
                paths: vec![path],
                project_directory: None,
                env_files: Vec::new(),
            }),
            Value::Mapping(map) => {
                let paths = match map.get(Value::String(String::from("path"))) {
                    Some(Value::String(s)) => vec![s.clone()],
                    Some(Value::Sequence(seq)) => seq
                        .iter()
                        .filter_map(|v| v.as_str().map(ToString::to_string))
                        .collect(),
                    _ => return None,
                };
                let project_directory = map
                    .get(Value::String(String::from("project_directory")))
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let env_files = match map.get(Value::String(String::from("env_file"))) {
                    Some(Value::String(s)) => vec![s.clone()],
                    Some(Value::Sequence(seq)) => seq
                        .iter()
                        .filter_map(|v| v.as_str().map(ToString::to_string))
                        .collect(),
                    _ => Vec::new(),
                };
                Some(ComposeInclude {
                    paths,
                    project_directory,
                    env_files,
                })
            }
            _ => None,
        })
        .collect()
}

fn to_scalar_string(value: &Value) -> String {
    match value {
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => String::new(),
    }
}

fn collect_top_level_unsupported(raw_value: &Value, document_index: usize) -> Vec<String> {
    let mut unsupported = Vec::new();
    let Some(map) = raw_value.as_mapping() else {
        return unsupported;
    };

    let allowed = ["name", "services", "version", "include"];
    for key in map.keys().filter_map(Value::as_str) {
        if !allowed.contains(&key) {
            unsupported.push(format!(
                "Document {document_index} uses unsupported top-level key `{key}`."
            ));
        }
    }

    unsupported
}

fn collect_service_unsupported(
    raw_value: &Value,
    document_index: usize,
    service_name: &str,
) -> Vec<String> {
    let mut unsupported = Vec::new();
    let Some(root) = raw_value.as_mapping() else {
        return unsupported;
    };
    let Some(services) = root
        .get(Value::String(String::from("services")))
        .and_then(Value::as_mapping)
    else {
        return unsupported;
    };
    let Some(service) = services
        .get(Value::String(service_name.to_string()))
        .and_then(Value::as_mapping)
    else {
        return unsupported;
    };

    let allowed = [
        "image",
        "build",
        "environment",
        "ports",
        "volumes",
        "command",
        "entrypoint",
        "working_dir",
        "depends_on",
        "post_start",
    ];

    for key in service.keys().filter_map(Value::as_str) {
        if !allowed.contains(&key) {
            unsupported.push(format!(
                "Document {document_index}, service `{service_name}` uses unsupported key `{key}`."
            ));
        }
    }

    unsupported
}

fn parse_port_short_syntax(raw: &str) -> Option<ComposePort> {
    let (without_protocol, protocol) = raw
        .split_once('/')
        .map_or((raw, None), |(left, right)| (left, Some(right.to_string())));
    let parts = without_protocol.split(':').collect::<Vec<_>>();

    match parts.as_slice() {
        [container] => Some(ComposePort {
            host: None,
            container: (*container).to_string(),
            protocol,
        }),
        [host, container] => Some(ComposePort {
            host: Some((*host).to_string()),
            container: (*container).to_string(),
            protocol,
        }),
        [_, host, container] => Some(ComposePort {
            host: Some((*host).to_string()),
            container: (*container).to_string(),
            protocol,
        }),
        _ => None,
    }
}

fn parse_port_long_syntax(map: &serde_yaml::Mapping) -> Option<ComposePort> {
    let container = map
        .get(Value::String(String::from("target")))
        .map(to_scalar_string_from_value)?;
    let host = map
        .get(Value::String(String::from("published")))
        .map(to_scalar_string_from_value);
    let protocol = map
        .get(Value::String(String::from("protocol")))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Some(ComposePort {
        host,
        container,
        protocol,
    })
}

fn parse_volume_short_syntax(raw: &str) -> Option<ComposeVolumeMount> {
    let parts = raw.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [target] => Some(ComposeVolumeMount {
            source: None,
            target: (*target).to_string(),
            read_only: false,
        }),
        [source, target] => Some(ComposeVolumeMount {
            source: Some((*source).to_string()),
            target: (*target).to_string(),
            read_only: false,
        }),
        [source, target, mode] => Some(ComposeVolumeMount {
            source: Some((*source).to_string()),
            target: (*target).to_string(),
            read_only: *mode == "ro",
        }),
        _ => None,
    }
}

fn parse_volume_long_syntax(map: &serde_yaml::Mapping) -> Option<ComposeVolumeMount> {
    let target = map
        .get(Value::String(String::from("target")))
        .and_then(Value::as_str)?
        .to_string();
    let source = map
        .get(Value::String(String::from("source")))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let read_only = map
        .get(Value::String(String::from("read_only")))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Some(ComposeVolumeMount {
        source,
        target,
        read_only,
    })
}

fn to_scalar_string_from_value(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        Value::Number(v) => v.to_string(),
        Value::Bool(v) => v.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_compose_documents;

    #[test]
    fn captures_unsupported_keys_and_normalizes_short_syntax() {
        let input = r#"
name: sample
networks:
  default: {}
services:
  web:
    image: nginx:latest
    ports:
      - "8080:80/tcp"
    volumes:
      - ".:/workspace:ro"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost"]
"#;

        let projects = parse_compose_documents(input).expect("parses");
        let project = &projects[0];
        let web = project.services.get("web").expect("web exists");

        assert_eq!(web.ports[0].host.as_deref(), Some("8080"));
        assert_eq!(web.ports[0].container, "80");
        assert_eq!(web.ports[0].protocol.as_deref(), Some("tcp"));
        assert_eq!(web.volumes[0].source.as_deref(), Some("."));
        assert_eq!(web.volumes[0].target, "/workspace");
        assert!(web.volumes[0].read_only);
        assert!(
            project
                .unsupported
                .iter()
                .any(|item| item.contains("unsupported top-level key `networks`"))
        );
        assert!(
            project
                .unsupported
                .iter()
                .any(|item| item.contains("service `web` uses unsupported key `healthcheck`"))
        );
    }

    #[test]
    fn parses_include_short_form() {
        let input = r#"
include:
  - ./db.yml
  - ./cache.yml
services:
  web:
    image: nginx
"#;
        let projects = parse_compose_documents(input).expect("parses");
        let project = &projects[0];
        assert_eq!(project.includes.len(), 2);
        assert_eq!(project.includes[0].paths, vec!["./db.yml"]);
        assert_eq!(project.includes[1].paths, vec!["./cache.yml"]);
    }

    #[test]
    fn parses_include_long_form() {
        let input = r#"
include:
  - path: ./base.yml
    project_directory: ./infra
    env_file: .env
  - path:
      - ./a.yml
      - ./b.yml
    env_file:
      - .env.local
      - .env.prod
services:
  web:
    image: nginx
"#;
        let projects = parse_compose_documents(input).expect("parses");
        let project = &projects[0];
        assert_eq!(project.includes.len(), 2);
        assert_eq!(project.includes[0].paths, vec!["./base.yml"]);
        assert_eq!(
            project.includes[0].project_directory.as_deref(),
            Some("./infra")
        );
        assert_eq!(project.includes[0].env_files, vec![".env"]);
        assert_eq!(project.includes[1].paths, vec!["./a.yml", "./b.yml"]);
        assert_eq!(
            project.includes[1].env_files,
            vec![".env.local", ".env.prod"]
        );
    }

    #[test]
    fn include_not_reported_as_unsupported() {
        let input = r#"
include:
  - ./other.yml
services:
  web:
    image: nginx
"#;
        let projects = parse_compose_documents(input).expect("parses");
        let project = &projects[0];
        assert!(
            !project.unsupported.iter().any(|u| u.contains("include")),
            "include should not be flagged as unsupported"
        );
    }
}
