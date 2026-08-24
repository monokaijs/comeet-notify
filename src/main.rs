use std::{env, process};

use comeet_notify::{AppState, Config, FcmClient, build_app};
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let config = Config::load();
    if env::args().nth(1).as_deref() == Some("healthcheck") {
        if let Err(error) = healthcheck(config.port).await {
            eprintln!("healthcheck failed: {error}");
            process::exit(1);
        }
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();

    let address = format!("0.0.0.0:{}", config.port);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            error!(%error, %address, "Failed to bind HTTP listener");
            process::exit(1);
        }
    };
    let fcm = FcmClient::new(config.firebase);
    let app = build_app(AppState::new(fcm));
    info!(
        %address,
        environment = %config.environment,
        "Comeet Notify is running"
    );
    info!("Swagger documentation is available at /docs");

    if let Err(error) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        error!(%error, "HTTP server stopped unexpectedly");
        process::exit(1);
    }
}

async fn healthcheck(port: u16) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{port}/");
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("server returned {}", response.status()));
    }
    let body = response.text().await.map_err(|error| error.to_string())?;
    if body != "Hello World!" {
        return Err("unexpected response body".to_owned());
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    info!("Shutdown signal received");
}
