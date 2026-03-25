use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use web_sys::{DragEvent, FileList, FileReader, HtmlInputElement, HtmlTextAreaElement};

#[component]
pub fn ComposeInput(compose_input: RwSignal<String>) -> impl IntoView {
    let dragging = RwSignal::new(false);
    let highlight_ref = NodeRef::<leptos::html::Pre>::new();

    let read_files = move |files: FileList| {
        for i in 0..files.length() {
            if let Some(file) = files.get(i) {
                let reader = FileReader::new().expect("FileReader");
                let reader_clone = reader.clone();
                let closure = Closure::wrap(Box::new(move || {
                    if let Ok(result) = reader_clone.result()
                        && let Some(text) = result.as_string()
                    {
                        let current = compose_input.get();
                        if current.trim().is_empty() {
                            compose_input.set(text);
                        } else {
                            compose_input.set(format!("{current}\n---\n{text}"));
                        }
                    }
                }) as Box<dyn Fn()>);
                reader.set_onloadend(Some(closure.as_ref().unchecked_ref()));
                closure.forget();
                let _ = reader.read_as_text(&file);
            }
        }
    };

    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        dragging.set(false);
        if let Some(dt) = ev.data_transfer()
            && let Some(files) = dt.files()
        {
            read_files(files);
        }
    };

    let on_dragover = move |ev: DragEvent| {
        ev.prevent_default();
        dragging.set(true);
    };

    let on_dragleave = move |ev: DragEvent| {
        ev.prevent_default();
        dragging.set(false);
    };

    let on_file_pick = move |ev: leptos::ev::Event| {
        let target: HtmlInputElement = event_target(&ev);
        if let Some(files) = target.files() {
            read_files(files);
        }
    };

    let file_input_ref = NodeRef::<leptos::html::Input>::new();
    let on_browse = move |_| {
        if let Some(input) = file_input_ref.get() {
            input.click();
        }
    };

    view! {
        <section class="panel compose-input-panel">
            <div class="panel-header">
                <h2>Compose Input</h2>
                <div class="panel-actions">
                    <button class="btn-secondary" on:click=on_browse>
                        "Upload Files"
                    </button>
                    <input
                        node_ref=file_input_ref
                        type="file"
                        accept=".yml,.yaml"
                        multiple=true
                        style="display:none"
                        on:change=on_file_pick
                    />
                    <button
                        class="btn-secondary"
                        on:click=move |_| compose_input.set(String::new())
                    >
                        "Clear"
                    </button>
                </div>
            </div>
            <div
                class=move || {
                    if dragging.get() { "drop-zone dragging" } else { "drop-zone" }
                }
                on:drop=on_drop
                on:dragover=on_dragover
                on:dragleave=on_dragleave
            >
                <div class="editor-highlight-wrap">
                    <pre class="editor-highlight yaml-highlighted" aria-hidden="true" node_ref=highlight_ref><code inner_html=move || {
                        let src = compose_input.get();
                        if src.is_empty() {
                            String::new()
                        } else {
                            crate::ui::yaml_highlight::highlight_yaml(&src)
                        }
                    }></code></pre>
                    <textarea
                        class="editor editor-overlay"
                        placeholder="Paste Docker Compose YAML here, or drag & drop .yml files. For multiple files, separate with ---"
                        prop:value=move || compose_input.get()
                        on:input=move |ev| compose_input.set(event_target_value(&ev))
                        on:scroll=move |ev| {
                            let ta: HtmlTextAreaElement = event_target(&ev);
                            if let Some(pre) = highlight_ref.get() {
                                let el: &web_sys::Element = &pre;
                                el.set_scroll_top(ta.scroll_top());
                                el.set_scroll_left(ta.scroll_left());
                            }
                        }
                        spellcheck="false"
                    ></textarea>
                </div>
            </div>
        </section>
    }
}
