use std::collections::BTreeMap;

use leptos::prelude::*;

use crate::domain::rules::{
    EnvTranslationRule, IdeContainerRule, RegistryCacheMode, RegistryCacheRule, RegistryMirrorRule,
    RuleSet,
};

/// Visual rule editor that builds a `RuleSet` JSON via form controls.
/// Syncs its output into `runtime_rules_input` so the conversion pipeline
/// picks it up automatically.
#[component]
pub fn RuleEditor(runtime_rules_input: RwSignal<String>) -> impl IntoView {
    // --- Registry cache ---
    let cache_enabled = RwSignal::new(false);
    let cache_prefix = RwSignal::new(String::new());
    let cache_mode = RwSignal::new(String::from("prepend"));

    // --- Registry mirrors ---
    let mirror_next_id = RwSignal::new(1u32);
    let mirror_rows = RwSignal::new(Vec::<MirrorRow>::new());

    // --- IDE container ---
    let ide_enabled = RwSignal::new(false);
    let ide_name = RwSignal::new(String::from("tool"));
    let ide_image = RwSignal::new(String::new());
    let ide_memory = RwSignal::new(String::new());

    // --- Env translations (dynamic list) ---
    let next_id = RwSignal::new(1u32);
    let env_rules = RwSignal::new(Vec::<EnvRow>::new());

    // Sync form state → JSON whenever anything changes
    let sync = move || {
        let mut ruleset = RuleSet::default();

        if cache_enabled.get() {
            let prefix = cache_prefix.get();
            if !prefix.trim().is_empty() {
                ruleset.registry_cache = Some(RegistryCacheRule {
                    prefix: prefix.trim().to_string(),
                    mode: if cache_mode.get() == "replace" {
                        RegistryCacheMode::Replace
                    } else {
                        RegistryCacheMode::Prepend
                    },
                });
            }
        }

        // Registry mirrors
        let mirrors = mirror_rows.get();
        for row in &mirrors {
            let src = row.source.get();
            let tgt = row.target.get();
            if !src.trim().is_empty() && !tgt.trim().is_empty() {
                ruleset.registry_mirrors.push(RegistryMirrorRule {
                    source: src.trim().to_string(),
                    target: tgt.trim().to_string(),
                });
            }
        }

        let rows = env_rules.get();
        for row in &rows {
            let from = row.from.get();
            let to = row.to.get();
            let from_opt = if from.trim().is_empty() {
                None
            } else {
                Some(from.trim().to_string())
            };
            let to_opt = if to.trim().is_empty() {
                None
            } else {
                Some(to.trim().to_string())
            };
            let set_val = row.set_key.get();
            let set_value = row.set_value.get();
            let mut set_map = BTreeMap::new();
            if !set_val.trim().is_empty() {
                set_map.insert(set_val.trim().to_string(), set_value.trim().to_string());
            }
            ruleset.env_translations.push(EnvTranslationRule {
                service: {
                    let s = row.service.get();
                    if s.trim().is_empty() {
                        String::from("*")
                    } else {
                        s.trim().to_string()
                    }
                },
                from: from_opt,
                to: to_opt,
                remove: row.remove.get(),
                set: set_map,
            });
        }

        if ide_enabled.get() {
            let image = ide_image.get();
            if !image.trim().is_empty() {
                let mem = ide_memory.get();
                ruleset.base_ide_container = Some(IdeContainerRule {
                    name: {
                        let n = ide_name.get();
                        if n.trim().is_empty() {
                            String::from("tool")
                        } else {
                            n.trim().to_string()
                        }
                    },
                    image: image.trim().to_string(),
                    memory_limit: if mem.trim().is_empty() {
                        None
                    } else {
                        Some(mem.trim().to_string())
                    },
                });
            }
        }

        // Only emit JSON when there's something non-default
        let has_content = ruleset.registry_cache.is_some()
            || !ruleset.registry_mirrors.is_empty()
            || !ruleset.env_translations.is_empty()
            || ruleset.base_ide_container.is_some();

        if has_content {
            match serde_json::to_string_pretty(&ruleset) {
                Ok(json) => runtime_rules_input.set(json),
                Err(_) => {}
            }
        } else {
            runtime_rules_input.set(String::new());
        }
    };

    let add_env_rule = move |_| {
        let id = next_id.get();
        next_id.set(id + 1);
        env_rules.update(|rows| {
            rows.push(EnvRow {
                id,
                service: RwSignal::new(String::from("*")),
                from: RwSignal::new(String::new()),
                to: RwSignal::new(String::new()),
                remove: RwSignal::new(false),
                set_key: RwSignal::new(String::new()),
                set_value: RwSignal::new(String::new()),
            });
        });
        sync();
    };

    let remove_env_rule = move |row_id: u32| {
        env_rules.update(|rows| rows.retain(|r| r.id != row_id));
        sync();
    };

    let add_mirror_rule = move |_| {
        let id = mirror_next_id.get();
        mirror_next_id.set(id + 1);
        mirror_rows.update(|rows| {
            rows.push(MirrorRow {
                id,
                source: RwSignal::new(String::new()),
                target: RwSignal::new(String::new()),
            });
        });
        sync();
    };

    let remove_mirror_rule = move |row_id: u32| {
        mirror_rows.update(|rows| rows.retain(|r| r.id != row_id));
        sync();
    };

    view! {
        <div class="rule-editor">
            // --- Registry Cache Section ---
            <fieldset class="rule-section">
                <legend>"Registry Cache"</legend>
                <label class="rule-checkbox">
                    <input
                        type="checkbox"
                        prop:checked=move || cache_enabled.get()
                        on:change=move |ev| {
                            cache_enabled.set(event_target_checked(&ev));
                            sync();
                        }
                    />
                    " Enable registry cache rewriting"
                </label>
                {move || {
                    if cache_enabled.get() {
                        Some(
                            view! {
                                <div class="rule-fields">
                                    <div class="rule-field">
                                        <label class="rule-label">"Prefix"</label>
                                        <input
                                            class="text rule-input"
                                            type="text"
                                            placeholder="e.g. registry-cache.local"
                                            prop:value=move || cache_prefix.get()
                                            on:input=move |ev| {
                                                cache_prefix.set(event_target_value(&ev));
                                                sync();
                                            }
                                        />
                                    </div>
                                    <div class="rule-field">
                                        <label class="rule-label">"Mode"</label>
                                        <select
                                            class="text rule-input"
                                            prop:value=move || cache_mode.get()
                                            on:change=move |ev| {
                                                cache_mode.set(event_target_value(&ev));
                                                sync();
                                            }
                                        >
                                            <option value="prepend">"Prepend"</option>
                                            <option value="replace">"Replace"</option>
                                        </select>
                                    </div>
                                </div>
                            },
                        )
                    } else {
                        None
                    }
                }}
            </fieldset>

            // --- Registry Mirrors Section ---
            <fieldset class="rule-section">
                <legend>"Registry Mirrors"</legend>
                <p class="hint">"Per-registry overrides: images from a source registry are rewritten to a specific target."</p>
                <div class="env-rules-list">
                    <For
                        each=move || mirror_rows.get()
                        key=|row| row.id
                        let:row
                    >
                        {
                            let row_id = row.id;
                            let source = row.source;
                            let target = row.target;
                            view! {
                                <div class="env-rule-card">
                                    <div class="env-rule-header">
                                        <span class="env-rule-title">
                                            {move || {
                                                let s = source.get();
                                                if s.is_empty() {
                                                    String::from("New mirror")
                                                } else {
                                                    format!("{s} →")
                                                }
                                            }}
                                        </span>
                                        <button
                                            class="btn-remove"
                                            title="Remove mirror"
                                            on:click=move |_| remove_mirror_rule(row_id)
                                        >
                                            "×"
                                        </button>
                                    </div>
                                    <div class="env-rule-body">
                                        <div class="rule-field">
                                            <label class="rule-label">"Source registry"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="e.g. ghcr.io"
                                                prop:value=move || source.get()
                                                on:input=move |ev| {
                                                    source.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                        <div class="rule-field">
                                            <label class="rule-label">"Target registry"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="e.g. ghcr-cache.local"
                                                prop:value=move || target.get()
                                                on:input=move |ev| {
                                                    target.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                    </div>
                                </div>
                            }
                        }
                    </For>
                </div>
                <button class="btn-secondary" on:click=add_mirror_rule>
                    "+ Add registry mirror"
                </button>
            </fieldset>

            // --- Env Translations Section ---
            <fieldset class="rule-section">
                <legend>"Environment Translations"</legend>
                <div class="env-rules-list">
                    <For
                        each=move || env_rules.get()
                        key=|row| row.id
                        let:row
                    >
                        {
                            let row_id = row.id;
                            let service = row.service;
                            let from = row.from;
                            let to = row.to;
                            let remove = row.remove;
                            let set_key = row.set_key;
                            let set_value = row.set_value;
                            view! {
                                <div class="env-rule-card">
                                    <div class="env-rule-header">
                                        <span class="env-rule-title">
                                            {move || {
                                                let s = service.get();
                                                if s.is_empty() || s == "*" {
                                                    String::from("All services")
                                                } else {
                                                    format!("Service: {s}")
                                                }
                                            }}
                                        </span>
                                        <button
                                            class="btn-remove"
                                            title="Remove rule"
                                            on:click=move |_| remove_env_rule(row_id)
                                        >
                                            "×"
                                        </button>
                                    </div>
                                    <div class="env-rule-body">
                                        <div class="rule-field">
                                            <label class="rule-label">"Service"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="* (all)"
                                                prop:value=move || service.get()
                                                on:input=move |ev| {
                                                    service.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                        <div class="rule-field">
                                            <label class="rule-label">"From (env var)"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="e.g. NODE_ENV"
                                                prop:value=move || from.get()
                                                on:input=move |ev| {
                                                    from.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                        <div class="rule-field">
                                            <label class="rule-label">"To (rename)"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="e.g. APP_ENV"
                                                prop:value=move || to.get()
                                                on:input=move |ev| {
                                                    to.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                        <label class="rule-checkbox">
                                            <input
                                                type="checkbox"
                                                prop:checked=move || remove.get()
                                                on:change=move |ev| {
                                                    remove.set(event_target_checked(&ev));
                                                    sync();
                                                }
                                            />
                                            " Remove original"
                                        </label>
                                        <div class="rule-field">
                                            <label class="rule-label">"Set key"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="e.g. GENERATED"
                                                prop:value=move || set_key.get()
                                                on:input=move |ev| {
                                                    set_key.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                        <div class="rule-field">
                                            <label class="rule-label">"Set value"</label>
                                            <input
                                                class="text rule-input"
                                                type="text"
                                                placeholder="e.g. true"
                                                prop:value=move || set_value.get()
                                                on:input=move |ev| {
                                                    set_value.set(event_target_value(&ev));
                                                    sync();
                                                }
                                            />
                                        </div>
                                    </div>
                                </div>
                            }
                        }
                    </For>
                </div>
                <button class="btn-secondary" on:click=add_env_rule>
                    "+ Add env translation"
                </button>
            </fieldset>

            // --- IDE Container Section ---
            <fieldset class="rule-section">
                <legend>"IDE / Tool Container"</legend>
                <label class="rule-checkbox">
                    <input
                        type="checkbox"
                        prop:checked=move || ide_enabled.get()
                        on:change=move |ev| {
                            ide_enabled.set(event_target_checked(&ev));
                            sync();
                        }
                    />
                    " Override IDE container"
                </label>
                {move || {
                    if ide_enabled.get() {
                        Some(
                            view! {
                                <div class="rule-fields">
                                    <div class="rule-field">
                                        <label class="rule-label">"Name"</label>
                                        <input
                                            class="text rule-input"
                                            type="text"
                                            placeholder="tool"
                                            prop:value=move || ide_name.get()
                                            on:input=move |ev| {
                                                ide_name.set(event_target_value(&ev));
                                                sync();
                                            }
                                        />
                                    </div>
                                    <div class="rule-field">
                                        <label class="rule-label">"Image"</label>
                                        <input
                                            class="text rule-input"
                                            type="text"
                                            placeholder="quay.io/devfile/universal-developer-image:latest"
                                            prop:value=move || ide_image.get()
                                            on:input=move |ev| {
                                                ide_image.set(event_target_value(&ev));
                                                sync();
                                            }
                                        />
                                    </div>
                                    <div class="rule-field">
                                        <label class="rule-label">"Memory limit"</label>
                                        <input
                                            class="text rule-input"
                                            type="text"
                                            placeholder="e.g. 2Gi"
                                            prop:value=move || ide_memory.get()
                                            on:input=move |ev| {
                                                ide_memory.set(event_target_value(&ev));
                                                sync();
                                            }
                                        />
                                    </div>
                                </div>
                            },
                        )
                    } else {
                        None
                    }
                }}
            </fieldset>
        </div>
    }
}

/// Helper to extract checked state from a checkbox event.
fn event_target_checked(ev: &leptos::ev::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.checked())
        .unwrap_or(false)
}

#[derive(Clone)]
struct MirrorRow {
    id: u32,
    source: RwSignal<String>,
    target: RwSignal<String>,
}

#[derive(Clone)]
struct EnvRow {
    id: u32,
    service: RwSignal<String>,
    from: RwSignal<String>,
    to: RwSignal<String>,
    remove: RwSignal<bool>,
    set_key: RwSignal<String>,
    set_value: RwSignal<String>,
}
