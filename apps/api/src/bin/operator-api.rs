// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use pos_api::operator::{serve, OperatorConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [] => serve(OperatorConfig::from_env()?).await,
        [command, issuer, subject, display_name, reason] if command == "provision" => {
            let id = pos_api::operator_provisioning::provision_from_environment(
                issuer, subject, display_name, reason,
            )
            .await?;
            println!("Provisioned support operator account {id}.");
            Ok(())
        }
        [command, id, status, reason] if command == "set-status" => {
            let message = pos_api::operator_provisioning::set_status_from_environment(
                id, status, reason,
            )
            .await?;
            println!("{message}");
            Ok(())
        }
        [command, id, display_name, reason] if command == "set-display-name" => {
            let message = pos_api::operator_provisioning::set_display_name_from_environment(
                id,
                display_name,
                reason,
            )
            .await?;
            println!("{message}");
            Ok(())
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Usage: operator-api [serve] | provision <issuer> <subject> <display-name> <reason> | set-display-name <operator-id> <display-name> <reason> | set-status <operator-id> <active|suspended|deactivated> <reason>",
        )
        .into()),
    }
}
