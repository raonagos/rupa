use leptos::prelude::*;
use leptos_router::components::*;

#[component]
pub fn Navigation() -> impl IntoView {
    view! {
        <nav class="navigation">
            <A href="">"Dashboard"</A>
            <A href="groups">"Groups"</A>
            <A href="users">"Users"</A>
            <A href="settings">"Setting"</A>
        </nav>
    }
}
