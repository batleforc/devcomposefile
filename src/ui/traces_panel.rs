use leptos::prelude::*;

#[component]
pub fn TracesPanel(traces: RwSignal<Vec<String>>) -> impl IntoView {
    view! {
        <section class="panel traces-panel">
            <h2>Applied Rules</h2>
            <div class="traces-content">
                {move || {
                    let items = traces.get();
                    if items.is_empty() {
                        view! {
                            <p class="hint">
                                "No rules applied yet. Run a conversion to see which rules were applied."
                            </p>
                        }
                        .into_any()
                    } else {
                        view! {
                            <ul class="traces-list">
                                {items
                                    .into_iter()
                                    .map(|trace| view! { <li>{trace}</li> })
                                    .collect::<Vec<_>>()}
                            </ul>
                        }
                        .into_any()
                    }
                }}
            </div>
        </section>
    }
}
