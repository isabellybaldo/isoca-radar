# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Polymarket API notes

- **`outcomePrices`** and **`clobTokenIds`** are JSON-encoded strings in Gamma API responses — decode them before use
- **`clobTokenIds`** (index 0 = YES token, index 1 = NO token) is what the CLOB API needs; do not use `conditionId` for price queries
- Pass `closed="false"` as a string, not a boolean, in Gamma API query params
