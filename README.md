# Compose to Devfile (Leptos)

Frontend-only app that converts Docker Compose YAML to Devfile 2.3.0 in the browser.

## Run

```bash
cargo install trunk
trunk serve
```

Then open the local URL shown by Trunk.

## Input behavior

- Paste one or more Compose files in the Compose textarea.
- Use `---` between YAML documents to represent multiple Compose files.
- Documents are merged with later documents taking precedence for scalar fields.

## Rules behavior

Rules precedence:

1. Bundled defaults: `assets/rules/default-rules.json`
2. Startup-provided rules: `assets/rules/startup-rules.json`
3. Runtime JSON rules from the UI textarea
4. Runtime IDE image input (highest priority for IDE container image)

## Supported transformations

- Image rewrite using `registryCache.prefix`
- Environment variable translation (`from` -> `to`, optional `remove`, and key/value `set`)
- IDE base container insertion

## Current limitations

- Compose features are intentionally partial in this first implementation.
- Build contexts, healthchecks, networks, and advanced volume syntax are not fully modeled.
