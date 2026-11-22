// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::{env, error::Error, io};

use sqlx::{migrate::Migrator, postgres::PgPoolOptions};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations/postgres");

pub async fn run_from_environment() -> Result<(), Box<dyn Error>> {
    let database_url = env::var("POS_API_MIGRATION_DATABASE_URL").map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "POS_API_MIGRATION_DATABASE_URL must be set for the migrate command.",
        )
    })?;

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    MIGRATOR.run(&pool).await?;
    pool.close().await;
    Ok(())
}
