use std::collections::BTreeMap;

use leptos::prelude::*;

use crate::convert::service_refs::DetectedRef;

#[component]
pub fn ServiceRefsPanel(
    detected_refs: RwSignal<Vec<DetectedRef>>,
    overrides: RwSignal<BTreeMap<String, String>>,
) -> impl IntoView {
    // Derive unique target services from detected refs
    let unique_targets = move || {
        let refs = detected_refs.get();
        let mut targets: Vec<String> = refs.iter().map(|r| r.target_service.clone()).collect();
        targets.sort();
        targets.dedup();
        targets
    };

    let has_refs = move || !detected_refs.get().is_empty();

    view! {
        <Show when=has_refs>
            <section class="panel service-refs-panel">
                <h2>"Inter-Service References"</h2>
                <p class="hint">
                    "The following services are referenced as hostnames by other containers. "
                    "Choose a replacement value for each."
                </p>

                <div class="ref-overrides-list">
                    <For
                        each=unique_targets
                        key=|t| t.clone()
                        children=move |target| {
                            let target_for_select = target.clone();
                            let target_for_input = target.clone();
                            let target_for_custom = target.clone();
                            let target_display = target.clone();

                            let current_choice = Signal::derive({
                                let target_c = target.clone();
                                move || {
                                    let map = overrides.get();
                                    match map.get(&target_c) {
                                        None => "localhost".to_string(),
                                        Some(v) if v == &target_c => "keep".to_string(),
                                        Some(_) => "custom".to_string(),
                                    }
                                }
                            });

                            let show_custom_input = move || current_choice.get() == "custom";

                            let custom_value = Signal::derive({
                                let target_cv = target_for_input.clone();
                                move || {
                                    let map = overrides.get();
                                    map.get(&target_cv)
                                        .cloned()
                                        .unwrap_or_default()
                                }
                            });

                            // Derive the reference count for this target
                            let target_for_count = target_display.clone();
                            let ref_count = move || {
                                detected_refs
                                    .get()
                                    .iter()
                                    .filter(|r| r.target_service == target_for_count)
                                    .count()
                            };

                            view! {
                                <div class="ref-override-row">
                                    <span class="ref-target-name">{target_display.clone()}</span>
                                    <span class="ref-count">{move || format!("{} ref(s)", ref_count())}</span>
                                    <select
                                        class="ref-select"
                                        prop:value=current_choice
                                        on:change=move |ev| {
                                            let val = event_target_value(&ev);
                                            let mut map = overrides.get();
                                            match val.as_str() {
                                                "localhost" => {
                                                    map.remove(&target_for_select);
                                                }
                                                "keep" => {
                                                    map.insert(
                                                        target_for_select.clone(),
                                                        target_for_select.clone(),
                                                    );
                                                }
                                                "custom" => {
                                                    map.insert(target_for_select.clone(), String::new());
                                                }
                                                _ => {}
                                            }
                                            overrides.set(map);
                                        }
                                    >
                                        <option value="localhost">"localhost"</option>
                                        <option value="keep">"Keep original"</option>
                                        <option value="custom">"Custom value…"</option>
                                    </select>
                                    <Show when=show_custom_input>
                                        {
                                            let target_key = target_for_custom.clone();
                                            view! {
                                                <input
                                                    class="ref-custom-input"
                                                    type="text"
                                                    placeholder="e.g. 10.0.0.5 or my-host.local"
                                                    prop:value=custom_value
                                                    on:input=move |ev| {
                                                        let mut map = overrides.get();
                                                        map.insert(
                                                            target_key.clone(),
                                                            event_target_value(&ev),
                                                        );
                                                        overrides.set(map);
                                                    }
                                                />
                                            }
                                        }
                                    </Show>
                                </div>
                            }
                        }
                    />
                </div>
            </section>
        </Show>
    }
}
