# Compose to Devfile (Leptos)

[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/batleforc/devcomposefile/badge)](https://scorecard.dev/viewer/?uri=github.com/batleforc/devcomposefile)

Frontend-only Rust + Leptos (WASM) app that converts Docker Compose YAML to [Devfile 2.3.0](https://devfile.io/docs/2.3.0/what-is-a-devfile) in the browser. No backend required — everything runs client-side.

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable, edition 2024)
- [Trunk](https://trunkrs.dev/) 0.21+
- `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`)

### Development

```bash
cargo install trunk
trunk serve
```

Open `http://127.0.0.1:8080/` in your browser.

### Production Build

```bash
trunk build --release
```

The `dist/` directory contains the complete static site ready for deployment.

### Tests

```bash
cargo test
```

## Deployment

The release build produces a self-contained `dist/` folder with:

- `index.html` — entry point
- `devcomposefile-*.js` — WASM bindings
- `devcomposefile-*_bg.wasm` — compiled WASM binary
- `style-*.css` — stylesheet
- `assets/` — bundled default and startup rule files

Deploy the `dist/` folder to any static hosting provider:

| Provider | Command / Steps |
|----------|----------------|
| **GitHub Pages** | Push `dist/` to a `gh-pages` branch, or configure Actions to run `trunk build --release` and deploy the output |
| **Netlify** | Set build command to `trunk build --release`, publish directory to `dist` |
| **Cloudflare Pages** | Same as Netlify — build command `trunk build --release`, output `dist` |
| **nginx** | Copy `dist/` contents to your web root |
| **Apache** | Copy `dist/` contents to `DocumentRoot` |

> The app uses no server-side logic. Any static file server works.

### Custom Startup Rules

To deploy with environment-specific rules, replace `dist/assets/rules/startup-rules.json` after the build. The app fetches this file at initialization and merges it with bundled defaults.

## Usage

### Fetch from Git Repository

Fetch a Compose file directly from a public Git repository:

1. Paste a repository URL (e.g. `https://github.com/docker/awesome-compose`)
2. Optionally specify a **branch/tag** (defaults to `main`) and **file path** (defaults to `docker-compose.yml`)
3. Click **Fetch** — the file content is loaded into the Compose input textarea

You can also paste a direct file URL like `https://github.com/owner/repo/blob/main/path/compose.yml` and the branch and path are extracted automatically.

**Supported providers:** GitHub, GitLab, Bitbucket (public repositories only).

### Compose Input

- Paste one or more Compose files in the Compose textarea.
- **Drag & drop** `.yml`/`.yaml` files onto the input area (multiple files auto-separated with `---`).
- **Upload files** via the file picker button.
- Use `---` between YAML documents to represent multiple Compose files.
- Documents are merged with later documents taking precedence for scalar fields.
- Fetched Git content is appended (with `---` separator) if the textarea already has content.

### Include Directives

Docker Compose `include` directives are supported. When a Compose file contains:

```yaml
include:
  - ./db.yml
  - path: ./monitoring/prometheus.yml
    project_directory: ./monitoring
```

The app resolves included files in two ways:

1. **Git context**: If the main Compose file was fetched from a Git repository, included files are automatically fetched from the same repo and branch using relative path resolution.
2. **Local file registry**: Upload include target files via the **Include Files** panel. File names are matched against include paths.

**Include resolution rules:**
- Included projects merge first (lower precedence), then the main project merges on top.
- Nested includes are supported with cycle detection.
- Both short form (`- path.yml`) and long form (`path:`, `project_directory:`, `env_file:`) are parsed.
- `project_directory` and `env_file` are noted but do not affect Devfile conversion (they are Docker runtime concepts).

### Output

- **Copy** the generated Devfile YAML to clipboard.
- **Download** as a `devfile.yaml` file.

## Rules JSON Format

Rules control how Compose services are transformed into Devfile components. Rules are defined as JSON objects with three optional sections.

### Complete Schema

```json
{
  "registryCache": {
    "prefix": "my-registry.example.com",
    "mode": "prepend"
  },
  "envTranslations": [
    {
      "service": "*",
      "from": "SOURCE_VAR",
      "to": "TARGET_VAR",
      "remove": true,
      "set": {
        "NEW_KEY": "value"
      }
    }
  ],
  "baseIdeContainer": {
    "name": "ide",
    "image": "quay.io/devfile/universal-developer-image:latest",
    "memoryLimit": "2Gi"
  }
}
```

### `registryCache`

Rewrites all container images to route through a registry cache/mirror.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `prefix` | string | *required* | Registry prefix to prepend or use as replacement |
| `mode` | `"prepend"` \| `"replace"` | `"prepend"` | **prepend**: adds prefix before the image name. **replace**: strips the original registry and substitutes the prefix |

Examples:

- **Prepend**: `nginx:latest` → `my-cache.local/nginx:latest`
- **Replace**: `ghcr.io/org/app:v1` → `my-cache.local/org/app:v1`

### `envTranslations`

An array of rules for renaming, removing, or injecting environment variables.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `service` | string | `"*"` | Glob pattern matching service names |
| `from` | string \| null | `null` | Source env var to match |
| `to` | string \| null | `null` | Target env var name to copy/rename to |
| `remove` | boolean | `false` | Remove the `from` variable after copy |
| `set` | object | `{}` | Key-value pairs to inject unconditionally |

**Service selector patterns:**

| Pattern | Matches |
|---------|---------|
| `*` | All services |
| `web*` | Services starting with "web" |
| `*worker` | Services ending with "worker" |
| `*db*` | Services containing "db" |
| `redis` | Exact match only |

**Rule behaviors:**
- `from` + `to` + `remove: false` → copies the value to a new key
- `from` + `to` + `remove: true` → renames the variable
- `from` + `remove: true` (no `to`) → removes the variable
- `set` → injects key-value pairs regardless of existing variables

Rules are applied in array order. Multiple rules can target the same service cumulatively.

### `baseIdeContainer`

Defines an IDE base container to insert into the generated Devfile.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | `"ide"` | Component name (auto-suffixed if it conflicts with a service name) |
| `image` | string | *required* | Container image for the IDE |
| `memoryLimit` | string \| null | `null` | Memory limit (e.g. `"2Gi"`, `"512Mi"`) |

### Rules Precedence

Rules are merged in this order (later wins for same-kind fields):

1. **Bundled defaults** (`assets/rules/default-rules.json`) — always loaded
2. **Startup rules** (`assets/rules/startup-rules.json`) — fetched at app init
3. **Runtime JSON** — pasted into the rules textarea in the UI
4. **Runtime IDE image input** — the text field in the UI (highest priority for IDE container image only)

Merge behavior:
- `registryCache`: later value **replaces** entirely
- `envTranslations`: later entries **append** to the array (all rules run cumulatively)
- `baseIdeContainer`: later value **replaces** entirely

## Supported Compose Subset

### Fully Supported Keys

| Compose Key | Devfile Mapping |
|-------------|-----------------|
| `services.<name>.image` | `components[].container.image` |
| `services.<name>.environment` (map or list) | `components[].container.env[]` |
| `services.<name>.ports` (short and long syntax) | `components[].container.endpoints[]` |
| `services.<name>.volumes` (short and long syntax) | `components[].container.volumeMounts[]` + volume components |
| `services.<name>.command` (string or list) | `components[].container.args` + `commands[].exec` |
| `services.<name>.entrypoint` (string or list) | `components[].container.command` + `commands[].exec` |
| `services.<name>.working_dir` | `commands[].exec.workingDir` |
| `services.<name>.depends_on` (list or mapping) | Parsed (used for merge order; not directly in Devfile) |
| `services.<name>.build` (string or object) | Parsed and noted in diagnostics; not converted yet |
| `name` | `metadata.name` |
| `include` (short and long form) | Resolved and merged before conversion (lower precedence than main file) |

### Transformation Details

- **Ports**: Host-mapped ports (`8080:80`) get `exposure: public`; container-only ports (`"80"`) get `exposure: internal`. Port ranges map to the first port with a diagnostic.
- **Volumes**: Named volumes create Devfile volume components with `volumeMounts`. Bind mounts (`.`, `/`, `~` prefixed sources) are skipped since `mountSources: true` covers source mounts. Anonymous volumes get auto-generated names.
- **Commands**: Services with `command` and/or `entrypoint` generate `run-<service>` commands. Services with ports additionally get `debug-<service>` commands.
- **Events**: All generated commands are added to `events.postStart`.
- **Multi-document merge**: Later documents override scalar fields (image, build, working_dir) and replace list fields (ports, volumes, command, entrypoint, depends_on). Environment maps are merged additively with later values winning per key.

### Unsupported Keys (Reported in Diagnostics)

The following Compose keys are detected and reported as unsupported in the diagnostics panel rather than silently ignored:

- Top-level: `volumes`, `networks`, `secrets`, `configs`, `version`, and any other non-`services`/`name` key
- Service-level: `healthcheck`, `restart`, `deploy`, `logging`, `labels`, `expose`, `links`, `extra_hosts`, `dns`, `cap_add`, `cap_drop`, `privileged`, `user`, `stdin_open`, `tty`, and any other key outside the supported set

## Known Limitations

- **Build contexts**: `build` is parsed and preserved in the internal model, but there is no Devfile equivalent — the service is skipped if it has no `image`.
- **Healthchecks**: Detected and surfaced as a diagnostic, not converted.
- **Networks**: Not mapped to Devfile. All containers share a flat network in the Devfile model.
- **Volume drivers / options**: Only source, target, and read-only are parsed from volume definitions.
- **Port ranges**: Only the first port in a range is mapped (with a diagnostic).
- **Compose `version` key**: Ignored (Compose V2 does not require it).
- **No backend**: All processing is client-side. Very large Compose files may be slow in the browser.
- **Git fetch**: Only public repositories are supported. Private repos require authentication which is not implemented.
- **Devfile 2.3.0 only**: The app targets a single Devfile schema version.

## Project Structure

```
src/
  main.rs              # WASM entry point
  lib.rs               # Library target for integration tests
  app/mod.rs           # Top-level Leptos component and orchestration
  domain/
    compose.rs         # Compose types, YAML parsing, normalization
    devfile.rs         # Devfile 2.3.0 output types
    git_fetch.rs       # Git URL parsing and raw-content URL building
    rules.rs           # Rules schema, loading, merge
  convert/
    merge.rs           # Multi-document Compose merge
    include_resolver.rs # Include directive resolution (git + local registry)
    transform.rs       # Compose → Devfile conversion pipeline
    rule_engine.rs     # Image/env rewrite logic with tracing
    validate.rs        # Devfile structural validation
  ui/
    mod.rs             # UI module exports
    compose_input.rs   # Compose input with drag & drop
    git_repo_input.rs  # Fetch Compose from Git repository
    include_files.rs   # Include file upload panel
    rules_panel.rs     # Rules panel with defaults toggle
    output.rs          # YAML output with copy/download
    diagnostics.rs     # Diagnostics list
    traces_panel.rs    # Applied rules trace panel
assets/rules/
  default-rules.json   # Bundled defaults
  startup-rules.json   # Per-deployment startup rules
tests/
  conversion_tests.rs  # Integration tests
  fixtures/            # Test Compose files and expected outputs
```
