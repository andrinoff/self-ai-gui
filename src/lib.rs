//! self-ai-gui: a chat assistant that remembers, served from one binary.

pub mod api;
pub mod assets;
pub mod config;
pub mod db;
pub mod memory;
pub mod model;
pub mod prompts;
pub mod upstream;

#[cfg(test)]
mod integration_tests;

use std::sync::Arc;

use tokio::net::TcpListener;

/// Boots config, storage and the model client, then serves until ctrl-c.
pub async fn main_entry() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = config::Config::from_env();
    let store = Arc::new(db::Store::open(&cfg.db_path)?);
    let upstream = Arc::new(upstream::Upstream::new(&cfg));

    let models = if cfg.models.is_empty() {
        "asking the provider".to_string()
    } else {
        cfg.models.join(", ")
    };
    let memory = if cfg.memory_enabled {
        format!("on (every {} turns)", cfg.memory_every.max(1))
    } else {
        "off".to_string()
    };
    println!("self-ai-gui");
    println!("  listening on http://{}", cfg.addr);
    println!("  model server {}", cfg.base_url);
    println!(
        "  key {}",
        if cfg.api_key.is_some() {
            "set"
        } else {
            "absent"
        }
    );
    println!("  models {models} (default {})", cfg.default_model);
    println!(
        "  memory {memory}, {} notes at most per prompt",
        cfg.memory_budget
    );
    println!("  database {}", cfg.db_path.display());

    let addr = cfg.addr.clone();
    let state = Arc::new(api::AppState::new(store, cfg, upstream));

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            println!("\nstopping");
        })
        .await?;
    Ok(())
}
