mod config;
mod state;
mod db;
mod domain;
mod api;
mod services;
mod psp;

use crate::config::Config;
use crate::state::AppState;
use crate::db::{init_db, run_migrations, seed_demo_data};
use crate::psp::client::PspClient;
use crate::services::webhook::run_webhook_worker;
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("Starting Dodo Payments service...");
    
    // Load config
    let config = Config::from_env();
    
    // Init Database
    let pool = init_db(&config.database_url).await?;
    
    // Run Migrations
    run_migrations(&pool).await?;
    
    // Seed default credentials
    seed_demo_data(&pool, config.demo_business_id, &config.demo_api_key).await?;
    
    // Create PSP client
    let psp_client = PspClient::new(config.psp_url.clone());
    
    // Create AppState
    let state = AppState {
        db: pool.clone(),
        psp_client,
        config: config.clone(),
    };
    
    // Spawn Webhook delivery background worker
    tokio::spawn(run_webhook_worker(pool.clone()));
    
    // Build Axum router
    let app = api::create_router(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());
    
    // Start listener
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("Invoice API server listening on http://{}", addr);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
