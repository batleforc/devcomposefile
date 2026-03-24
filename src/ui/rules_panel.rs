use leptos::prelude::*;

#[component]
pub fn RulesPanel(
    runtime_rules_input: RwSignal<String>,
    ide_image_input: RwSignal<String>,
    startup_rules_status: RwSignal<String>,
) -> impl IntoView {
    let show_defaults = RwSignal::new(false);
    let default_rules_json =
        include_str!("../../assets/rules/default-rules.json").to_string();

    view! {
        <section class="panel two-col">
            <div>
                <h2>Rules</h2>
                <p class="status">{move || startup_rules_status.get()}</p>

                <div class="defaults-toggle">
                    <button
                        class="btn-link"
                        on:click=move |_| show_defaults.set(!show_defaults.get())
                    >
                        {move || {
                            if show_defaults.get() {
                                "Hide bundled defaults"
                            } else {
                                "Show bundled defaults"
                            }
                        }}
                    </button>
                </div>

                {move || {
                    if show_defaults.get() {
                        Some(view! {
                            <pre class="defaults-preview">{default_rules_json.clone()}</pre>
                        })
                    } else {
                        None
                    }
                }}

                <textarea
                    class="editor small"
                    placeholder="Optional runtime rules JSON (merged last, overrides defaults)"
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
    }
}
