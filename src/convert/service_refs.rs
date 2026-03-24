use std::collections::BTreeSet;

use crate::convert::rule_engine::RuleTrace;
use crate::domain::compose::ComposeProject;

/// Scan all string fields in a `ComposeProject` for references to other
/// service names used as hostnames (e.g. `db:5432`, `http://redis:6379/path`,
/// `mongodb://mongo:27017/mydb`) and replace those service names with
/// `localhost`.
///
/// Returns trace entries describing each replacement.
pub fn rewrite_service_references(project: &mut ComposeProject) -> Vec<RuleTrace> {
    let service_names: BTreeSet<String> = project.services.keys().cloned().collect();
    let mut traces = Vec::new();

    let svc_keys: Vec<String> = project.services.keys().cloned().collect();
    for svc_key in &svc_keys {
        let service = project.services.get_mut(svc_key).unwrap();

        // environment values
        let env_keys: Vec<String> = service.environment.keys().cloned().collect();
        for env_key in env_keys {
            if let Some(val) = service.environment.get_mut(&env_key) {
                let rewritten = replace_service_hostnames(val, &svc_key, &service_names);
                if *val != rewritten {
                    traces.push(RuleTrace {
                        service: svc_key.clone(),
                        description: format!(
                            "Env `{env_key}`: replaced service reference → localhost ({val} → {rewritten})"
                        ),
                    });
                    *val = rewritten;
                }
            }
        }

        // command args
        for item in &mut service.command {
            let rewritten = replace_service_hostnames(item, &svc_key, &service_names);
            if *item != rewritten {
                traces.push(RuleTrace {
                    service: svc_key.clone(),
                    description: format!(
                        "Command arg: replaced service reference → localhost ({item} → {rewritten})"
                    ),
                });
                *item = rewritten;
            }
        }

        // entrypoint args
        for item in &mut service.entrypoint {
            let rewritten = replace_service_hostnames(item, &svc_key, &service_names);
            if *item != rewritten {
                traces.push(RuleTrace {
                    service: svc_key.clone(),
                    description: format!(
                        "Entrypoint arg: replaced service reference → localhost ({item} → {rewritten})"
                    ),
                });
                *item = rewritten;
            }
        }
    }

    traces
}

/// Replace occurrences of other service names used as hostnames in `input`.
/// A service name is considered a hostname when followed by `:digit` or
/// preceded by `://` or `@`, or followed by `/` after a scheme-like prefix.
///
/// We skip references to the service's own name to avoid self-replacement.
fn replace_service_hostnames(
    input: &str,
    own_service: &str,
    service_names: &BTreeSet<String>,
) -> String {
    let mut result = input.to_string();

    for name in service_names {
        // Don't replace self-references
        if name == own_service {
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
                            "{}localhost{}",
                            &result[..abs_pos],
                            &result[abs_pos + name.len()..]
                        );
                        continue;
                    }
                }
                break;
            }

            result = format!(
                "{}localhost{}",
                &result[..pos],
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

    // Preceded by "://"
    if before.ends_with("://") {
        return true;
    }

    // Preceded by "@"
    if before.ends_with('@') {
        return true;
    }

    // Followed by ":" + digit  (host:port pattern)
    if after_str.starts_with(':') {
        if let Some(ch) = after_str.chars().nth(1) {
            if ch.is_ascii_digit() {
                // Also verify the character before the name is a word boundary
                return is_word_boundary_before(s, pos);
            }
        }
    }

    // Followed by "/" and somewhere earlier there's "://"
    if after_str.starts_with('/') && before.contains("://") {
        return true;
    }

    false
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
        svc.environment
            .insert("DATABASE_URL".into(), "postgres://user:pass@db:5432/mydb".into());

        let mut project = project_with(vec![
            ("web", svc),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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

        let mut project = project_with(vec![
            ("app", svc),
            ("cache", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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

        let mut project = project_with(vec![
            ("api", svc),
            ("mongo", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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

        let mut project = project_with(vec![
            ("worker", svc),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

        assert_eq!(project.services["worker"].command[0], "--host");
        assert_eq!(project.services["worker"].command[1], "localhost:5432");
        assert_eq!(traces.len(), 1);
    }

    #[test]
    fn replaces_in_entrypoint_args() {
        let mut svc = empty_service();
        svc.entrypoint = vec!["wait-for-it".into(), "db:5432".into(), "--".into()];

        let mut project = project_with(vec![
            ("app", svc),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

        assert_eq!(project.services["app"].entrypoint[1], "localhost:5432");
        assert!(!traces.is_empty());
    }

    #[test]
    fn does_not_replace_self_references() {
        let mut svc = empty_service();
        svc.environment
            .insert("SELF".into(), "http://web:8080".into());

        let mut project = project_with(vec![
            ("web", svc),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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

        let mut project = project_with(vec![
            ("app", svc),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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

        let mut project = project_with(vec![
            ("frontend", svc),
            ("backend", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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
            .insert(
                "CONNECT".into(),
                "redis://cache:6379,http://db:5432".into(),
            );

        let mut project = project_with(vec![
            ("app", svc),
            ("cache", empty_service()),
            ("db", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

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

        let traces = rewrite_service_references(&mut project);

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
            .insert(
                "AMQP".into(),
                "amqp://guest:guest@rabbit:5672/%2F".into(),
            );

        let mut project = project_with(vec![
            ("worker", svc),
            ("rabbit", empty_service()),
        ]);

        let traces = rewrite_service_references(&mut project);

        assert_eq!(
            project.services["worker"].environment["AMQP"],
            "amqp://guest:guest@localhost:5672/%2F"
        );
        assert!(!traces.is_empty());
    }
}
