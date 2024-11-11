use crate::components::error::ErrorTemplate;
use crate::components::navigation::Navigation;
use crate::error::AppError;
use crate::pages::*;
use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::*;
use leptos_router::*;

/// Entry component to manage the VPN server.
#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();

    let fallback_error = || {
        let mut outside_errors = Errors::default();
        outside_errors.insert_with_default_key(AppError::NotFound);
        view! { <ErrorTemplate outside_errors/> }.into_view()
    };

    view! {
        <Title text="Rupā"/>

        <Router>
            <header>
                <Navigation/>
            </header>
            <main>
                <Routes fallback=fallback_error>
                    <Route path=path!("") view=DashboardPage/>
                    <Route path=path!("login") view=LoginPage/>
                </Routes>
            </main>
        </Router>
    }
}
