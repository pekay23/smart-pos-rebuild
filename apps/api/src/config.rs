// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use std::{
    env,
    net::{IpAddr, SocketAddr},
};

use axum::http::HeaderValue;
use sqlx::postgres::{PgConnectOptions, PgSslMode};

use crate::auth::Authenticator;

#[derive(Clone)]
pub struct Config {
    pub bind_address: SocketAddr,
    pub allowed_origins: Vec<HeaderValue>,
    pub authenticator: Option<Authenticator>,
    pub database_options: Option<PgConnectOptions>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind_address = env::var("POS_API_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
            .parse::<SocketAddr>()
            .map_err(|_| "POS_API_BIND must be a valid socket address.".to_owned())?;
        let allowed_origins = env::var("POS_API_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(parse_origin)
            .collect::<Result<Vec<_>, _>>()?;
        let authenticator = Authenticator::from_env()?;
        let database_options = env::var("POS_API_DATABASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|url| {
                let allow_insecure_docker_database =
                    env::var("POS_API_ALLOW_INSECURE_DOCKER_DATABASE")
                        .is_ok_and(|value| value.eq_ignore_ascii_case("true"));
                parse_runtime_database_options(&url, allow_insecure_docker_database)
            })
            .transpose()?;

        Ok(Self {
            bind_address,
            allowed_origins,
            authenticator,
            database_options,
        })
    }
}

fn parse_runtime_database_options(
    url: &str,
    allow_insecure_docker_database: bool,
) -> Result<PgConnectOptions, String> {
    let options = url.parse::<PgConnectOptions>().map_err(|_| {
        "POS_API_DATABASE_URL must be a valid PostgreSQL connection URL.".to_owned()
    })?;
    let host = options.get_host();
    let is_local = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let is_compose_database = host.eq_ignore_ascii_case("postgres");
    if allow_insecure_docker_database && (!cfg!(debug_assertions) || !is_compose_database) {
        return Err(
            "POS_API_ALLOW_INSECURE_DOCKER_DATABASE is limited to the postgres Compose service in debug builds."
                .to_owned(),
        );
    }
    if !is_local
        && !matches!(options.get_ssl_mode(), PgSslMode::VerifyFull)
        && !allow_insecure_docker_database
    {
        return Err(
            "POS_API_DATABASE_URL must use sslmode=verify-full for non-local hosts.".to_owned(),
        );
    }
    Ok(options.options([("statement_timeout", "5000")]))
}

fn parse_origin(origin: &str) -> Result<HeaderValue, String> {
    let is_http_origin = origin.starts_with("http://") || origin.starts_with("https://");
    let has_path_or_query = origin
        .split_once("://")
        .is_some_and(|(_, authority)| authority.is_empty() || authority.contains(['/', '?', '#']));
    if !is_http_origin || has_path_or_query || origin == "*" {
        return Err("POS_API_ALLOWED_ORIGINS must contain exact http(s) origins without paths or wildcards.".to_owned());
    }
    HeaderValue::from_str(origin)
        .map_err(|_| "POS_API_ALLOWED_ORIGINS contains an invalid origin.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wildcard_and_path_origins() {
        assert!(parse_origin("*").is_err());
        assert!(parse_origin("https://example.com/app").is_err());
        assert!(parse_origin("https://example.com").is_ok());
    }

    #[test]
    fn hosted_runtime_database_requires_verified_tls_but_loopback_allows_local_development() {
        assert!(
            parse_runtime_database_options("postgres://user:pass@db.example/pos", false).is_err()
        );
        assert!(parse_runtime_database_options(
            "postgres://user:pass@db.example/pos?sslmode=verify-full",
            false
        )
        .is_ok());
        assert!(parse_runtime_database_options(
            "postgres://user:pass@127.0.0.1/pos?sslmode=disable",
            false
        )
        .is_ok());
        assert!(parse_runtime_database_options("not a postgres url", false).is_err());
    }

    #[test]
    fn insecure_database_override_is_opt_in_and_explicitly_scoped_to_debug_runtime() {
        assert!(parse_runtime_database_options(
            "postgres://user:pass@postgres/pos?sslmode=disable",
            false
        )
        .is_err());
        assert!(parse_runtime_database_options(
            "postgres://user:pass@postgres/pos?sslmode=disable",
            true
        )
        .is_ok());
        assert!(parse_runtime_database_options(
            "postgres://user:pass@db/pos?sslmode=disable",
            true
        )
        .is_err());
    }
}
