#[tokio::main]
async fn main() {
    use axum::Router;
    use leptos::logging;
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use rupa::app::*;
    use rupa::cli::*;
    use rupa::shared::AppState;
    use sqlx::SqlitePool;

    // #[cfg(feature = "sqlite")]
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("couldn't initiate sqlite");

    let conf = get_configuration(None).expect("couldn't get leptos config");
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    // dbg!(server_fn::axum::server_fn_paths().collect::<Vec<_>>());

    let app_state = AppState {
        leptos_options,
        pool: std::sync::Arc::new(pool),
    };

    let cli = AppCli::parse();

    let app = match cli.mode.clone() {
        Modes::Master => Router::new()
            .leptos_routes(&app_state, routes, {
                let leptos_options = app_state.leptos_options.clone();
                move || shell(leptos_options.clone())
            })
            .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
            .with_state(app_state),

        Modes::Edge => Router::new().route(
            "/*fn_name",
            axum::routing::any(leptos_axum::handle_server_fns).with_state(app_state),
        ),
    };

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("couldn't bind the address provided");
    logging::log!("listening on http://{}", &addr);
    axum::serve(listener, app.into_make_service())
        .await
        .expect("couldn't serve the app");
}

fn shell(options: leptos::prelude::LeptosOptions) -> impl leptos::prelude::IntoView {
    use leptos::prelude::*;
    use leptos_meta::*;
    use rupa::app::*;

    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options=options.clone()/>
                <MetaTags/>
                <HashedStylesheet options id="leptos"/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

// #[cfg(not(feature = "server"))]
// fn main() {
//     use rupa::app::*;
//     console_error_panic_hook::set_once();
//     leptos::mount::mount_to_body(App);
// }
