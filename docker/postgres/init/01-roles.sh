#!/bin/sh
# Spec §10 roles. Table privileges are granted by the migrations (crates/store/migrations).
# NOTE: runs only on an EMPTY pgdata volume; changing a password later needs ALTER ROLE by hand.
# analyze_rw (Phase 5a) is created LOGIN here when ANALYZE_RW_DB_PASSWORD is set; otherwise
# migration 0006 creates it NOLOGIN and the runbook activates it once. compact_rw (Phase 5b)
# likewise from COMPACT_RW_DB_PASSWORD (migration 0008 otherwise); paper_reset_rw (Phase 7a) from RESET_RW_DB_PASSWORD (0009).
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
     -v scanner_pw="$SCANNER_DB_PASSWORD" -v analyze_pw="$ANALYZE_DB_PASSWORD" <<'SQL'
CREATE ROLE scanner_rw LOGIN PASSWORD :'scanner_pw';
CREATE ROLE analyze_ro LOGIN PASSWORD :'analyze_pw';
ALTER ROLE scanner_rw SET statement_timeout = '5s';
ALTER ROLE analyze_ro SET statement_timeout = '60s';
SQL
if [ -n "${ANALYZE_RW_DB_PASSWORD:-}" ]; then
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
       -v analyze_rw_pw="$ANALYZE_RW_DB_PASSWORD" <<'SQL'
CREATE ROLE analyze_rw LOGIN PASSWORD :'analyze_rw_pw';
ALTER ROLE analyze_rw SET statement_timeout = '10s';
SQL
fi
if [ -n "${COMPACT_RW_DB_PASSWORD:-}" ]; then
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
       -v compact_rw_pw="$COMPACT_RW_DB_PASSWORD" <<'SQL'
CREATE ROLE compact_rw LOGIN PASSWORD :'compact_rw_pw';
ALTER ROLE compact_rw SET statement_timeout = '10s';
SQL
fi
# paper_reset_rw (Phase 7a) from RESET_RW_DB_PASSWORD (migration 0009 otherwise).
if [ -n "${RESET_RW_DB_PASSWORD:-}" ]; then
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB"        -v reset_rw_pw="$RESET_RW_DB_PASSWORD" <<'SQL'
CREATE ROLE paper_reset_rw LOGIN PASSWORD :'reset_rw_pw';
ALTER ROLE paper_reset_rw SET statement_timeout = '5s';
SQL
fi
