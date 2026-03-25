use std::collections::BTreeMap;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{FileList, FileReader, HtmlInputElement};

#[component]
pub fn IncludeFilesPanel(file_registry: RwSignal<BTreeMap<String, String>>) -> impl IntoView {
    let on_files = move |files: FileList| {
        for i in 0..files.length() {
            let Some(file) = files.get(i) else {
                continue;
            };
            let name = file.name();
            let reader = FileReader::new().unwrap();
            let reader_clone = reader.clone();
            let file_registry = file_registry;

            let onload = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                if let Ok(result) = reader_clone.result()
                    && let Some(text) = result.as_string()
                {
                    file_registry.update(|reg| {
                        reg.insert(name.clone(), text);
                    });
                }
            }) as Box<dyn FnMut(_)>);

            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            let _ = reader.read_as_text(&file);
        }
    };

    let on_input_change = move |ev: leptos::ev::Event| {
        let target: HtmlInputElement = ev.target().unwrap().unchecked_into();
        if let Some(files) = target.files() {
            on_files(files);
        }
    };

    let on_remove = move |path: String| {
        file_registry.update(|reg| {
            reg.remove(&path);
        });
    };

    view! {
        <details class="panel collapsible include-panel">
            <summary class="panel-summary">
                <h2>"Include Files"</h2>
                <span class="badge">{move || file_registry.get().len()}</span>
            </summary>

            <div class="include-body">
                <p class="hint">
                    "Upload files referenced by Compose "
                    <code>"include"</code>
                    " directives. File names are matched against include paths."
                </p>
                <input
                    type="file"
                    multiple=true
                    accept=".yml,.yaml"
                    on:change=on_input_change
                />

                <ul class="include-list">
                    {move || {
                        let reg = file_registry.get();
                        if reg.is_empty() {
                            return vec![
                                view! {
                                    <li>
                                        <span class="include-path">"No include files uploaded yet."</span>
                                    </li>
                                }.into_any(),
                            ];
                        }
                        reg.keys()
                            .map(|path| {
                                let p = path.clone();
                                let p2 = path.clone();
                                view! {
                                    <li>
                                        <span class="include-path">{p}</span>
                                        <button
                                            class="btn-remove"
                                            on:click=move |_| on_remove(p2.clone())
                                        >
                                            "×"
                                        </button>
                                    </li>
                                }.into_any()
                            })
                            .collect::<Vec<_>>()
                    }}
                </ul>
            </div>
        </details>
    }
}
