use std::collections::BTreeMap;

use serde::Deserialize;
use serde_yaml::Value;

#[derive(Debug, Clone, Default)]
pub struct ComposeProject {
    pub name: Option<String>,
    pub services: BTreeMap<String, ComposeService>,
    pub unsupported: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ComposeService {
    pub image: Option<String>,
    pub environment: BTreeMap<String, String>,
    pub ports: Vec<String>,
    pub volumes: Vec<String>,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub working_dir: Option<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ComposeRaw {
    name: Option<String>,
    #[serde(default)]
    services: BTreeMap<String, ServiceRaw>,
}

#[derive(Debug, Deserialize, Default)]
struct ServiceRaw {
    image: Option<String>,
    environment: Option<Value>,
    ports: Option<Value>,
    volumes: Option<Value>,
    command: Option<Value>,
    entrypoint: Option<Value>,
    working_dir: Option<String>,
    depends_on: Option<Value>,
}

pub fn parse_compose_documents(input: &str) -> Result<Vec<ComposeProject>, String> {
    let mut out = Vec::new();
    let docs = serde_yaml::Deserializer::from_str(input);

    for (idx, doc) in docs.into_iter().enumerate() {
        let parsed = ComposeRaw::deserialize(doc)
            .map_err(|err| format!("Compose YAML parse error in document {}: {err}", idx + 1))?;
        out.push(normalize(parsed));
    }

    Ok(out)
}

fn normalize(raw: ComposeRaw) -> ComposeProject {
    let mut project = ComposeProject {
        name: raw.name,
        services: BTreeMap::new(),
        unsupported: Vec::new(),
    };

    for (name, svc_raw) in raw.services {
        let service = ComposeService {
            image: svc_raw.image,
            environment: parse_environment(svc_raw.environment),
            ports: parse_string_array(svc_raw.ports),
            volumes: parse_string_array(svc_raw.volumes),
            command: parse_command_like(svc_raw.command),
            entrypoint: parse_command_like(svc_raw.entrypoint),
            working_dir: svc_raw.working_dir,
            depends_on: parse_depends_on(svc_raw.depends_on),
        };
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

fn parse_string_array(value: Option<Value>) -> Vec<String> {
    let Some(raw) = value else {
        return Vec::new();
    };

    match raw {
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

fn to_scalar_string(value: &Value) -> String {
    match value {
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => String::new(),
    }
}
