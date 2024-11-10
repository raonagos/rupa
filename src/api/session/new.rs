use leptos::prelude::*;

#[server(endpoint = "login")]
pub async fn new() -> Result<String, ServerFnError> {
    Ok(String::from("new session ok"))
}
