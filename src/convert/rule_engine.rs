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

    if let Some(cache) = &rules.registry_cache {
        if let Some(image) = &service.image {
            let rewritten = rewrite_image(image, &cache.prefix, &cache.mode);
            if *image != rewritten {
                traces.push(RuleTrace {
                    service: service_name.to_string(),
                    description: format!("Image rewritten: {image} → {rewritten}"),
                });
                service.image = Some(rewritten);
            }
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

fn rewrite_image(image: &str, prefix: &str, mode: &RegistryCacheMode) -> String {
    let normalized = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };

    match mode {
        RegistryCacheMode::Prepend => {
            if image.starts_with(&normalized) {
                image.to_string()
            } else {
                format!("{normalized}{image}")
            }
        }
        RegistryCacheMode::Replace => {
            // Replace everything before the first '/' with the cache prefix
            match image.split_once('/') {
                Some((_, rest)) if rest.contains('/') => {
                    // image like registry.io/org/repo:tag → prefix/org/repo:tag
                    let after_registry = &image[image.find('/').unwrap() + 1..];
                    format!("{normalized}{after_registry}")
                }
                _ => {
                    // Simple image like nginx:latest or org/repo:tag → prefix/image
                    format!("{normalized}{image}")
                }
            }
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

    use super::{apply_rules, service_matches};

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
        };

        let traces = apply_rules("web", &mut service, &rules);

        assert_eq!(service.image.as_deref(), Some("cache.local/nginx:latest"));
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
}
