use gloo_net::http::Request;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::domain::git_fetch::{DEFAULT_COMPOSE_PATHS, RepoRef, parse_repo_url, raw_content_url};

/// Read a query-string parameter from the current page URL.
fn query_param(name: &str) -> Option<String> {
    let window = web_sys::window()?;
    let href = window.location().href().ok()?;
    let url = web_sys::Url::new(&href).ok()?;
    let value = url.search_params().get(name);
    value.filter(|v| !v.is_empty())
}

#[component]
pub fn GitRepoInput(
    compose_input: RwSignal<String>,
    git_context: RwSignal<Option<RepoRef>>,
) -> impl IntoView {
    // Seed signals from query parameters (?repo=...&ref=...&path=...)
    let initial_repo = query_param("repo").unwrap_or_default();
    let initial_ref = query_param("ref").unwrap_or_default();
    let initial_path = query_param("path").unwrap_or_default();
    let auto_fetch = !initial_repo.is_empty();

    let repo_url = RwSignal::new(initial_repo);
    let git_ref = RwSignal::new(initial_ref);
    let file_path = RwSignal::new(initial_path);
    let fetch_status = RwSignal::new(String::new());
    let fetching = RwSignal::new(false);

    let do_fetch = move || {
        let url_val = repo_url.get();
        if url_val.trim().is_empty() {
            fetch_status.set(String::from("Enter a repository URL."));
            return;
        }

        let ref_val = git_ref.get();
        let path_val = file_path.get();

        let ref_override = if ref_val.trim().is_empty() {
            None
        } else {
            Some(ref_val.clone())
        };
        let path_override = if path_val.trim().is_empty() {
            None
        } else {
            Some(path_val.clone())
        };

        let parsed =
            match parse_repo_url(&url_val, ref_override.as_deref(), path_override.as_deref()) {
                Ok(r) => r,
                Err(err) => {
                    fetch_status.set(err);
                    return;
                }
            };

        let raw_url = raw_content_url(&parsed);
        let using_default_path = path_val.trim().is_empty();
        fetch_status.set(format!("Fetching {}...", parsed.path));
        fetching.set(true);

        spawn_local(async move {
            // Try the primary URL first
            let result = Request::get(&raw_url).send().await;
            let (response_ok, text_result, final_parsed) = match result {
                Ok(response) if response.ok() => {
                    let text = response.text().await;
                    (true, text, parsed.clone())
                }
                Ok(response) if using_default_path && response.status() == 404 => {
                    // Fallback: try alternative compose file names
                    let mut found = None;
                    for &alt_path in DEFAULT_COMPOSE_PATHS.iter().skip(1) {
                        let mut alt = parsed.clone();
                        alt.path = String::from(alt_path);
                        let alt_url = raw_content_url(&alt);
                        fetch_status.set(format!("Trying {alt_path}..."));
                        if let Ok(alt_resp) = Request::get(&alt_url).send().await
                            && alt_resp.ok() {
                                let text = alt_resp.text().await;
                                found = Some((text, alt));
                                break;
                            }
                    }
                    match found {
                        Some((text, alt)) => (true, text, alt),
                        None => {
                            fetch_status.set(String::from(
                                "No docker-compose.yml or compose.yaml found in the repository.",
                            ));
                            fetching.set(false);
                            return;
                        }
                    }
                }
                Ok(response) => {
                    let code = response.status();
                    fetch_status.set(format!(
                        "Failed to fetch: HTTP {code}. Check the URL, branch, and file path."
                    ));
                    fetching.set(false);
                    return;
                }
                Err(err) => {
                    fetch_status.set(format!("Network error: {err}"));
                    fetching.set(false);
                    return;
                }
            };

            if response_ok {
                match text_result {
                    Ok(text) if !text.trim().is_empty() => {
                        let current = compose_input.get();
                        if current.trim().is_empty() {
                            compose_input.set(text);
                        } else {
                            compose_input.set(format!("{current}\n---\n{text}"));
                        }
                        git_context.set(Some(final_parsed.clone()));
                        fetch_status.set(format!(
                            "Loaded {}/{} @ {} — {}",
                            final_parsed.owner,
                            final_parsed.repo,
                            final_parsed.git_ref,
                            final_parsed.path
                        ));
                    }
                    _ => {
                        fetch_status.set(String::from("File fetched but content was empty."));
                    }
                }
            }
            fetching.set(false);
        });
    };

    // Auto-fetch when ?repo= query parameter was provided
    if auto_fetch {
        do_fetch();
    }

    let on_fetch = move |_| do_fetch();

    view! {
        <section class="panel git-repo-panel">
            <div class="panel-header">
                <h2>"Fetch from Git Repository"</h2>
            </div>
            <div class="git-repo-form">
                <div class="git-repo-row">
                    <input
                        class="git-input git-url"
                        type="text"
                        placeholder="https://github.com/owner/repo"
                        prop:value=move || repo_url.get()
                        on:input=move |ev| repo_url.set(event_target_value(&ev))
                    />
                </div>
                <div class="git-repo-row git-repo-details">
                    <input
                        class="git-input"
                        type="text"
                        placeholder="Branch / tag (default: main)"
                        prop:value=move || git_ref.get()
                        on:input=move |ev| git_ref.set(event_target_value(&ev))
                    />
                    <input
                        class="git-input"
                        type="text"
                        placeholder="File path (default: docker-compose.yml / compose.yaml)"
                        prop:value=move || file_path.get()
                        on:input=move |ev| file_path.set(event_target_value(&ev))
                    />
                    <button
                        class="btn-secondary"
                        on:click=on_fetch
                        disabled=move || fetching.get()
                    >
                        {move || if fetching.get() { "Fetching..." } else { "Fetch" }}
                    </button>
                </div>
            </div>
            <p class="hint">
                "Supports GitHub, GitLab, and Bitbucket public repositories. "
                "Paste a repo URL or a direct link to a Compose file."
            </p>
            {move || {
                let status = fetch_status.get();
                if status.is_empty() {
                    None
                } else {
                    Some(view! { <p class="status">{status}</p> })
                }
            }}
        </section>
    }
}
