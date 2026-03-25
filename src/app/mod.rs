use std::collections::BTreeMap;

use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::convert::include_resolver::{IncludeContext, resolve_includes};
use crate::convert::merge::merge_projects;
use crate::convert::transform::convert_to_devfile;
use crate::convert::validate::validate_devfile;
use crate::domain::compose::parse_compose_documents;
use crate::domain::git_fetch::RepoRef;
use crate::domain::rules::{RuleSet, load_default_rules, load_rules_from_json, merge_rules};
use crate::ui::compose_input::ComposeInput;
use crate::ui::diagnostics::DiagnosticsPanel;
use crate::ui::git_repo_input::GitRepoInput;
use crate::ui::include_files::IncludeFilesPanel;
use crate::ui::output::OutputPanel;
use crate::ui::rules_panel::RulesPanel;
use crate::ui::traces_panel::TracesPanel;

#[component]
pub fn App() -> impl IntoView {
    let compose_input = RwSignal::new(String::new());
    let runtime_rules_input = RwSignal::new(String::new());
    let ide_image_input = RwSignal::new(String::new());

    let startup_rules = RwSignal::new(None::<RuleSet>);
    let startup_rules_status = RwSignal::new(String::from("Loading startup rules..."));
    let startup_rules_loaded = RwSignal::new(false);

    let output_yaml = RwSignal::new(String::new());
    let diagnostics = RwSignal::new(Vec::<String>::new());
    let applied_rules = RwSignal::new(Vec::<String>::new());

    let file_registry = RwSignal::new(BTreeMap::<String, String>::new());
    let git_context = RwSignal::new(None::<RepoRef>);

    Effect::new(move |_| {
        if startup_rules_loaded.get() {
            return;
        }
        startup_rules_loaded.set(true);

        spawn_local(async move {
            let request = Request::get("/assets/rules/startup-rules.json")
                .send()
                .await;
            match request {
                Ok(response) if response.ok() => match response.text().await {
                    Ok(text) if !text.trim().is_empty() => match load_rules_from_json(&text) {
                        Ok(parsed) => {
                            startup_rules.set(Some(parsed));
                            startup_rules_status.set(String::from("Startup rules loaded."));
                        }
                        Err(err) => {
                            startup_rules_status.set(format!("Startup rules invalid JSON: {err}"))
                        }
                    },
                    _ => startup_rules_status
                        .set(String::from("Startup rules file is empty; ignored.")),
                },
                _ => startup_rules_status.set(String::from(
                    "No startup rules file found; using defaults only.",
                )),
            }
        });
    });

    let on_convert = move |_| {
        spawn_local(async move {
            let mut messages = Vec::<String>::new();

            let compose_docs = match parse_compose_documents(&compose_input.get()) {
                Ok(docs) => docs,
                Err(err) => {
                    messages.push(err);
                    diagnostics.set(messages);
                    output_yaml.set(String::new());
                    return;
                }
            };

            if compose_docs.is_empty() {
                diagnostics.set(vec![String::from(
                    "No Compose documents found. Paste at least one YAML document.",
                )]);
                output_yaml.set(String::new());
                return;
            }

            // Resolve includes
            let context = match git_context.get() {
                Some(repo_ref) => IncludeContext::Git(repo_ref),
                None => IncludeContext::Local,
            };

            let mut registry = file_registry.get();
            let mut resolution = resolve_includes(compose_docs.clone(), &context, &registry);

            // Auto-fetch pending includes from Git
            if !resolution.pending_fetches.is_empty() {
                let mut fetched_any = false;
                for pending in &resolution.pending_fetches {
                    match Request::get(&pending.raw_url).send().await {
                        Ok(response) if response.ok() => match response.text().await {
                            Ok(text) if !text.trim().is_empty() => {
                                registry.insert(pending.path.clone(), text);
                                fetched_any = true;
                            }
                            _ => {
                                messages.push(format!(
                                    "Include `{}` fetched but content was empty.",
                                    pending.path
                                ));
                            }
                        },
                        Ok(response) => {
                            messages.push(format!(
                                "Include `{}`: HTTP {} — could not fetch from Git.",
                                pending.path,
                                response.status()
                            ));
                        }
                        Err(err) => {
                            messages
                                .push(format!("Include `{}`: network error — {err}", pending.path));
                        }
                    }
                }

                if fetched_any {
                    // Update the shared registry and re-resolve with newly fetched content
                    file_registry.set(registry.clone());
                    resolution = resolve_includes(compose_docs, &context, &registry);
                }
            }

            messages.extend(
                resolution
                    .diagnostics
                    .into_iter()
                    .filter(|d| !d.contains("will be fetched")),
            );

            let merged_project = merge_projects(resolution.projects);

            let default_rules = match load_default_rules() {
                Ok(rules) => rules,
                Err(err) => {
                    diagnostics.set(vec![format!("Bundled default rules failed to load: {err}")]);
                    output_yaml.set(String::new());
                    return;
                }
            };

            let merged_with_startup = if let Some(startup) = startup_rules.get() {
                merge_rules(&default_rules, &startup)
            } else {
                default_rules
            };

            let final_rules = if runtime_rules_input.get().trim().is_empty() {
                merged_with_startup
            } else {
                match load_rules_from_json(&runtime_rules_input.get()) {
                    Ok(runtime_rules) => merge_rules(&merged_with_startup, &runtime_rules),
                    Err(err) => {
                        diagnostics.set(vec![format!("Runtime rules JSON invalid: {err}")]);
                        output_yaml.set(String::new());
                        return;
                    }
                }
            };

            let ide_override = {
                let raw = ide_image_input.get();
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            };

            let conversion = convert_to_devfile(merged_project, final_rules, ide_override);
            messages.extend(conversion.diagnostics);
            messages.extend(validate_devfile(&conversion.devfile));

            let traces: Vec<String> = conversion
                .rule_traces
                .iter()
                .map(|t| format!("[{}] {}", t.service, t.description))
                .collect();

            match serde_yaml::to_string(&conversion.devfile) {
                Ok(yaml) => {
                    output_yaml.set(yaml);
                    diagnostics.set(messages);
                    applied_rules.set(traces);
                }
                Err(err) => {
                    diagnostics.set(vec![format!("Failed to serialize Devfile YAML: {err}")]);
                    output_yaml.set(String::new());
                    applied_rules.set(Vec::new());
                }
            }
        });
    };

    view! {
        <main class="page">
            <header class="hero">
                <h1>"Compose → Devfile"</h1>
                <p>
                    "Convert Docker Compose files into Devfile 2.3.0. "
                    "Drag & drop, paste YAML, or fetch from a Git repository."
                </p>
            </header>

            <GitRepoInput compose_input=compose_input git_context=git_context />

            <ComposeInput compose_input=compose_input />

            <IncludeFilesPanel file_registry=file_registry />

            <RulesPanel
                runtime_rules_input=runtime_rules_input
                ide_image_input=ide_image_input
                startup_rules_status=startup_rules_status
            />

            <section class="actions">
                <button class="convert" on:click=on_convert>
                    Generate Devfile
                </button>
            </section>

            <OutputPanel yaml=output_yaml />
            <DiagnosticsPanel diagnostics=diagnostics />
            <TracesPanel traces=applied_rules />
        </main>

        <footer class="site-footer">
            <p>
                "Made with \u{2764}\u{fe0f}, too much \u{2615} and a little \u{1f916}"
            </p>
            <p class="commit-ref">
                {format!("commit {}", env!("GIT_COMMIT_SHORT"))}
            </p>
        </footer>
    }
}
