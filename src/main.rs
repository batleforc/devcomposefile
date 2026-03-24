#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(devcomposefile::app::App);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Run this app in the browser with `trunk serve`.");
}
