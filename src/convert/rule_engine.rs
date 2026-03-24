use crate::domain::compose::ComposeService;
use crate::domain::rules::{EnvTranslationRule, RegistryCacheMode, RuleSet};

/// Trace entry recording which rule was applied to which service.
#[derive(Debug, Clone)]
pub struct RuleTrace {
    pub service: String,
    pub description: String,
}

pub fn apply_rules(
    service_name: &str,
    service: &mut ComposeService,
    rules: &RuleSet,
) -> Vec<RuleTrace> {
    let mut traces = Vec::new();

    if let Some(image) = &service.image {
        let (source_registry, _) = parse_image_parts(image);

        // Check for a specific registry mirror rule first
        let matched_mirror = rules
            .registry_mirrors
            .iter()
            .find(|m| m.source == source_registry);

        let rewritten = if let Some(mirror) = matched_mirror {
            // Mirror rules always use Replace semantics
            rewrite_image(image, &mirror.target, &RegistryCacheMode::Replace)
        } else if let Some(cache) = &rules.registry_cache {
            rewrite_image(image, &cache.prefix, &cache.mode)
        } else {
            image.clone()
        };

        if *image != rewritten {
            traces.push(RuleTrace {
                service: service_name.to_string(),
                description: format!("Image rewritten: {image} → {rewritten}"),
            });
            service.image = Some(rewritten);
        }
    }

    for rule in &rules.env_translations {
        if service_matches(service_name, &rule.service) {
            let desc = describe_env_rule(rule);
            apply_env_rule(service, rule);
            traces.push(RuleTrace {
                service: service_name.to_string(),
                description: desc,
            });
        }
    }

    traces
}

/// Check whether the first segment of an image reference looks like a
/// registry domain (contains `.` or `:`, or is `localhost`).
fn is_registry_domain(segment: &str) -> bool {
    segment.contains('.') || segment.contains(':') || segment == "localhost"
}

/// Parse an image reference into (source_registry, path).
///
/// - `nginx:latest` → `("docker.io", "library/nginx:latest")`
/// - `myorg/myrepo:tag` → `("docker.io", "myorg/myrepo:tag")`
/// - `ghcr.io/org/repo:v1` → `("ghcr.io", "org/repo:v1")`
/// - `localhost:5000/app:1` → `("localhost:5000", "app:1")`
fn parse_image_parts(image: &str) -> (String, String) {
    match image.split_once('/') {
        Some((first, rest)) if is_registry_domain(first) => (first.to_string(), rest.to_string()),
        Some(_) => {
            // org/repo:tag — Docker Hub without explicit registry
            ("docker.io".to_string(), image.to_string())
        }
        None => {
            // bare image like nginx:latest — Docker Hub library
            ("docker.io".to_string(), format!("library/{image}"))
        }
    }
}

fn rewrite_image(image: &str, prefix: &str, mode: &RegistryCacheMode) -> String {
    let normalized = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };

    match mode {
        RegistryCacheMode::Prepend => {
            if image.starts_with(&normalized) {
                return image.to_string();
            }
            // Normalize bare images to include library/ prefix
            let effective = if !image.contains('/') {
                format!("library/{image}")
            } else {
                image.to_string()
            };
            format!("{normalized}{effective}")
        }
        RegistryCacheMode::Replace => {
            let (_, path) = parse_image_parts(image);
            format!("{normalized}{path}")
        }
    }
}

/// Match service name against a pattern that supports `*` as a wildcard.
/// Patterns: `*` matches all, `web*` matches prefix, `*worker` matches suffix,
/// `*mid*` matches contains, exact string matches exactly.
fn service_matches(service_name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let starts_wild = pattern.starts_with('*');
    let ends_wild = pattern.ends_with('*');

    match (starts_wild, ends_wild) {
        (true, true) => {
            let inner = &pattern[1..pattern.len() - 1];
            service_name.contains(inner)
        }
        (true, false) => {
            let suffix = &pattern[1..];
            service_name.ends_with(suffix)
        }
        (false, true) => {
            let prefix = &pattern[..pattern.len() - 1];
            service_name.starts_with(prefix)
        }
        (false, false) => service_name == pattern,
    }
}

fn describe_env_rule(rule: &EnvTranslationRule) -> String {
    let mut parts = Vec::new();
    if let Some(from) = &rule.from {
        if let Some(to) = &rule.to {
            if rule.remove {
                parts.push(format!("Renamed env {from} → {to}"));
            } else {
                parts.push(format!("Copied env {from} → {to}"));
            }
        } else if rule.remove {
            parts.push(format!("Removed env {from}"));
        }
    }
    if !rule.set.is_empty() {
        let keys: Vec<_> = rule.set.keys().collect();
        parts.push(format!(
            "Set env: {}",
            keys.iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join("; ")
}

fn apply_env_rule(service: &mut ComposeService, rule: &EnvTranslationRule) {
    if let Some(from) = &rule.from {
        if let Some(existing) = service.environment.get(from).cloned() {
            if rule.remove {
                service.environment.remove(from);
            }

            if let Some(to) = &rule.to {
                service.environment.insert(to.clone(), existing);
            }
        }
    }

    for (k, v) in &rule.set {
        service.environment.insert(k.clone(), v.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::compose::ComposeService;
    use crate::domain::rules::{EnvTranslationRule, RegistryCacheMode, RegistryCacheRule, RuleSet};

    use super::{apply_rules, parse_image_parts, rewrite_image, service_matches};

    #[test]
    fn rewrites_image_and_env() {
        let mut service = ComposeService {
            image: Some(String::from("nginx:latest")),
            environment: BTreeMap::from([(String::from("A"), String::from("1"))]),
            ..Default::default()
        };

        let rules = RuleSet {
            registry_cache: Some(RegistryCacheRule {
                prefix: String::from("cache.local"),
                mode: RegistryCacheMode::Prepend,
            }),
            env_translations: vec![EnvTranslationRule {
                service: String::from("web"),
                from: Some(String::from("A")),
                to: Some(String::from("B")),
                remove: true,
                set: BTreeMap::new(),
            }],
            base_ide_container: None,
            ..Default::default()
        };

        let traces = apply_rules("web", &mut service, &rules);

        // Bare image gets library/ prefix
        assert_eq!(
            service.image.as_deref(),
            Some("cache.local/library/nginx:latest")
        );
        assert!(service.environment.get("A").is_none());
        assert_eq!(service.environment.get("B").map(String::as_str), Some("1"));
        assert!(traces.len() >= 2);
        assert!(traces[0].description.contains("Image rewritten"));
    }

    #[test]
    fn replace_mode_strips_original_registry() {
        let mut service = ComposeService {
            image: Some(String::from("ghcr.io/org/repo:v1")),
            ..Default::default()
        };

        let rules = RuleSet {
            registry_cache: Some(RegistryCacheRule {
                prefix: String::from("mirror.local"),
                mode: RegistryCacheMode::Replace,
            }),
            ..Default::default()
        };

        apply_rules("svc", &mut service, &rules);
        assert_eq!(service.image.as_deref(), Some("mirror.local/org/repo:v1"));
    }

    #[test]
    fn glob_service_matching() {
        assert!(service_matches("web", "*"));
        assert!(service_matches("web-frontend", "web*"));
        assert!(!service_matches("api", "web*"));
        assert!(service_matches("backend-worker", "*worker"));
        assert!(!service_matches("backend-worker", "*api"));
        assert!(service_matches("my-db-primary", "*db*"));
        assert!(!service_matches("my-cache", "*db*"));
        assert!(service_matches("exact", "exact"));
        assert!(!service_matches("exact", "other"));
    }

    #[test]
    fn parse_image_parts_bare_image() {
        let (reg, path) = parse_image_parts("nginx:latest");
        assert_eq!(reg, "docker.io");
        assert_eq!(path, "library/nginx:latest");
    }

    #[test]
    fn parse_image_parts_org_image() {
        let (reg, path) = parse_image_parts("myorg/myrepo:tag");
        assert_eq!(reg, "docker.io");
        assert_eq!(path, "myorg/myrepo:tag");
    }

    #[test]
    fn parse_image_parts_full_image() {
        let (reg, path) = parse_image_parts("ghcr.io/org/repo:v1");
        assert_eq!(reg, "ghcr.io");
        assert_eq!(path, "org/repo:v1");
    }

    #[test]
    fn parse_image_parts_quay() {
        let (reg, path) = parse_image_parts("quay.io/devfile/udi:latest");
        assert_eq!(reg, "quay.io");
        assert_eq!(path, "devfile/udi:latest");
    }

    #[test]
    fn parse_image_parts_localhost_registry() {
        let (reg, path) = parse_image_parts("localhost:5000/app:1");
        assert_eq!(reg, "localhost:5000");
        assert_eq!(path, "app:1");
    }

    #[test]
    fn prepend_mode_bare_image_includes_library() {
        let result = rewrite_image("postgres:15", "cache.local", &RegistryCacheMode::Prepend);
        assert_eq!(result, "cache.local/library/postgres:15");
    }

    #[test]
    fn prepend_mode_org_image_no_library() {
        let result = rewrite_image(
            "bitnami/redis:7",
            "cache.local",
            &RegistryCacheMode::Prepend,
        );
        assert_eq!(result, "cache.local/bitnami/redis:7");
    }

    #[test]
    fn replace_mode_bare_image_includes_library() {
        let result = rewrite_image("postgres:15", "cache.local", &RegistryCacheMode::Replace);
        assert_eq!(result, "cache.local/library/postgres:15");
    }

    #[test]
    fn replace_mode_org_image() {
        let result = rewrite_image(
            "bitnami/redis:7",
            "cache.local",
            &RegistryCacheMode::Replace,
        );
        assert_eq!(result, "cache.local/bitnami/redis:7");
    }

    #[test]
    fn mirror_rule_overrides_generic_cache() {
        use crate::domain::rules::RegistryMirrorRule;

        let mut service = ComposeService {
            image: Some(String::from("ghcr.io/org/repo:v1")),
            ..Default::default()
        };

        let rules = RuleSet {
            registry_cache: Some(RegistryCacheRule {
                prefix: String::from("generic-cache.local"),
                mode: RegistryCacheMode::Prepend,
            }),
            registry_mirrors: vec![RegistryMirrorRule {
                source: String::from("ghcr.io"),
                target: String::from("ghcr-cache.local"),
            }],
            ..Default::default()
        };

        apply_rules("svc", &mut service, &rules);
        // Mirror takes precedence over generic cache
        assert_eq!(
            service.image.as_deref(),
            Some("ghcr-cache.local/org/repo:v1")
        );
    }

    #[test]
    fn mirror_rule_docker_hub_bare_image() {
        use crate::domain::rules::RegistryMirrorRule;

        let mut service = ComposeService {
            image: Some(String::from("nginx:latest")),
            ..Default::default()
        };

        let rules = RuleSet {
            registry_mirrors: vec![RegistryMirrorRule {
                source: String::from("docker.io"),
                target: String::from("dockerhub-cache.local"),
            }],
            ..Default::default()
        };

        apply_rules("svc", &mut service, &rules);
        assert_eq!(
            service.image.as_deref(),
            Some("dockerhub-cache.local/library/nginx:latest")
        );
    }

    #[test]
    fn unmatched_registry_falls_back_to_generic_cache() {
        use crate::domain::rules::RegistryMirrorRule;

        let mut service = ComposeService {
            image: Some(String::from("quay.io/devfile/udi:latest")),
            ..Default::default()
        };

        let rules = RuleSet {
            registry_cache: Some(RegistryCacheRule {
                prefix: String::from("generic-cache.local"),
                mode: RegistryCacheMode::Prepend,
            }),
            registry_mirrors: vec![RegistryMirrorRule {
                source: String::from("ghcr.io"),
                target: String::from("ghcr-cache.local"),
            }],
            ..Default::default()
        };

        apply_rules("svc", &mut service, &rules);
        // No mirror for quay.io → falls back to generic cache (prepend)
        assert_eq!(
            service.image.as_deref(),
            Some("generic-cache.local/quay.io/devfile/udi:latest")
        );
    }
}
