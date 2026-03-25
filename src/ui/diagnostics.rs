use leptos::prelude::*;

#[component]
pub fn DiagnosticsPanel(diagnostics: RwSignal<Vec<String>>) -> impl IntoView {
    let has_diags = move || !diagnostics.get().is_empty();
    let count = move || diagnostics.get().len();

    view! {
        <Show when=has_diags>
            <section class="panel diagnostics-panel">
                <div class="panel-header">
                    <h2>"Diagnostics"</h2>
                    <span class="diagnostics-badge">{count}</span>
                </div>
                <ul class="diagnostics">
                    <For
                        each=move || diagnostics.get()
                        key=|item| item.clone()
                        children=move |item| {
                            let level = classify_diagnostic(&item);
                            let icon = match level {
                                "error" => "\u{2718}",   // ✘
                                "warn" => "\u{26a0}",    // ⚠
                                _ => "\u{2139}",         // ℹ
                            };
                            view! {
                                <li class=format!("diag-item diag-{level}")>
                                    <span class="diag-icon">{icon}</span>
                                    <span class="diag-text">{item}</span>
                                </li>
                            }
                        }
                    />
                </ul>
            </section>
        </Show>
    }
}

fn classify_diagnostic(msg: &str) -> &'static str {
    let lower = msg.to_lowercase();
    if lower.contains("error")
        || lower.contains("failed")
        || lower.contains("invalid")
        || lower.contains("could not")
    {
        "error"
    } else if lower.contains("warning")
        || lower.contains("unsupported")
        || lower.contains("skipped")
        || lower.contains("no tool container")
        || lower.contains("no container components")
        || lower.contains("duplicate")
        || lower.contains("orphan")
    {
        "warn"
    } else {
        "info"
    }
}
