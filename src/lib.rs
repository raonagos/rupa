mod api;
pub mod app;
#[cfg(feature = "server")]
pub mod cli;
mod components;
mod error;
mod modules;
mod pages;
mod repositories;

pub mod shared {
    #[cfg(feature = "server")]
    pub use crate::repositories::database::AppState;
}

#[cfg(feature = "server")]
type AppResult<T> = std::result::Result<T, error::AppError>;

#[cfg(feature = "web")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
