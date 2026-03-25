use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

#[component]
pub fn OutputPanel(yaml: RwSignal<String>) -> impl IntoView {
    let copy_label = RwSignal::new(String::from("Copy"));

    let on_copy = move |_| {
        let text = yaml.get();
        if text.is_empty() {
            return;
        }
        spawn_local(async move {
            let window = web_sys::window().expect("window");
            let clipboard = window.navigator().clipboard();
            let promise = clipboard.write_text(&text);
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
            copy_label.set(String::from("Copied!"));
            // Reset label after a brief delay via another spawn
            gloo_timers_compat_set_timeout(1200, move || {
                copy_label.set(String::from("Copy"));
            });
        });
    };

    let on_download = move |_| {
        let text = yaml.get();
        if text.is_empty() {
            return;
        }
        let array = js_sys::Array::new();
        array.push(&JsValue::from_str(&text));
        let opts = BlobPropertyBag::new();
        opts.set_type("text/yaml");
        let blob = Blob::new_with_str_sequence_and_options(&array, &opts).expect("blob");
        let url = Url::create_object_url_with_blob(&blob).expect("url");

        let window = web_sys::window().expect("window");
        let doc = window.document().expect("document");
        let a: HtmlAnchorElement = doc
            .create_element("a")
            .expect("create_element")
            .unchecked_into();
        a.set_href(&url);
        a.set_download("devfile.yaml");
        a.click();
        let _ = Url::revoke_object_url(&url);
    };

    view! {
        <section class="panel output-panel">
            <div class="panel-header">
                <h2>Generated Devfile</h2>
                <div class="panel-actions">
                    <button
                        class="btn-secondary"
                        on:click=on_copy
                        disabled=move || yaml.get().is_empty()
                    >
                        {move || copy_label.get()}
                    </button>
                    <button
                        class="btn-secondary"
                        on:click=on_download
                        disabled=move || yaml.get().is_empty()
                    >
                        "Download"
                    </button>
                </div>
            </div>
            <pre class="editor yaml-highlighted"><code inner_html=move || {
                crate::ui::yaml_highlight::highlight_yaml(&yaml.get())
            }></code></pre>
        </section>
    }
}

/// Simple timeout helper using wasm_bindgen closure on window.setTimeout
fn gloo_timers_compat_set_timeout(ms: i32, f: impl Fn() + 'static) {
    let closure = Closure::wrap(Box::new(f) as Box<dyn Fn()>);
    let window = web_sys::window().expect("window");
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        ms,
    );
    closure.forget();
}
