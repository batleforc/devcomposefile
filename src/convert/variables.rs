use std::collections::BTreeMap;

use crate::domain::compose::ComposeProject;

/// Regex-free extraction of `${VAR}`, `${VAR:-default}`, and `${VAR-default}`
/// patterns from a string. Returns collected variable names mapped to their
/// default values (empty string when no default is provided), and the rewritten
/// string with Devfile `{{VAR}}` syntax.
fn rewrite_var_refs(input: &str, vars: &mut BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
            // Find closing brace
            if let Some(close) = input[i + 2..].find('}') {
                let inner = &input[i + 2..i + 2 + close];
                let (name, default) = parse_var_inner(inner);
                if !name.is_empty() {
                    vars.entry(name.to_string())
                        .or_insert_with(|| default.to_string());
                    out.push_str("{{");
                    out.push_str(name);
                    out.push_str("}}");
                    i += 2 + close + 1; // skip past }
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

/// Parse the interior of `${...}` into (name, default).
/// Supports: `VAR`, `VAR:-default`, `VAR-default`.
fn parse_var_inner(inner: &str) -> (&str, &str) {
    if let Some(pos) = inner.find(":-") {
        (&inner[..pos], &inner[pos + 2..])
    } else if let Some(pos) = inner.find('-') {
        (&inner[..pos], &inner[pos + 1..])
    } else {
        (inner, "")
    }
}

/// Rewrite a single optional string field in place.
fn rewrite_opt(field: &mut Option<String>, vars: &mut BTreeMap<String, String>) {
    if let Some(val) = field.as_mut() {
        *val = rewrite_var_refs(val, vars);
    }
}

/// Scan and rewrite all variable references in a `ComposeProject`.
/// Returns a map of variable-name → default-value for every unique variable found.
pub fn extract_and_rewrite_variables(project: &mut ComposeProject) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();

    for service in project.services.values_mut() {
        // image
        rewrite_opt(&mut service.image, &mut vars);

        // environment values
        let env_keys: Vec<String> = service.environment.keys().cloned().collect();
        for key in env_keys {
            if let Some(val) = service.environment.get_mut(&key) {
                *val = rewrite_var_refs(val, &mut vars);
            }
        }

        // command, entrypoint
        for item in service.command.iter_mut() {
            *item = rewrite_var_refs(item, &mut vars);
        }
        for item in service.entrypoint.iter_mut() {
            *item = rewrite_var_refs(item, &mut vars);
        }

        // working_dir
        rewrite_opt(&mut service.working_dir, &mut vars);

        // ports (host / container)
        for port in service.ports.iter_mut() {
            rewrite_opt(&mut port.host, &mut vars);
            port.container = rewrite_var_refs(&port.container, &mut vars);
        }

        // volumes (source / target)
        for vol in service.volumes.iter_mut() {
            rewrite_opt(&mut vol.source, &mut vars);
            vol.target = rewrite_var_refs(&vol.target, &mut vars);
        }
    }

    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_simple_var() {
        let mut vars = BTreeMap::new();
        let out = rewrite_var_refs("uid=${PUID}", &mut vars);
        assert_eq!(out, "uid={{PUID}}");
        assert_eq!(vars.get("PUID").unwrap(), "");
    }

    #[test]
    fn rewrite_var_with_default() {
        let mut vars = BTreeMap::new();
        let out = rewrite_var_refs("port=${PORT:-8080}", &mut vars);
        assert_eq!(out, "port={{PORT}}");
        assert_eq!(vars.get("PORT").unwrap(), "8080");
    }

    #[test]
    fn rewrite_var_with_dash_default() {
        let mut vars = BTreeMap::new();
        let out = rewrite_var_refs("${VAR-fallback}", &mut vars);
        assert_eq!(out, "{{VAR}}");
        assert_eq!(vars.get("VAR").unwrap(), "fallback");
    }

    #[test]
    fn multiple_vars_in_one_string() {
        let mut vars = BTreeMap::new();
        let out = rewrite_var_refs("${HOST}:${PORT:-3000}/path", &mut vars);
        assert_eq!(out, "{{HOST}}:{{PORT}}/path");
        assert_eq!(vars.get("HOST").unwrap(), "");
        assert_eq!(vars.get("PORT").unwrap(), "3000");
    }

    #[test]
    fn no_vars_unchanged() {
        let mut vars = BTreeMap::new();
        let out = rewrite_var_refs("plain-string", &mut vars);
        assert_eq!(out, "plain-string");
        assert!(vars.is_empty());
    }

    #[test]
    fn first_default_wins() {
        let mut vars = BTreeMap::new();
        rewrite_var_refs("${X:-first}", &mut vars);
        rewrite_var_refs("${X:-second}", &mut vars);
        assert_eq!(vars.get("X").unwrap(), "first");
    }

    #[test]
    fn extract_from_compose_project() {
        use crate::domain::compose::{ComposeProject, ComposeService};

        let mut project = ComposeProject {
            name: Some(String::from("test")),
            services: BTreeMap::from([(
                String::from("app"),
                ComposeService {
                    image: Some(String::from("myapp:${TAG:-latest}")),
                    environment: BTreeMap::from([
                        (String::from("PUID"), String::from("${PUID:-1000}")),
                        (String::from("PGID"), String::from("${PGID}")),
                    ]),
                    ..Default::default()
                },
            )]),
            unsupported: Vec::new(),
            includes: Vec::new(),
        };

        let vars = extract_and_rewrite_variables(&mut project);

        // Variables collected
        assert_eq!(vars.get("TAG").unwrap(), "latest");
        assert_eq!(vars.get("PUID").unwrap(), "1000");
        assert_eq!(vars.get("PGID").unwrap(), "");

        // Strings rewritten
        let svc = project.services.get("app").unwrap();
        assert_eq!(svc.image.as_deref(), Some("myapp:{{TAG}}"));
        assert_eq!(svc.environment.get("PUID").unwrap(), "{{PUID}}");
        assert_eq!(svc.environment.get("PGID").unwrap(), "{{PGID}}");
    }
}
