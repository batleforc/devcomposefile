use std::collections::BTreeMap;

use devcomposefile::convert::include_resolver::{IncludeContext, resolve_includes};
use devcomposefile::convert::merge::merge_projects;
use devcomposefile::convert::transform::convert_to_devfile;
use devcomposefile::domain::compose::parse_compose_documents;
use devcomposefile::domain::devfile::ComponentSpec;
use devcomposefile::domain::git_fetch::{GitProvider, RepoRef, parse_repo_url, raw_content_url};
use devcomposefile::domain::rules::{
    EnvTranslationRule, ParentDevfileRule, RegistryCacheMode, RegistryCacheRule, RuleSet,
    load_default_rules, load_rules_from_json, merge_rules,
};

#[test]
fn merges_compose_documents_and_generates_expected_devfile_shape() {
    let compose_input = concat!(
        include_str!("fixtures/compose-base.yml"),
        "\n---\n",
        include_str!("fixtures/compose-override.yml")
    );
    let projects = parse_compose_documents(compose_input).expect("compose parses");
    let merged = merge_projects(projects);

    let rules = merge_rules(
        &merge_rules(
            &load_default_rules().expect("default rules"),
            &load_rules_from_json(include_str!("../assets/rules/startup-rules.json"))
                .expect("startup rules"),
        ),
        &load_rules_from_json(include_str!("fixtures/runtime-rules.json")).expect("runtime rules"),
    );

    let result = convert_to_devfile(
        merged,
        rules,
        Some(String::from("quay.io/devfile/custom-udi:latest")),
    );

    let expected: serde_yaml::Value =
        serde_yaml::from_str(include_str!("fixtures/expected-devfile.yml")).expect("expected yaml");
    let actual: serde_yaml::Value =
        serde_yaml::to_value(&result.devfile).expect("actual yaml value");

    assert_eq!(actual, expected);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.contains("unsupported top-level key `volumes`"))
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.contains("unsupported key `healthcheck`"))
    );
}

#[test]
fn glob_rules_and_replace_mode_and_debug_commands() {
    let compose = r#"
services:
  web-frontend:
    image: docker.io/org/frontend:latest
    environment:
      LOG_LEVEL: debug
    command: ["npm", "start"]
    ports:
      - "3000:3000"
  web-backend:
    image: docker.io/org/backend:latest
    environment:
      LOG_LEVEL: info
    command: ["python", "app.py"]
  db:
    image: postgres:15
    ports:
      - "5432"
"#;

    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        registry_cache: Some(RegistryCacheRule {
            prefix: String::from("cache.corp"),
            mode: RegistryCacheMode::Replace,
        }),
        env_translations: vec![EnvTranslationRule {
            service: String::from("web*"),
            from: Some(String::from("LOG_LEVEL")),
            to: Some(String::from("APP_LOG_LEVEL")),
            remove: true,
            set: std::collections::BTreeMap::new(),
        }],
        ..Default::default()
    };

    let result = convert_to_devfile(merged, rules, None);

    // Check replace-mode image rewrite strips original registry
    let frontend = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "web-frontend")
        .expect("frontend");
    if let ComponentSpec::Container(ref c) = frontend.spec {
        assert_eq!(c.image, "cache.corp/org/frontend:latest");
        // Glob matched "web*": LOG_LEVEL removed, APP_LOG_LEVEL set
        assert!(
            c.env
                .iter()
                .any(|e| e.name == "APP_LOG_LEVEL" && e.value == "debug")
        );
        assert!(!c.env.iter().any(|e| e.name == "LOG_LEVEL"));
        // Port 3000 with host mapping → public exposure
        assert_eq!(c.endpoints[0].exposure.as_deref(), Some("public"));
    } else {
        panic!("expected container");
    }

    // Backend matched by "web*" glob too
    let backend = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "web-backend")
        .expect("backend");
    if let ComponentSpec::Container(ref c) = backend.spec {
        assert_eq!(c.image, "cache.corp/org/backend:latest");
        assert!(
            c.env
                .iter()
                .any(|e| e.name == "APP_LOG_LEVEL" && e.value == "info")
        );
        assert!(!c.env.iter().any(|e| e.name == "LOG_LEVEL"));
    } else {
        panic!("expected container");
    }

    // db NOT matched by "web*" glob — LOG_LEVEL would be kept if it existed
    let db = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "db")
        .expect("db");
    if let ComponentSpec::Container(ref c) = db.spec {
        // Bare image (no namespace) gets library/ prefix in Replace mode
        assert_eq!(c.image, "cache.corp/library/postgres:15");
        // Container-only port (no host) → internal exposure
        assert_eq!(c.endpoints[0].exposure.as_deref(), Some("internal"));
    } else {
        panic!("expected container");
    }

    // Services whose command is already part of the container should
    // NOT produce run/debug commands or postStart events.
    assert!(
        !result
            .devfile
            .commands
            .iter()
            .any(|c| c.id == "run-web-frontend"),
        "run-web-frontend should not exist — command already on container"
    );
    assert!(
        !result
            .devfile
            .commands
            .iter()
            .any(|c| c.id == "debug-web-frontend"),
        "debug-web-frontend should not exist"
    );
    assert!(
        !result
            .devfile
            .commands
            .iter()
            .any(|c| c.id == "run-web-backend"),
        "run-web-backend should not exist — command already on container"
    );
    assert!(
        !result
            .devfile
            .commands
            .iter()
            .any(|c| c.id == "debug-web-backend"),
        "debug-web-backend should not exist"
    );

    // Rule traces recorded
    assert!(!result.rule_traces.is_empty());
    assert!(
        result
            .rule_traces
            .iter()
            .any(|t| t.description.contains("Image rewritten"))
    );
}

#[test]
fn rules_merge_precedence_runtime_overrides_startup_and_defaults() {
    // Default rules now only contain baseIdeContainer (no registryCache, no envTranslations)
    let defaults = load_default_rules().expect("default rules");
    assert!(defaults.registry_cache.is_none());
    assert!(defaults.env_translations.is_empty());
    assert!(defaults.base_ide_container.is_some());

    // Startup rules have env translations but no registryCache override
    let startup =
        load_rules_from_json(include_str!("../assets/rules/startup-rules.json")).expect("startup");
    let after_startup = merge_rules(&defaults, &startup);
    // registryCache still absent
    assert!(after_startup.registry_cache.is_none());
    // Env translations from startup accumulated
    assert_eq!(
        after_startup.env_translations.len(),
        startup.env_translations.len()
    );

    // Runtime rules override registryCache entirely
    let runtime = RuleSet {
        registry_cache: Some(RegistryCacheRule {
            prefix: String::from("runtime-cache.example"),
            mode: RegistryCacheMode::Replace,
        }),
        ..Default::default()
    };
    let final_rules = merge_rules(&after_startup, &runtime);
    assert_eq!(
        final_rules.registry_cache.as_ref().unwrap().prefix,
        "runtime-cache.example"
    );
    assert!(matches!(
        final_rules.registry_cache.as_ref().unwrap().mode,
        RegistryCacheMode::Replace
    ));
    // Env translations from startup still present (runtime had none)
    assert_eq!(
        final_rules.env_translations.len(),
        after_startup.env_translations.len()
    );
}

#[test]
fn ide_container_runtime_override_beats_rules() {
    let compose = r#"
services:
  app:
    image: myapp:latest
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        base_ide_container: Some(devcomposefile::domain::rules::IdeContainerRule {
            name: String::from("ide"),
            image: String::from("quay.io/devfile/udi:latest"),
            memory_limit: Some(String::from("4Gi")),
        }),
        ..Default::default()
    };

    // With runtime override: runtime image wins, but memory_limit from rules is kept
    let result = convert_to_devfile(merged, rules.clone(), Some(String::from("custom-ide:v2")));
    let ide = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "ide")
        .expect("ide component");
    if let ComponentSpec::Container(ref c) = ide.spec {
        assert_eq!(c.image, "custom-ide:v2");
        assert_eq!(c.memory_limit.as_deref(), Some("4Gi"));
    } else {
        panic!("expected container");
    }
    // Tool container must be first in components list
    assert_eq!(result.devfile.components[0].name, "ide");

    // Without runtime override: rule image used
    let projects2 = parse_compose_documents(compose).expect("parse");
    let merged2 = merge_projects(projects2);
    let result2 = convert_to_devfile(merged2, rules, None);
    let ide2 = result2
        .devfile
        .components
        .iter()
        .find(|c| c.name == "ide")
        .expect("ide component");
    if let ComponentSpec::Container(ref c) = ide2.spec {
        assert_eq!(c.image, "quay.io/devfile/udi:latest");
    } else {
        panic!("expected container");
    }
}

#[test]
fn no_ide_container_when_neither_rules_nor_override() {
    let compose = r#"
services:
  app:
    image: myapp:latest
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let result = convert_to_devfile(merged, RuleSet::default(), None);
    // No tool component generated
    assert!(
        !result
            .devfile
            .components
            .iter()
            .any(|c| c.name == "tool" || c.name == "tool-base")
    );
    // Diagnostic warns about missing tool container
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.contains("No tool container"))
    );
}

#[test]
fn empty_services_produces_diagnostic() {
    let compose = "services: {}";
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);
    // Validation should flag no components (only missing IDE diagnostic will appear)
    assert!(result.devfile.components.is_empty());
}

#[test]
fn build_only_service_is_skipped_with_diagnostic() {
    let compose = r#"
services:
  builder:
    build:
      context: .
      dockerfile: Dockerfile
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.contains("builder") && d.contains("no image"))
    );
    assert!(
        !result
            .devfile
            .components
            .iter()
            .any(|c| c.name == "builder")
    );
}

#[test]
fn list_format_environment_parsed_correctly() {
    let compose = r#"
services:
  app:
    image: myapp:latest
    environment:
      - FOO=bar
      - BAZ=qux
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let svc = projects[0].services.get("app").expect("app service");
    assert_eq!(svc.environment.get("FOO").map(String::as_str), Some("bar"));
    assert_eq!(svc.environment.get("BAZ").map(String::as_str), Some("qux"));
}

#[test]
fn depends_on_mapping_form_parsed() {
    let compose = r#"
services:
  web:
    image: nginx
    depends_on:
      db:
        condition: service_healthy
      redis:
        condition: service_started
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let web = projects[0].services.get("web").expect("web service");
    assert!(web.depends_on.contains(&String::from("db")));
    assert!(web.depends_on.contains(&String::from("redis")));
}

#[test]
fn cumulative_env_translations_apply_in_order() {
    let compose = r#"
services:
  app:
    image: myapp:latest
    environment:
      A: "1"
      B: "2"
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        env_translations: vec![
            // First rule: rename A → C and remove A
            EnvTranslationRule {
                service: String::from("*"),
                from: Some(String::from("A")),
                to: Some(String::from("C")),
                remove: true,
                set: std::collections::BTreeMap::new(),
            },
            // Second rule: set D=4
            EnvTranslationRule {
                service: String::from("*"),
                from: None,
                to: None,
                remove: false,
                set: std::collections::BTreeMap::from([(String::from("D"), String::from("4"))]),
            },
        ],
        ..Default::default()
    };

    let result = convert_to_devfile(merged, rules, None);
    let app = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "app")
        .expect("app component");
    if let ComponentSpec::Container(ref c) = app.spec {
        // A should be gone, C should have A's value
        assert!(!c.env.iter().any(|e| e.name == "A"));
        assert!(c.env.iter().any(|e| e.name == "C" && e.value == "1"));
        assert!(c.env.iter().any(|e| e.name == "B" && e.value == "2"));
        assert!(c.env.iter().any(|e| e.name == "D" && e.value == "4"));
    } else {
        panic!("expected container");
    }
}

#[test]
fn git_url_to_raw_content_end_to_end() {
    // GitHub: full blob URL → correct raw URL
    let r = parse_repo_url(
        "https://github.com/docker/awesome-compose/blob/master/react-express-mysql/compose.yaml",
        None,
        None,
    )
    .unwrap();
    assert_eq!(r.provider, GitProvider::GitHub);
    assert_eq!(r.owner, "docker");
    assert_eq!(r.repo, "awesome-compose");
    assert_eq!(r.git_ref, "master");
    assert_eq!(r.path, "react-express-mysql/compose.yaml");
    assert_eq!(
        raw_content_url(&r),
        "https://raw.githubusercontent.com/docker/awesome-compose/master/react-express-mysql/compose.yaml"
    );

    // GitLab: simple repo URL with overrides
    let r = parse_repo_url(
        "https://gitlab.com/team/infra",
        Some("release/1.0"),
        Some("deploy/docker-compose.yml"),
    )
    .unwrap();
    assert_eq!(r.provider, GitProvider::GitLab);
    assert_eq!(r.git_ref, "release/1.0");
    assert_eq!(
        raw_content_url(&r),
        "https://gitlab.com/team/infra/-/raw/release/1.0/deploy/docker-compose.yml"
    );

    // Bitbucket: simple URL with defaults
    let r = parse_repo_url("https://bitbucket.org/org/service", None, None).unwrap();
    assert_eq!(r.provider, GitProvider::Bitbucket);
    assert_eq!(r.git_ref, "main");
    assert_eq!(r.path, "docker-compose.yml");
    assert_eq!(
        raw_content_url(&r),
        "https://bitbucket.org/org/service/raw/main/docker-compose.yml"
    );
}

#[test]
fn include_resolved_from_file_registry_and_merged() {
    let main_yaml = r#"
include:
  - db.yml
services:
  web:
    image: nginx:latest
    environment:
      APP_ENV: production
"#;

    let db_yaml = r#"
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_DB: mydb
"#;

    let docs = parse_compose_documents(main_yaml).expect("parses");
    assert_eq!(docs[0].includes.len(), 1);

    let mut registry = BTreeMap::new();
    registry.insert(String::from("db.yml"), db_yaml.to_string());

    let resolution = resolve_includes(docs, &IncludeContext::Local, &registry);
    assert!(resolution.pending_fetches.is_empty());

    let merged = merge_projects(resolution.projects);
    assert!(merged.services.contains_key("web"));
    assert!(merged.services.contains_key("db"));
    assert_eq!(merged.services["db"].image.as_deref(), Some("postgres:16"));
}

#[test]
fn include_main_overrides_included_service() {
    let main_yaml = r#"
include:
  - base.yml
services:
  api:
    image: api:v2
    environment:
      LOG_LEVEL: debug
"#;

    let base_yaml = r#"
services:
  api:
    image: api:v1
    environment:
      LOG_LEVEL: info
      DATABASE_URL: postgres://localhost/db
"#;

    let docs = parse_compose_documents(main_yaml).expect("parses");
    let mut registry = BTreeMap::new();
    registry.insert(String::from("base.yml"), base_yaml.to_string());

    let resolution = resolve_includes(docs, &IncludeContext::Local, &registry);
    let merged = merge_projects(resolution.projects);

    let api = &merged.services["api"];
    // Main overrides image
    assert_eq!(api.image.as_deref(), Some("api:v2"));
    // Main overrides LOG_LEVEL, but inherited DATABASE_URL from base
    assert_eq!(api.environment["LOG_LEVEL"], "debug");
    assert_eq!(api.environment["DATABASE_URL"], "postgres://localhost/db");
}

#[test]
fn include_full_pipeline_with_rules() {
    let main_yaml = r#"
include:
  - infra/db.yml
services:
  web:
    image: nginx:latest
    ports:
      - "8080:80"
"#;

    let db_yaml = r#"
services:
  db:
    image: postgres:16
    ports:
      - "5432:5432"
"#;

    let docs = parse_compose_documents(main_yaml).expect("parses");
    let mut registry = BTreeMap::new();
    registry.insert(String::from("infra/db.yml"), db_yaml.to_string());

    let resolution = resolve_includes(docs, &IncludeContext::Local, &registry);
    let merged = merge_projects(resolution.projects);

    let rules = RuleSet {
        registry_cache: Some(RegistryCacheRule {
            prefix: String::from("cache.local"),
            mode: RegistryCacheMode::Prepend,
        }),
        ..Default::default()
    };

    let result = convert_to_devfile(merged, rules, None);
    // Both services should be converted with the registry cache applied
    let web = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "web")
        .expect("web component");
    let db = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "db")
        .expect("db component");

    match &web.spec {
        ComponentSpec::Container(c) => assert!(c.image.starts_with("cache.local/")),
        _ => panic!("expected container"),
    }
    match &db.spec {
        ComponentSpec::Container(c) => assert!(c.image.starts_with("cache.local/")),
        _ => panic!("expected container"),
    }
}

#[test]
fn git_context_produces_pending_fetch_for_missing_include() {
    let main_yaml = r#"
include:
  - ./monitoring/prometheus.yml
services:
  web:
    image: nginx
"#;

    let docs = parse_compose_documents(main_yaml).expect("parses");
    let context = IncludeContext::Git(RepoRef {
        provider: GitProvider::GitHub,
        owner: String::from("acme"),
        repo: String::from("app"),
        git_ref: String::from("main"),
        path: String::from("docker-compose.yml"),
    });

    let resolution = resolve_includes(docs, &context, &BTreeMap::new());
    assert_eq!(resolution.pending_fetches.len(), 1);
    assert!(
        resolution.pending_fetches[0]
            .raw_url
            .contains("monitoring/prometheus.yml")
    );
    assert!(
        resolution.pending_fetches[0]
            .raw_url
            .contains("raw.githubusercontent.com")
    );
}

#[test]
fn nested_includes_resolved_depth_first() {
    let main_yaml = r#"
include:
  - a.yml
services:
  main:
    image: main:latest
"#;

    let a_yaml = r#"
include:
  - b.yml
services:
  svc_a:
    image: a:latest
"#;

    let b_yaml = r#"
services:
  svc_b:
    image: b:latest
"#;

    let docs = parse_compose_documents(main_yaml).expect("parses");
    let mut registry = BTreeMap::new();
    registry.insert(String::from("a.yml"), a_yaml.to_string());
    registry.insert(String::from("b.yml"), b_yaml.to_string());

    let resolution = resolve_includes(docs, &IncludeContext::Local, &registry);
    assert!(resolution.pending_fetches.is_empty());

    let merged = merge_projects(resolution.projects);
    assert!(merged.services.contains_key("main"));
    assert!(merged.services.contains_key("svc_a"));
    assert!(merged.services.contains_key("svc_b"));
}

#[test]
fn env_variable_references_become_devfile_variables() {
    let yaml = r#"
services:
  app:
    image: myapp:${TAG:-latest}
    environment:
      PUID: "${PUID:-1000}"
      PGID: "${PGID}"
      STATIC: "no-vars-here"
    command: ["run", "--port=${PORT:-8080}"]
"#;
    let projects = parse_compose_documents(yaml).expect("parses");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    // Variables map should have all extracted vars with defaults
    assert_eq!(result.devfile.variables.get("TAG").unwrap(), "latest");
    assert_eq!(result.devfile.variables.get("PUID").unwrap(), "1000");
    assert_eq!(result.devfile.variables.get("PGID").unwrap(), "");
    assert_eq!(result.devfile.variables.get("PORT").unwrap(), "8080");

    // Component values should use {{VAR}} syntax
    let app = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "app")
        .unwrap();
    if let ComponentSpec::Container(ref ctr) = app.spec {
        assert_eq!(ctr.image, "myapp:{{TAG}}");
        assert!(
            ctr.env
                .iter()
                .any(|e| e.name == "PUID" && e.value == "{{PUID}}")
        );
        assert!(
            ctr.env
                .iter()
                .any(|e| e.name == "PGID" && e.value == "{{PGID}}")
        );
        assert!(
            ctr.env
                .iter()
                .any(|e| e.name == "STATIC" && e.value == "no-vars-here")
        );
        assert_eq!(ctr.args.as_ref().unwrap()[1], "--port={{PORT}}");
    } else {
        panic!("expected container");
    }

    // The variables section should appear in serialized YAML
    let yaml_out = serde_yaml::to_string(&result.devfile).unwrap();
    assert!(yaml_out.contains("variables:"));
    assert!(yaml_out.contains("TAG: latest"));
    assert!(yaml_out.contains("PUID: '1000'"));
}

#[test]
fn service_references_replaced_with_localhost() {
    let yaml = r#"
services:
  web:
    image: myapp:latest
    environment:
      DATABASE_URL: "postgres://user:pass@db:5432/mydb"
      REDIS_URL: "redis://cache:6379"
      SELF_REF: "http://web:8080"
    command: ["--db-host", "db:5432"]
  db:
    image: postgres:16
  cache:
    image: redis:7
"#;
    let projects = parse_compose_documents(yaml).expect("parses");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    let web = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "web")
        .unwrap();
    if let ComponentSpec::Container(ref ctr) = web.spec {
        // db and cache references should be replaced with localhost
        assert!(
            ctr.env.iter().any(|e| e.name == "DATABASE_URL"
                && e.value == "postgres://user:pass@localhost:5432/mydb")
        );
        assert!(
            ctr.env
                .iter()
                .any(|e| e.name == "REDIS_URL" && e.value == "redis://localhost:6379")
        );
        // Self-reference should NOT be replaced
        assert!(
            ctr.env
                .iter()
                .any(|e| e.name == "SELF_REF" && e.value == "http://web:8080")
        );
        // Command args should also be rewritten
        assert_eq!(ctr.args.as_ref().unwrap()[1], "localhost:5432");
    } else {
        panic!("expected container");
    }

    // Rule traces should document the replacements
    assert!(
        result
            .rule_traces
            .iter()
            .any(|t| t.description.contains("localhost"))
    );
}

#[test]
fn duplicate_endpoint_ports_get_host_prefix() {
    let yaml = r#"
services:
  frontend:
    image: nginx:latest
    ports:
      - "8080:3000"
  backend:
    image: node:20
    ports:
      - "9090:3000"
  db:
    image: postgres:16
    ports:
      - "5432:5432"
"#;
    let projects = parse_compose_documents(yaml).expect("parses");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    let frontend = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "frontend")
        .unwrap();
    let backend = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "backend")
        .unwrap();
    let db = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "db")
        .unwrap();

    // Duplicate container port 3000 → prefixed with host port
    if let ComponentSpec::Container(ref ctr) = frontend.spec {
        assert_eq!(ctr.endpoints[0].name, "port-8080-3000");
        assert_eq!(ctr.endpoints[0].target_port, 3000);
    } else {
        panic!("expected container");
    }
    if let ComponentSpec::Container(ref ctr) = backend.spec {
        assert_eq!(ctr.endpoints[0].name, "port-9090-3000");
        assert_eq!(ctr.endpoints[0].target_port, 3000);
    } else {
        panic!("expected container");
    }
    // Unique port 5432 → no prefix
    if let ComponentSpec::Container(ref ctr) = db.spec {
        assert_eq!(ctr.endpoints[0].name, "port-5432");
        assert_eq!(ctr.endpoints[0].target_port, 5432);
    } else {
        panic!("expected container");
    }
}

#[test]
fn parent_devfile_used_instead_of_inline_ide_container() {
    let compose = r#"
services:
  app:
    image: myapp:latest
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        parent_devfile: Some(ParentDevfileRule {
            id: Some(String::from("udi")),
            registry_url: Some(String::from("https://registry.devfile.io")),
            uri: None,
            version: Some(String::from("2.2.0")),
        }),
        ..Default::default()
    };

    let result = convert_to_devfile(merged, rules, None);

    // Parent should be set
    let parent = result
        .devfile
        .parent
        .as_ref()
        .expect("parent should be set");
    assert_eq!(parent.id.as_deref(), Some("udi"));
    assert_eq!(
        parent.registry_url.as_deref(),
        Some("https://registry.devfile.io")
    );
    assert_eq!(parent.version.as_deref(), Some("2.2.0"));
    assert!(parent.uri.is_none());

    // No inline IDE container should be inserted
    assert!(
        !result
            .devfile
            .components
            .iter()
            .any(|c| c.name == "tool" || c.name == "ide")
    );

    // Service component should still be present
    assert!(result.devfile.components.iter().any(|c| c.name == "app"));

    // No diagnostic about missing tool container
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.contains("No tool container"))
    );
}

#[test]
fn parent_devfile_uri_mode() {
    let compose = r#"
services:
  app:
    image: myapp:latest
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        parent_devfile: Some(ParentDevfileRule {
            id: None,
            registry_url: None,
            uri: Some(String::from(
                "https://raw.githubusercontent.com/org/repo/main/devfile.yaml",
            )),
            version: None,
        }),
        ..Default::default()
    };

    let result = convert_to_devfile(merged, rules, None);

    let parent = result
        .devfile
        .parent
        .as_ref()
        .expect("parent should be set");
    assert!(parent.id.is_none());
    assert_eq!(
        parent.uri.as_deref(),
        Some("https://raw.githubusercontent.com/org/repo/main/devfile.yaml")
    );
}

#[test]
fn ide_image_override_takes_precedence_over_parent_devfile() {
    let compose = r#"
services:
  app:
    image: myapp:latest
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        parent_devfile: Some(ParentDevfileRule {
            id: Some(String::from("udi")),
            registry_url: Some(String::from("https://registry.devfile.io")),
            uri: None,
            version: None,
        }),
        ..Default::default()
    };

    // When ide_image_override is provided, it wins over parent devfile
    let result = convert_to_devfile(
        merged,
        rules,
        Some(String::from("quay.io/devfile/custom-udi:latest")),
    );

    // No parent — inline IDE container used instead
    assert!(result.devfile.parent.is_none());
    assert!(result.devfile.components.iter().any(|c| c.name == "tool"));
}

#[test]
fn parent_devfile_serialized_in_yaml_output() {
    let compose = r#"
services:
  web:
    image: nginx:latest
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);

    let rules = RuleSet {
        parent_devfile: Some(ParentDevfileRule {
            id: Some(String::from("udi")),
            registry_url: Some(String::from("https://registry.devfile.io")),
            uri: None,
            version: None,
        }),
        ..Default::default()
    };

    let result = convert_to_devfile(merged, rules, None);
    let yaml = serde_yaml::to_string(&result.devfile).expect("serialize");

    assert!(yaml.contains("parent:"));
    assert!(yaml.contains("id: udi"));
    assert!(yaml.contains("registryUrl: https://registry.devfile.io"));
    // No parent.uri since it was None
    assert!(!yaml.contains("uri:"));
}

#[test]
fn parent_devfile_rule_merges_correctly() {
    let base = RuleSet {
        base_ide_container: Some(devcomposefile::domain::rules::IdeContainerRule {
            name: String::from("tool"),
            image: String::from("quay.io/devfile/udi:latest"),
            memory_limit: None,
        }),
        ..Default::default()
    };

    let extra = RuleSet {
        parent_devfile: Some(ParentDevfileRule {
            id: Some(String::from("udi")),
            registry_url: None,
            uri: None,
            version: None,
        }),
        ..Default::default()
    };

    let merged = merge_rules(&base, &extra);
    // Parent devfile from extra is merged
    assert!(merged.parent_devfile.is_some());
    assert_eq!(
        merged.parent_devfile.as_ref().unwrap().id.as_deref(),
        Some("udi")
    );
    // Base IDE container is still present (not cleared by parent rule)
    assert!(merged.base_ide_container.is_some());
}

#[test]
fn depends_on_moves_command_to_devfile_command_and_idles_container() {
    let compose = r#"
services:
  db:
    image: postgres:16
    ports:
      - "5432:5432"
  app:
    image: node:20
    command: ["npm", "start"]
    entrypoint: ["/bin/sh", "-c"]
    working_dir: /workspace
    depends_on:
      - db
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    // app container should idle with tail -f /dev/null
    let app = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "app")
        .expect("app component");
    if let ComponentSpec::Container(ref ctr) = app.spec {
        assert_eq!(ctr.command.as_deref(), Some(&[String::from("tail")][..]));
        assert_eq!(
            ctr.args.as_deref(),
            Some(&[String::from("-f"), String::from("/dev/null")][..])
        );
    } else {
        panic!("expected container");
    }

    // A run-app command should exist with the original entrypoint + command
    let cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "run-app")
        .expect("run-app command");
    assert_eq!(cmd.exec.component, "app");
    assert_eq!(cmd.exec.command_line, "/bin/sh -c npm start");
    assert_eq!(cmd.exec.working_dir.as_deref(), Some("/workspace"));

    // postStart events should reference the command
    let events = result.devfile.events.as_ref().expect("events");
    assert!(events.post_start.contains(&String::from("run-app")));

    // db has no depends_on, so its container should NOT have tail -f /dev/null
    let db = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "db")
        .expect("db component");
    if let ComponentSpec::Container(ref ctr) = db.spec {
        assert!(ctr.command.is_none());
        assert!(ctr.args.is_none());
    } else {
        panic!("expected container");
    }
}

#[test]
fn depends_on_without_command_does_not_create_run_command() {
    let compose = r#"
services:
  db:
    image: postgres:16
  app:
    image: node:20
    depends_on:
      - db
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    // app has depends_on but no command/entrypoint — no idle override needed
    let app = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "app")
        .expect("app component");
    if let ComponentSpec::Container(ref ctr) = app.spec {
        assert!(ctr.command.is_none());
        assert!(ctr.args.is_none());
    } else {
        panic!("expected container");
    }

    // No commands generated
    assert!(result.devfile.commands.is_empty());
}

#[test]
fn depends_on_entrypoint_only_creates_command() {
    let compose = r#"
services:
  redis:
    image: redis:7
  worker:
    image: python:3.12
    entrypoint: ["python", "worker.py"]
    depends_on:
      - redis
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    // worker container idles
    let worker = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "worker")
        .expect("worker component");
    if let ComponentSpec::Container(ref ctr) = worker.spec {
        assert_eq!(ctr.command.as_deref(), Some(&[String::from("tail")][..]));
        assert_eq!(
            ctr.args.as_deref(),
            Some(&[String::from("-f"), String::from("/dev/null")][..])
        );
    } else {
        panic!("expected container");
    }

    // run-worker command has the entrypoint
    let cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "run-worker")
        .expect("run-worker command");
    assert_eq!(cmd.exec.command_line, "python worker.py");
}

#[test]
fn post_start_creates_devfile_command() {
    let compose = r#"
services:
  app:
    image: node:20
    post_start: ["npm", "run", "migrate"]
    working_dir: /workspace
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    // A post-start-app command should exist
    let cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "post-start-app")
        .expect("post-start-app command");
    assert_eq!(cmd.exec.component, "app");
    assert_eq!(cmd.exec.command_line, "npm run migrate");
    assert_eq!(cmd.exec.working_dir.as_deref(), Some("/workspace"));

    // postStart events should reference the command
    let events = result.devfile.events.as_ref().expect("events");
    assert!(events.post_start.contains(&String::from("post-start-app")));

    // Container should NOT be idling (no depends_on)
    let app = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "app")
        .expect("app component");
    if let ComponentSpec::Container(ref ctr) = app.spec {
        assert!(ctr.command.is_none());
        assert!(ctr.args.is_none());
    } else {
        panic!("expected container");
    }

    // Verify the command appears in YAML output
    let yaml_out = serde_yaml::to_string(&result.devfile).unwrap();
    assert!(
        yaml_out.contains("post-start-app"),
        "YAML output should contain post-start-app command:\n{yaml_out}"
    );
    assert!(
        yaml_out.contains("npm run migrate"),
        "YAML output should contain command line:\n{yaml_out}"
    );
}

#[test]
fn post_start_string_format_parsed() {
    let compose = r#"
services:
  db:
    image: postgres:16
    post_start: "pg_isready -U postgres"
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    let cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "post-start-db")
        .expect("post-start-db command");
    // String form is parsed as single element; spaces cause shell-quoting
    assert_eq!(cmd.exec.command_line, "\"pg_isready -U postgres\"");
}

#[test]
fn post_start_combined_with_depends_on() {
    let compose = r#"
services:
  db:
    image: postgres:16
  app:
    image: node:20
    command: ["npm", "start"]
    entrypoint: ["/bin/sh", "-c"]
    post_start: ["npm", "run", "seed"]
    working_dir: /workspace
    depends_on:
      - db
"#;
    let projects = parse_compose_documents(compose).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    // depends_on moves command to run-app
    let run_cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "run-app")
        .expect("run-app command");
    assert_eq!(run_cmd.exec.command_line, "/bin/sh -c npm start");

    // post_start creates post-start-app
    let post_cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "post-start-app")
        .expect("post-start-app command");
    assert_eq!(post_cmd.exec.command_line, "npm run seed");

    // Both should be in postStart events
    let events = result.devfile.events.as_ref().expect("events");
    assert!(events.post_start.contains(&String::from("run-app")));
    assert!(events.post_start.contains(&String::from("post-start-app")));

    // Container should be idling (depends_on)
    let app = result
        .devfile
        .components
        .iter()
        .find(|c| c.name == "app")
        .expect("app component");
    if let ComponentSpec::Container(ref ctr) = app.spec {
        assert_eq!(ctr.command.as_deref(), Some(&[String::from("tail")][..]));
    } else {
        panic!("expected container");
    }
}

#[test]
fn post_start_override_merges_from_later_document() {
    let base = r#"
services:
  app:
    image: node:20
    post_start: ["npm", "run", "old"]
"#;
    let override_doc = r#"
services:
  app:
    post_start: ["npm", "run", "new"]
"#;
    let combined = format!("{base}\n---\n{override_doc}");
    let projects = parse_compose_documents(&combined).expect("parse");
    let merged = merge_projects(projects);
    let result = convert_to_devfile(merged, RuleSet::default(), None);

    let cmd = result
        .devfile
        .commands
        .iter()
        .find(|c| c.id == "post-start-app")
        .expect("post-start-app command");
    assert_eq!(cmd.exec.command_line, "npm run new");
}
