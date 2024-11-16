use leptos::prelude::*;
use leptos_router::components::*;
use leptos_router::hooks::*;
use leptos_router::location::Location;

#[component]
pub fn Navigation() -> impl IntoView {
    let Location { pathname, .. } = use_location();
    // hide navigation at /login page
    let is_login_page = move || !pathname.get().contains("login");

    view! {
        <Show when=is_login_page>
            <nav class="navigation">
                <A href="">"Dashboard"</A>
                <A href="groups">"Groups"</A>
                <A href="users">"Users"</A>
                <A href="settings">"Setting"</A>
            </nav>
        </Show>
    }
}
