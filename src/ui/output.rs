use leptos::prelude::*;

#[component]
pub fn OutputPanel(yaml: RwSignal<String>) -> impl IntoView {
    view! {
        <section class="panel">
            <h2>Generated Devfile</h2>
            <textarea class="editor" readonly=true prop:value=move || yaml.get()></textarea>
        </section>
    }
}