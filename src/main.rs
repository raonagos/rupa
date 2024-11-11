#[cfg(feature = "server")]
#[tokio::main]
async fn main() {
    use leptos::logging;
    use rupa::cli::*;

    let cli = AppCli::parse();

    match cli.mode {
        Modes::Master => {
            logging::log!("Run in mode master");
            run_as_master().await
        }
        Modes::Edge => {
            logging::log!("Run in mode edge");
            run_as_edge().await
        }
    };
}

#[cfg(feature = "server")]
async fn run_as_master() {
    use axum::Router;
    use leptos::prelude::*;
    use leptos::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use rupa::app::*;

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    // build our application with a route
    let app = Router::new()
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    logging::log!("listening on http://{}", &addr);
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(feature = "server")]
async fn run_as_edge() {
    use axum::Router;
    use leptos::prelude::*;
    use leptos::*;

    let conf = get_configuration(None).unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;

    // build our application with a route
    let app = Router::new().route(
        "/api/*fn_name",
        axum::routing::post(leptos_axum::handle_server_fns),
    );

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    logging::log!("listening on http://{}", &addr);
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(feature = "server")]
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

#[cfg(not(feature = "server"))]
fn main() {
    use rupa::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
