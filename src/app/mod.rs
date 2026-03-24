use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::convert::merge::merge_projects;
use crate::convert::transform::convert_to_devfile;
use crate::convert::validate::validate_devfile;
use crate::domain::compose::parse_compose_documents;
use crate::domain::rules::{RuleSet, load_default_rules, load_rules_from_json, merge_rules};
use crate::ui::diagnostics::DiagnosticsPanel;
use crate::ui::output::OutputPanel;

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

        let merged_project = merge_projects(compose_docs);

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

        match serde_yaml::to_string(&conversion.devfile) {
            Ok(yaml) => {
                output_yaml.set(yaml);
                diagnostics.set(messages);
            }
            Err(err) => {
                diagnostics.set(vec![format!("Failed to serialize Devfile YAML: {err}")]);
                output_yaml.set(String::new());
            }
        }
    };

    view! {
        <main class="page">
            <section class="hero">
                <h1>Compose to Devfile</h1>
                <p>
                    Convert one or more Docker Compose YAML documents into Devfile 2.3.0.
                    Multiple files are supported by using YAML document separators: <code>---</code>.
                </p>
            </section>

            <section class="panel">
                <h2>Compose Input</h2>
                <textarea
                    class="editor"
                    placeholder="Paste Docker Compose YAML here. For multiple files, separate with ---"
                    prop:value=move || compose_input.get()
                    on:input=move |ev| compose_input.set(event_target_value(&ev))
                ></textarea>
            </section>

            <section class="panel two-col">
                <div>
                    <h2>Rules</h2>
                    <p class="status">{move || startup_rules_status.get()}</p>
                    <textarea
                        class="editor small"
                        placeholder="Optional runtime rules JSON (merged last)"
                        prop:value=move || runtime_rules_input.get()
                        on:input=move |ev| runtime_rules_input.set(event_target_value(&ev))
                    ></textarea>
                </div>

                <div>
                    <h2>IDE Base Container</h2>
                    <input
                        class="text"
                        type="text"
                        placeholder="Container image (e.g. quay.io/devfile/universal-developer-image:latest)"
                        prop:value=move || ide_image_input.get()
                        on:input=move |ev| ide_image_input.set(event_target_value(&ev))
                    />
                    <p class="hint">
                        This runtime value overrides rules for the IDE base container image.
                    </p>
                </div>
            </section>

            <section class="actions">
                <button class="convert" on:click=on_convert>
                    Generate Devfile
                </button>
            </section>

            <OutputPanel yaml=output_yaml />
            <DiagnosticsPanel diagnostics=diagnostics />
        </main>
    }
}
