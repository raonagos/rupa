use leptos::prelude::*;

#[server(endpoint = "greet")]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    use crate::repositories::database::AppState;

    let state = use_context::<AppState>();

    leptos::logging::log!("state {state:?}");

    let greetings_message = format!("Greetings {name}");
    Ok(greetings_message)
}
