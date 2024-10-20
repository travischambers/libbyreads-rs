#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use dotenv::dotenv;
    use leptos::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use libbyreads_rs::app::*;
    use libbyreads_rs::fileserv::file_and_error_handler;
    use opentelemetry::KeyValue;
    use opentelemetry_appender_tracing::layer;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::logs::LoggerProvider;
    use opentelemetry_sdk::Resource;
    use std::env;
    use std::time::Duration;
    use tracing::info;
    use tracing_subscriber;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::EnvFilter;

    dotenv().ok();

    console_error_panic_hook::set_once();

    // tracing_subscriber::fmt::init();

    let export_config = opentelemetry_otlp::ExportConfig {
        endpoint: env::var("HONEYCOMB_LOG_API_ENDPOINT")
            .expect("HONEYCOMB_LOG_API_ENDPOINT not set"),
        protocol: opentelemetry_otlp::Protocol::HttpBinary,
        timeout: Duration::from_secs(3),
    };
    let log_exporter = opentelemetry_otlp::new_exporter()
        .http()
        .with_export_config(export_config)
        .with_headers({
            let mut headers = std::collections::HashMap::new();
            headers.insert(
                "x-honeycomb-team".to_string(),
                env::var("HONEYCOMB_API_KEY").expect("HONEYCOMB_API_KEY not set"),
            );
            headers.insert(
                "x-honeycomb-dataset".to_string(),
                env::var("HONEYCOMB_DATASET").expect("HONEYCOMB_DATASET not set"),
            );
            headers
        })
        .build_log_exporter()
        .unwrap();
    let resource = Resource::new(vec![
        KeyValue::new("service.name", "libbyreads"),
        KeyValue::new("service.version", "0.1.0"),
    ]);
    let logger_provider = LoggerProvider::builder()
        .with_batch_exporter(log_exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(resource)
        .build();

    let logger_layer = layer::OpenTelemetryTracingBridge::new(&logger_provider);
    let env_filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let subscriber = tracing_subscriber::registry().with(env_filter_layer);

    tracing::subscriber::set_global_default(subscriber.with(logger_layer)).unwrap();
    info!("Starting server");
    // Setting get_configuration(None) means we'll be using cargo-leptos's env values
    // For deployment these variables are:
    // <https://github.com/leptos-rs/start-axum#executing-a-server-on-a-remote-machine-without-the-toolchain>
    // Alternately a file can be specified such as Some("Cargo.toml")
    // The file would need to be included with the executable when moved to deployment
    let conf = get_configuration(None).await.unwrap();
    let leptos_options = conf.leptos_options;
    let addr = leptos_options.site_addr;
    let routes = generate_route_list(App);

    // build our application with a route
    let app = Router::new()
        .leptos_routes(&leptos_options, routes, App)
        .fallback(file_and_error_handler)
        .with_state(leptos_options);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!("listening on http://{}", &addr);
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for a purely client-side app
    // see lib.rs for hydration function instead
}
