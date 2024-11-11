use leptos::prelude::*;

#[server(endpoint = "logout")]
pub async fn delete() -> Result<String, ServerFnError> {
    Ok(String::from("delete session ok"))
}
