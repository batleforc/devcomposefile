use crate::domain::compose::ComposeService;
use crate::domain::rules::{EnvTranslationRule, RuleSet};

pub fn apply_rules(service_name: &str, service: &mut ComposeService, rules: &RuleSet) {
    if let Some(cache) = &rules.registry_cache {
        if let Some(image) = &service.image {
            service.image = Some(rewrite_image(image, &cache.prefix));
        }
    }

    for rule in &rules.env_translations {
        if service_matches(service_name, rule) {
            apply_env_rule(service, rule);
        }
    }
}

fn rewrite_image(image: &str, prefix: &str) -> String {
    let normalized = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };

    if image.starts_with(&normalized) {
        image.to_string()
    } else {
        format!("{normalized}{image}")
    }
}

fn service_matches(service_name: &str, rule: &EnvTranslationRule) -> bool {
    rule.service == "*" || rule.service == service_name
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
    use crate::domain::rules::{EnvTranslationRule, RegistryCacheRule, RuleSet};

    use super::apply_rules;

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

        apply_rules("web", &mut service, &rules);

        assert_eq!(service.image.as_deref(), Some("cache.local/nginx:latest"));
        assert!(service.environment.get("A").is_none());
        assert_eq!(service.environment.get("B").map(String::as_str), Some("1"));
    }
}