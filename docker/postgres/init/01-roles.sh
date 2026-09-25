#!/bin/sh
# Spec §10 roles. Table privileges are granted by the migrations (crates/store/migrations).
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<SQL
CREATE ROLE scanner_rw LOGIN PASSWORD '${SCANNER_DB_PASSWORD}';
CREATE ROLE analyze_ro LOGIN PASSWORD '${ANALYZE_DB_PASSWORD}';
ALTER ROLE scanner_rw SET statement_timeout = '5s';
ALTER ROLE analyze_ro SET statement_timeout = '60s';
SQL
