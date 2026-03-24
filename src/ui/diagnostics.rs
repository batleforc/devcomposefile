use leptos::prelude::*;

#[component]
pub fn DiagnosticsPanel(diagnostics: RwSignal<Vec<String>>) -> impl IntoView {
    view! {
        <section class="panel">
            <h2>Diagnostics</h2>
            <ul class="diagnostics">
                <For
                    each=move || diagnostics.get()
                    key=|item| item.clone()
                    children=move |item| {
                        view! { <li>{item}</li> }
                    }
                />
            </ul>
        </section>
    }
}