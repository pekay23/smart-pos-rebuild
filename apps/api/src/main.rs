// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::{env, error::Error, io, time::Duration};

use pos_api::{app_with_runtime, Config};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => serve().await,
        [command] if command == "migrate" => migrate().await,
        [command] if command == "pickup-expiry-worker" => pickup_expiry_worker().await,
        [command] if command == "attendance-expiry-worker" => attendance_expiry_worker().await,
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Usage: pos-api [migrate|pickup-expiry-worker]",
        )
        .into()),
    }
}

async fn serve() -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;
    let identity_database = match config.database_options {
        Some(database_options) => Some(
            PgPoolOptions::new()
                .max_connections(8)
                .acquire_timeout(Duration::from_secs(3))
                .connect_with(database_options)
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        "POS_API_DATABASE_URL could not connect to the runtime database.",
                    )
                })?,
        ),
        None => None,
    };
    let listener = TcpListener::bind(config.bind_address).await?;
    eprintln!("Advanced POS API listening on {}", listener.local_addr()?);
    eprintln!(
        "Licensed under AGPL-3.0-or-later. Source: https://github.com/Pekay23/smart-pos-rebuild"
    );
    axum::serve(
        listener,
        app_with_runtime(
            config.allowed_origins,
            config.authenticator,
            identity_database,
        ),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn migrate() -> Result<(), Box<dyn Error>> {
    eprintln!("Applying embedded PostgreSQL migrations.");
    pos_api::run_migrations().await?;
    eprintln!("PostgreSQL migrations are up to date.");
    Ok(())
}

async fn pickup_expiry_worker() -> Result<(), Box<dyn Error>> {
    run_scheduled_worker(
        "POS_API_EXPIRY_DATABASE_URL",
        "Pickup expiry",
        "SELECT pos_worker.materialize_expired_pickup_reservation()",
    )
    .await
}

async fn attendance_expiry_worker() -> Result<(), Box<dyn Error>> {
    run_scheduled_worker(
        "POS_API_ATTENDANCE_DATABASE_URL",
        "Attendance scheduled-end",
        "SELECT pos_worker.end_next_due_attendance_period()",
    )
    .await
}

async fn run_scheduled_worker(
    database_url_name: &str,
    worker_name: &str,
    routine_sql: &'static str,
) -> Result<(), Box<dyn Error>> {
    let database_url = env::var(database_url_name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{database_url_name} is required for the {worker_name} worker."),
        )
    })?;
    let pool =
        PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&database_url)
            .await
            .map_err(|_| {
                io::Error::new(
                io::ErrorKind::ConnectionRefused,
            format!("The {worker_name} worker could not connect to its restricted database role."),
            )
            })?;
    eprintln!("{worker_name} worker started.");
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let mut retry_delay = Duration::from_secs(5);

    loop {
        let mut materialized = 0_u16;
        let mut failed = false;
        for _ in 0..100 {
            let result = tokio::select! {
                _ = &mut shutdown => {
                    eprintln!("{worker_name} worker stopped.");
                    pool.close().await;
                    return Ok(());
                }
                result = sqlx::query_scalar::<_, bool>(routine_sql).fetch_one(&pool) => result,
            };
            match result {
                Ok(true) => materialized += 1,
                Ok(false) => break,
                Err(error) => {
                    let code = error
                        .as_database_error()
                        .and_then(|database| database.code())
                        .map(|value| value.into_owned())
                        .unwrap_or_else(|| "connection_error".to_owned());
                    eprintln!("{worker_name} pass stopped after a database error (code={code}).");
                    failed = true;
                    break;
                }
            }
        }
        if materialized > 0 {
            eprintln!("{worker_name} pass completed (materialized={materialized}).");
        }

        let delay = if failed {
            let jitter_ms = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_millis()
                % 1_000) as u64;
            let delay = retry_delay + Duration::from_millis(jitter_ms);
            retry_delay = (retry_delay * 2).min(Duration::from_secs(60));
            delay
        } else {
            retry_delay = Duration::from_secs(5);
            if materialized == 100 {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(5)
            }
        };
        tokio::select! {
            _ = &mut shutdown => {
                    eprintln!("{worker_name} worker stopped.");
                pool.close().await;
                return Ok(());
            }
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("Could not listen for Ctrl+C: {error}");
    }
}
