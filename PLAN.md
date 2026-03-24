# Plan: Leptos Compose-to-Devfile Web App

Build a pure frontend Rust + Leptos (WASM) app that converts one or more Docker Compose files into Devfile 2.3.0, with a deterministic transformation pipeline and extensible JSON rule system (bundled defaults + startup-provided rules + per-run IDE container input). Prioritize a reliable conversion core first, then UX, then validation/export.

## Phases

### Phase 1 — Project foundation ✅

1. Initialize a Leptos CSR project configured for static hosting and WASM output; wire build scripts and local dev workflow.
2. Define module boundaries early: compose ingestion/merge, normalization, rule engine, devfile builder, schema validation, and UI orchestration.
3. Add serialization dependencies for YAML/JSON handling and deterministic output ordering.

### Phase 2 — Domain models and transformation contracts ✅ *(depends on Phase 1)*

4. Define internal canonical model for merged Compose services (image, build, env, ports, volumes, command, entrypoint, working_dir, depends_on).
5. Define Devfile 2.3.0 output model for components/commands/events/projects/attributes needed by this app.
6. Define rule JSON schema and precedence: bundled defaults < startup-provided rules < runtime UI IDE container input for applicable fields.
7. Add strict parsing + user-friendly diagnostics for invalid Compose YAML, invalid rule JSON, and unsupported Compose fields.

### Phase 3 — Compose ingestion and merge ✅ *(depends on Phase 2)*

8. Implement support for multiple Compose files with deterministic merge behavior (service maps merged by name, later files override scalar fields, env maps merged with later precedence).
9. Normalize Compose variants (list/map env, short/long syntax where practical) into canonical model before conversion.
10. Add explicit unsupported-feature capture list so UI can surface what was ignored.

### Phase 4 — Rule engine and conversion ✅ *(depends on Phase 3)*

11. Implement transformation pipeline stages: pre-normalization rule hooks (optional), service-level rewrite rules (image rewrite, env rewrite), Devfile augmentation rules.
12. Implement registry-cache image rewrite rule pattern (e.g., prepend/replace registry domain).
13. Implement environment variable translation rules (rename, inject, remove, map by service selector).
14. Implement IDE base container insertion strategy using runtime user input as highest-priority source.
15. Map services to Devfile container components and generate default run/debug commands when enough metadata exists.

### Phase 5 — Frontend UX ✅ *(parallel with late Phase 4 unit tests)*

16. Build UI sections: Compose input (multi-file paste/upload), rules panel (show bundled defaults + optional startup-provided overrides), IDE container input, conversion output, diagnostics panel.
17. Add conversion preview with YAML output and copy/download actions.
18. Add explainability panel that lists applied rules and resulting diffs at service/component level.

### Phase 6 — Validation and hardening ✅ *(depends on Phases 4 and 5)*

19. Add Devfile schema checks against selected 2.3.0 structural constraints (at minimum required fields and container component validity).
20. Add unit tests for merge semantics, rewrite precedence, IDE container priority, and representative Compose fixtures.
21. Add end-to-end browser tests for main conversion journeys and error states.

### Phase 7 — Packaging and documentation ✅ *(depends on Phase 6)*

22. Prepare static build output and lightweight deployment docs (GitHub Pages/Netlify equivalent static host).
23. Write user guide for rules JSON format, precedence, supported Compose subset, and known limitations.

### Phase 8 — Git repository Compose fetch ✅ *(depends on Phase 7)*

24. Add ability to fetch a Compose file from a public Git repository URL (GitHub, GitLab, Bitbucket raw-content APIs).
25. Build a `GitRepoInput` UI component with URL input, branch/ref selector, path input, and fetch button.
26. Translate user-friendly repo URLs to raw-content URLs; populate the Compose input textarea with fetched content.
27. Add unit tests for URL parsing/translation and integration test for fetch workflow.

### Phase 9 — Docker Compose `include` directive *(depends on Phase 8)*

28. Parse the `include` top-level key from Compose YAML (string list shorthand and long-form `path`/`project_directory`/`env_file` objects).
29. Implement include resolution: for Git-fetched Compose files, resolve relative include paths against the same repository/ref and auto-fetch; for pasted/uploaded content, resolve against a local file registry populated by drag & drop / upload.
30. Add an auxiliary file upload panel so users can provide include targets when working without Git context.
31. Merge included projects before the main project (lower precedence) with recursive include support and cycle detection.
32. Add unit tests for include parsing and resolution, and integration tests for full include → merge → convert pipelines.

### Phase 10 — Tool container, modern UI, compose.yaml fallback *(depends on Phase 9)*

33. Rename the IDE base container from `ide` to `tool` and move it to the first position in the generated Devfile components list.
34. In Git fetch, when `docker-compose.yml` is not found (HTTP 404), automatically retry with `compose.yaml` before reporting an error.
35. Rework the frontend CSS and layout for a modern, polished design with dark header, card-based panels, better typography, and improved responsive behavior.

### Phase 11 — Compose variable references → Devfile variables *(depends on Phase 10)*

36. Scan all string fields in the merged Compose project (`image`, `environment` values, `command`, `entrypoint`, `working_dir`, port/volume strings) for `${VAR}`, `${VAR:-default}`, and `${VAR-default}` references.
37. Collect unique variable names into a `variables` map on the Devfile (first default encountered wins) and rewrite references from Docker Compose `${VAR}` syntax to Devfile `{{VAR}}` syntax.
38. Add `variables: BTreeMap<String, String>` field to the `Devfile` struct, serialized between `metadata` and `components`, and skipped when empty.
39. New `src/convert/variables.rs` module with `extract_and_rewrite_variables()` function called in the transform pipeline after rule application but before component building.

### Phase 12 — Pre-fill Git repo from URL query parameters *(depends on Phase 10)*

40. Read `?repo=`, `?ref=`, and `?path=` query parameters from the page URL on component mount using `web_sys::Url` / `UrlSearchParams`.
41. Pre-fill the Git repository URL, branch/tag, and file path input fields with the query parameter values.
42. When `?repo=` is present, automatically trigger the fetch on mount so the user sees the Compose file immediately.

## Relevant Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Rust workspace and dependency setup for Leptos/WASM, serde, YAML/JSON handling |
| `src/main.rs` | App bootstrap and root component mount |
| `src/lib.rs` | Library target exposing all modules for integration tests |
| `src/app/mod.rs` | Top-level app state and orchestration |
| `src/domain/compose.rs` | Compose canonical types and normalization utilities |
| `src/domain/devfile.rs` | Devfile 2.3.0 output types |
| `src/domain/git_fetch.rs` | Git URL parsing and raw-content URL translation |
| `src/domain/rules.rs` | Rules schema and precedence structures |
| `src/convert/merge.rs` | Multi-file compose merge logic |
| `src/convert/include_resolver.rs` | Include directive resolution with git and local registry support |
| `src/convert/transform.rs` | Conversion pipeline orchestration |
| `src/convert/rule_engine.rs` | Image/env/base-container rewrite logic |
| `src/convert/validate.rs` | Output structural checks and diagnostics |
| `src/convert/variables.rs` | Compose `${VAR}` extraction and Devfile `{{VAR}}` rewriting |
| `src/ui/output.rs` | YAML preview and export actions |
| `src/ui/diagnostics.rs` | Surfaced parse/merge/rule/validation messages |
| `src/ui/git_repo_input.rs` | Git repository URL input and fetch UI |
| `src/ui/include_files.rs` | Include file upload panel for auxiliary Compose files |
| `assets/rules/default-rules.json` | Bundled default special rules loaded on startup |
| `assets/rules/startup-rules.json` | Startup-provided rules fetched at runtime via HTTP |
| `tests/fixtures/*.yml` | Compose merge/conversion fixtures |
| `tests/conversion_tests.rs` | Conversion regression tests |
| `index.html` | Trunk entry point |
| `style.css` | App styling |
| `README.md` | Usage, rule format, and supported feature matrix |

## Verification

1. Run Rust unit tests for parser, merge, rule precedence, and converter determinism.
2. Run browser integration tests for upload/paste flows, multi-file merge path, and error handling.
3. Validate generated Devfile examples from fixture Compose sets and verify expected IDE container insertion.
4. Manually verify registry-cache image rewrite and env variable mapping rules using at least 3 heterogeneous Compose examples.
5. Build production WASM bundle and smoke-test on static hosting.

## Decisions

- **Stack**: Rust + Leptos.
- **Runtime**: Pure static web app first (browser-only WASM).
- **Devfile target**: 2.3.0.
- **Rules source**: Bundled local default JSON plus startup-provided rules; runtime IDE container input per conversion.
- **Compose scope**: Support multiple Compose files with merge semantics in v1.
- **Included in scope**: Image rewrite, env translation, IDE base container insertion, diagnostics, export.
- **Excluded from v1**: Backend persistence, remote rule fetching, advanced Compose features beyond declared supported subset.

## Further Considerations

1. **Startup-provided rules transport**: Pass JSON via app initialization payload or embedded static asset variant per deployment environment to avoid browser CORS/runtime fetch complexity.
2. **Devfile fidelity**: Define and publish a supported Compose feature matrix early to avoid perceived incorrect conversions for unsupported keys.
3. **Determinism**: Stabilize map ordering in serialization so output diffs remain clean and test snapshots are reliable.
