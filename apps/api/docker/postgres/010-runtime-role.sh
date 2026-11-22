#!/bin/sh
set -eu

psql --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  --set=runtime_password="${POS_API_DEV_RUNTIME_PASSWORD:?POS_API_DEV_RUNTIME_PASSWORD is required}" \
  --set=operator_password="${POS_API_DEV_OPERATOR_PASSWORD:?POS_API_DEV_OPERATOR_PASSWORD is required}" \
  --file=/docker-entrypoint-initdb.d/020-runtime-role.sql
