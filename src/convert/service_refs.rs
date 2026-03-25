use std::collections::{BTreeMap, BTreeSet};

use crate::convert::rule_engine::RuleTrace;
use crate::domain::compose::ComposeProject;

/// A detected reference to another service used as a hostname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRef {
    /// The service that contains the reference.
    pub source_service: String,
    /// The field where it was found (e.g. env key name, "command", "entrypoint").
    pub field: String,
    /// The original full value containing the reference.
    pub original_value: String,
    /// The target service name referenced as hostname.
    pub target_service: String,
}

/// Scan a `ComposeProject` for inter-service hostname references without
/// modifying anything. Returns one entry per (source, field, target) tuple.
pub fn detect_service_references(project: &ComposeProject) -> Vec<DetectedRef> {
    let service_names: BTreeSet<String> = project.services.keys().cloned().collect();
    // Sort longest-first so that e.g. "truc-engines-proxy" is checked before "truc-engines"
    let sorted_names: Vec<&String> = {
        let mut v: Vec<&String> = service_names.iter().collect();
        v.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        v
    };
    let mut refs = Vec::new();

    for (svc_key, service) in &project.services {
        for (env_key, val) in &service.environment {
            for name in &sorted_names {
                if *name == svc_key {
                    continue;
                }
                if contains_hostname_reference(val, name) {
                    refs.push(DetectedRef {
                        source_service: svc_key.clone(),
                        field: format!("env:{env_key}"),
                        original_value: val.clone(),
                        target_service: (*name).clone(),
                    });
                }
            }
        }

        for item in &service.command {
            for name in &sorted_names {
                if *name == svc_key {
                    continue;
                }
                if contains_hostname_reference(item, name) {
                    refs.push(DetectedRef {
                        source_service: svc_key.clone(),
                        field: "command".to_string(),
                        original_value: item.clone(),
                        target_service: (*name).clone(),
                    });
                }
            }
        }

        for item in &service.entrypoint {
            for name in &sorted_names {
                if *name == svc_key {
                    continue;
                }
                if contains_hostname_reference(item, name) {
                    refs.push(DetectedRef {
                        source_service: svc_key.clone(),
                        field: "entrypoint".to_string(),
                        original_value: item.clone(),
                        target_service: (*name).clone(),
                    });
                }
            }
        }
    }

    refs
}

/// Check whether `input` contains at least one hostname reference to `name`.
fn contains_hostname_reference(input: &str, name: &str) -> bool {
    let mut search_from = 0;
    while let Some(pos) = input[search_from..].find(name) {
        let abs_pos = search_from + pos;
        if is_hostname_reference(input, abs_pos, name.len()) {
            return true;
        }
        search_from = abs_pos + name.len();
    }
    false
}

/// Scan all string fields in a `ComposeProject` for references to other
/// service names used as hostnames (e.g. `db:5432`, `http://redis:6379/path`,
/// `mongodb://mongo:27017/mydb`) and replace those service names with the
/// value from `overrides` (or `localhost` when no override is provided).
///
/// If the override value equals the target service name, the reference is kept
/// unchanged (i.e. "keep original" behaviour).
///
/// Returns trace entries describing each replacement.
pub fn rewrite_service_references(
    project: &mut ComposeProject,
    overrides: &BTreeMap<String, String>,
) -> Vec<RuleTrace> {
    let service_names: BTreeSet<String> = project.services.keys().cloned().collect();
    let mut traces = Vec::new();

    let svc_keys: Vec<String> = project.services.keys().cloned().collect();
    for svc_key in &svc_keys {
        let service = project.services.get_mut(svc_key).unwrap();

        // environment values
        let env_keys: Vec<String> = service.environment.keys().cloned().collect();
        for env_key in env_keys {
            if let Some(val) = service.environment.get_mut(&env_key) {
                let rewritten = replace_service_hostnames(val, svc_key, &service_names, overrides);
                if *val != rewritten {
                    let replacement_label = overrides_label(overrides);
                    traces.push(RuleTrace {
                        service: svc_key.clone(),
                        description: format!(
                            "Env `{env_key}`: replaced service reference → {replacement_label} ({val} → {rewritten})"
                        ),
                    });
                    *val = rewritten;
                }
            }
        }

        // command args
        for item in &mut service.command {
            let rewritten = replace_service_hostnames(item, svc_key, &service_names, overrides);
            if *item != rewritten {
                let replacement_label = overrides_label(overrides);
                traces.push(RuleTrace {
                    service: svc_key.clone(),
                    description: format!(
                        "Command arg: replaced service reference → {replacement_label} ({item} → {rewritten})"
                    ),
                });
                *item = rewritten;
            }
        }

        // entrypoint args
        for item in &mut service.entrypoint {
            let rewritten = replace_service_hostnames(item, svc_key, &service_names, overrides);
            if *item != rewritten {
                let replacement_label = overrides_label(overrides);
                traces.push(RuleTrace {
                    service: svc_key.clone(),
                    description: format!(
                        "Entrypoint arg: replaced service reference → {replacement_label} ({item} → {rewritten})"
                    ),
                });
                *item = rewritten;
            }
        }
    }

    traces
}

fn overrides_label(overrides: &BTreeMap<String, String>) -> String {
    if overrides.is_empty() {
        "localhost".to_string()
    } else {
        "user-defined".to_string()
    }
}

/// Replace occurrences of other service names used as hostnames in `input`.
/// Uses the value from `overrides` for each target service, falling back to
/// `localhost`. If the override equals the service name, it's kept unchanged.
fn replace_service_hostnames(
    input: &str,
    own_service: &str,
    service_names: &BTreeSet<String>,
    overrides: &BTreeMap<String, String>,
) -> String {
    let mut result = input.to_string();

    // Sort longest-first so "truc-engines-proxy" is replaced before "truc-engines"
    let mut sorted: Vec<&String> = service_names.iter().collect();
    sorted.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    for name in sorted {
        // Don't replace self-references
        if name == own_service {
            continue;
        }

        let replacement = overrides
            .get(name.as_str())
            .cloned()
            .unwrap_or_else(|| "localhost".to_string());

        // If replacement == service name, the user chose "keep original"
        if replacement == *name {
            continue;
        }

        // Repeatedly scan and replace to handle multiple occurrences
        loop {
            let Some(pos) = result.find(name.as_str()) else {
                break;
            };

            if !is_hostname_reference(&result, pos, name.len()) {
                // Only skip this exact occurrence — search for more after it
                let after = pos + name.len();
                if let Some(next) = result[after..].find(name.as_str()) {
                    let abs_pos = after + next;
                    if is_hostname_reference(&result, abs_pos, name.len()) {
                        result = format!(
                            "{}{}{}",
                            &result[..abs_pos],
                            replacement,
                            &result[abs_pos + name.len()..]
                        );
                        continue;
                    }
                }
                break;
            }

            result = format!(
                "{}{}{}",
                &result[..pos],
                replacement,
                &result[pos + name.len()..]
            );
        }
    }

    result
}

/// Determine whether the substring at `pos` with length `len` looks like a
/// hostname reference in a connection-string / URL context.
///
/// True when:
/// - Preceded by `://`  (scheme)
/// - Preceded by `@`    (user:pass@host)
/// - Followed by `:`    then a digit (host:port)
/// - Followed by `/`    and preceded by `://...` earlier in the string
fn is_hostname_reference(s: &str, pos: usize, len: usize) -> bool {
    let after = pos + len;
    let before = &s[..pos];
    let after_str = &s[after..];

    // The character right after the match must NOT continue the hostname token
    // (e.g. "-" or alphanumeric), otherwise we matched a prefix of a longer name.
    if !is_word_boundary_after(s, after) {
        return false;
    }

    // Preceded by "://"
    if before.ends_with("://") {
        return true;
    }

    // Preceded by "@"
    if before.ends_with('@') {
        return true;
    }

    // Followed by ":" + digit  (host:port pattern)
    if after_str.starts_with(':')
        && let Some(ch) = after_str.chars().nth(1)
        && ch.is_ascii_digit()
    {
        // Also verify the character before the name is a word boundary
        return is_word_boundary_before(s, pos);
    }

    // Followed by "/" and somewhere earlier there's "://"
    if after_str.starts_with('/') && before.contains("://") {
        return true;
    }

    false
}

/// Check that the character after position `pos` is a suitable word boundary
/// (end of string, or not alphanumeric / underscore / hyphen).
fn is_word_boundary_after(s: &str, pos: usize) -> bool {
    if pos >= s.len() {
        return true;
    }
    let next = s.as_bytes()[pos];
    !next.is_ascii_alphanumeric() && next != b'_' && next != b'-'
}

/// Check that the character before position `pos` is a suitable word boundary
/// (start of string, or non-alphanumeric/non-underscore/non-hyphen).
fn is_word_boundary_before(s: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let prev = s.as_bytes()[pos - 1];
    // Allow boundary at: /, @, =, space, comma, ;, [, (, ", '
    !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'-'
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::compose::{ComposeProject, ComposeService};

    fn empty_service() -> ComposeService {
        ComposeService {
            image: None,
            build: None,
            environment: BTreeMap::new(),
            ports: Vec::new(),
            volumes: Vec::new(),
            command: Vec::new(),
            entrypoint: Vec::new(),
            working_dir: None,
            depends_on: Vec::new(),
            post_start: Vec::new(),
        }
    }

    fn project_with(services: Vec<(&str, ComposeService)>) -> ComposeProject {
        ComposeProject {
            name: None,
            services: services
                .into_iter()
                .map(|(n, s)| (n.to_string(), s))
                .collect(),
            unsupported: Vec::new(),
            includes: Vec::new(),
        }
    }

    #[test]
    fn replaces_host_port_in_env() {
        let mut svc = empty_service();
        svc.environment.insert(
            "DATABASE_URL".into(),
            "postgres://user:pass@db:5432/mydb".into(),
        );

        let mut project = project_with(vec![("web", svc), ("db", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["web"].environment["DATABASE_URL"],
            "postgres://user:pass@localhost:5432/mydb"
        );
        assert_eq!(traces.len(), 1);
        assert!(traces[0].description.contains("localhost"));
    }

    #[test]
    fn replaces_scheme_host_in_env() {
        let mut svc = empty_service();
        svc.environment
            .insert("REDIS_URL".into(), "redis://cache:6379".into());

        let mut project = project_with(vec![("app", svc), ("cache", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["app"].environment["REDIS_URL"],
            "redis://localhost:6379"
        );
        assert!(!traces.is_empty());
    }

    #[test]
    fn replaces_bare_host_port_in_env() {
        let mut svc = empty_service();
        svc.environment
            .insert("MONGO_HOST".into(), "mongo:27017".into());

        let mut project = project_with(vec![("api", svc), ("mongo", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["api"].environment["MONGO_HOST"],
            "localhost:27017"
        );
        assert!(!traces.is_empty());
    }

    #[test]
    fn replaces_in_command_args() {
        let mut svc = empty_service();
        svc.command = vec!["--host".into(), "db:5432".into()];

        let mut project = project_with(vec![("worker", svc), ("db", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(project.services["worker"].command[0], "--host");
        assert_eq!(project.services["worker"].command[1], "localhost:5432");
        assert_eq!(traces.len(), 1);
    }

    #[test]
    fn replaces_in_entrypoint_args() {
        let mut svc = empty_service();
        svc.entrypoint = vec!["wait-for-it".into(), "db:5432".into(), "--".into()];

        let mut project = project_with(vec![("app", svc), ("db", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(project.services["app"].entrypoint[1], "localhost:5432");
        assert!(!traces.is_empty());
    }

    #[test]
    fn does_not_replace_self_references() {
        let mut svc = empty_service();
        svc.environment
            .insert("SELF".into(), "http://web:8080".into());

        let mut project = project_with(vec![("web", svc), ("db", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        // "web" should NOT be replaced because it's the service's own name
        assert_eq!(
            project.services["web"].environment["SELF"],
            "http://web:8080"
        );
        assert!(traces.is_empty());
    }

    #[test]
    fn does_not_replace_unrelated_substrings() {
        let mut svc = empty_service();
        svc.environment
            .insert("PATH".into(), "/usr/local/db/bin".into());

        let mut project = project_with(vec![("app", svc), ("db", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        // "db" appears in a path but not as a hostname — should NOT be replaced
        assert_eq!(
            project.services["app"].environment["PATH"],
            "/usr/local/db/bin"
        );
        assert!(traces.is_empty());
    }

    #[test]
    fn handles_url_with_path() {
        let mut svc = empty_service();
        svc.environment
            .insert("API_URL".into(), "http://backend:3000/api/v1".into());

        let mut project = project_with(vec![("frontend", svc), ("backend", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["frontend"].environment["API_URL"],
            "http://localhost:3000/api/v1"
        );
        assert!(!traces.is_empty());
    }

    #[test]
    fn handles_multiple_services_in_one_value() {
        let mut svc = empty_service();
        svc.environment
            .insert("CONNECT".into(), "redis://cache:6379,http://db:5432".into());

        let mut project = project_with(vec![
            ("app", svc),
            ("cache", empty_service()),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["app"].environment["CONNECT"],
            "redis://localhost:6379,http://localhost:5432"
        );
        // One trace per env key (the before→after message captures all replacements)
        assert_eq!(traces.len(), 1);
    }

    #[test]
    fn no_replacement_when_no_other_services() {
        let mut svc = empty_service();
        svc.environment
            .insert("URL".into(), "http://external:9090".into());

        let mut project = project_with(vec![("solo", svc)]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["solo"].environment["URL"],
            "http://external:9090"
        );
        assert!(traces.is_empty());
    }

    #[test]
    fn replaces_at_sign_prefix() {
        let mut svc = empty_service();
        svc.environment
            .insert("AMQP".into(), "amqp://guest:guest@rabbit:5672/%2F".into());

        let mut project = project_with(vec![("worker", svc), ("rabbit", empty_service())]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["worker"].environment["AMQP"],
            "amqp://guest:guest@localhost:5672/%2F"
        );
        assert!(!traces.is_empty());
    }

    #[test]
    fn detect_finds_env_and_command_refs() {
        let mut web = empty_service();
        web.environment
            .insert("DB_URL".into(), "postgres://db:5432/app".into());
        web.command = vec!["--redis".into(), "cache:6379".into()];

        let project = project_with(vec![
            ("web", web),
            ("db", empty_service()),
            ("cache", empty_service()),
        ]);

        let refs = detect_service_references(&project);
        assert_eq!(refs.len(), 2);
        assert!(
            refs.iter()
                .any(|r| r.target_service == "db" && r.field == "env:DB_URL")
        );
        assert!(
            refs.iter()
                .any(|r| r.target_service == "cache" && r.field == "command")
        );
    }

    #[test]
    fn detect_ignores_self_references() {
        let mut svc = empty_service();
        svc.environment
            .insert("ME".into(), "http://web:8080".into());

        let project = project_with(vec![("web", svc)]);
        let refs = detect_service_references(&project);
        assert!(refs.is_empty());
    }

    #[test]
    fn override_custom_value() {
        let mut svc = empty_service();
        svc.environment
            .insert("DB".into(), "postgres://db:5432/x".into());

        let mut project = project_with(vec![("app", svc), ("db", empty_service())]);

        let mut overrides = BTreeMap::new();
        overrides.insert("db".into(), "10.0.0.5".into());
        let traces = rewrite_service_references(&mut project, &overrides);

        assert_eq!(
            project.services["app"].environment["DB"],
            "postgres://10.0.0.5:5432/x"
        );
        assert!(!traces.is_empty());
    }

    #[test]
    fn override_keep_original() {
        let mut svc = empty_service();
        svc.environment
            .insert("URL".into(), "http://backend:3000/api".into());

        let mut project = project_with(vec![("frontend", svc), ("backend", empty_service())]);

        let mut overrides = BTreeMap::new();
        overrides.insert("backend".into(), "backend".into());
        let traces = rewrite_service_references(&mut project, &overrides);

        assert_eq!(
            project.services["frontend"].environment["URL"],
            "http://backend:3000/api"
        );
        assert!(traces.is_empty());
    }

    #[test]
    fn longer_name_not_shadowed_by_prefix_in_detection() {
        let mut svc = empty_service();
        svc.environment
            .insert("PROXY".into(), "http://truc-engines-proxy:8080/api".into());

        let project = project_with(vec![
            ("app", svc),
            ("truc-engines", empty_service()),
            ("truc-engines-proxy", empty_service()),
        ]);

        let refs = detect_service_references(&project);
        // Only truc-engines-proxy should be detected, NOT truc-engines
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].target_service, "truc-engines-proxy");
    }

    #[test]
    fn longer_name_not_shadowed_by_prefix_in_rewrite() {
        let mut svc = empty_service();
        svc.environment
            .insert("PROXY".into(), "http://truc-engines-proxy:8080/api".into());

        let mut project = project_with(vec![
            ("app", svc),
            ("truc-engines", empty_service()),
            ("truc-engines-proxy", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project, &BTreeMap::new());

        assert_eq!(
            project.services["app"].environment["PROXY"],
            "http://localhost:8080/api"
        );
        // Only one replacement for truc-engines-proxy, not truc-engines
        assert_eq!(traces.len(), 1);
        assert!(traces[0].description.contains("truc-engines-proxy"));
    }

    #[test]
    fn both_prefix_and_longer_detected_when_both_referenced() {
        let mut svc = empty_service();
        svc.environment.insert(
            "URLS".into(),
            "http://truc-engines:3000,http://truc-engines-proxy:8080".into(),
        );

        let project = project_with(vec![
            ("app", svc),
            ("truc-engines", empty_service()),
            ("truc-engines-proxy", empty_service()),
        ]);

        let refs = detect_service_references(&project);
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().any(|r| r.target_service == "truc-engines"));
        assert!(
            refs.iter()
                .any(|r| r.target_service == "truc-engines-proxy")
        );
    }
}
