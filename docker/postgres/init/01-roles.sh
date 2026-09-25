#!/bin/sh
# Spec §10 roles. Table privileges are granted by the migrations (crates/store/migrations).
# NOTE: runs only on an EMPTY pgdata volume; changing a password later needs ALTER ROLE by hand.
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
     -v scanner_pw="$SCANNER_DB_PASSWORD" -v analyze_pw="$ANALYZE_DB_PASSWORD" <<'SQL'
CREATE ROLE scanner_rw LOGIN PASSWORD :'scanner_pw';
CREATE ROLE analyze_ro LOGIN PASSWORD :'analyze_pw';
ALTER ROLE scanner_rw SET statement_timeout = '5s';
ALTER ROLE analyze_ro SET statement_timeout = '60s';
SQL
