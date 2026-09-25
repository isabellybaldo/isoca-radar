# Phase 0 results (fill in after the run)

Run date / machine: …

| Question | Answer | Evidence |
|---|---|---|
| Q7a: per-IP websocket connection cap | opened = …, first refusal at n = … (or none up to 40). Note: a `no frame within 15 s` detail is a timeout, not necessarily a refusal — read the detail column. | conncap.csv |
| Q7b: message rate for the NegRisk universe (run-5000 covers it) | … frames/s AND … events/s, from the `all` row of run-5000/report.md | run-5000/report.md |
| Q7c: bytes/day of raw capture | … bytes/day uncompressed (`all` row, dumps excluded); … dump bytes per reconnect; … compressed size (from raw_size.txt) | run-5000/report.md, run-5000/raw_size.txt |
| Q7d: stall rate vs shard size (50 / 500 / 5000) | … / … / … stalls_permille (per run, `all` row) | run-*/report.md |
| Q7e: chosen shard size + connection budget for S2c | … tokens/conn, … connections | reasoning: … |
| Q17: WSL2 clock offset (min lag; negative = server ahead) | min = … ms, lag_p1 = … ms. Note: min lag = clock offset + min one-way latency; reference: compare `docker run --rm debian:bookworm-slim date +%s%3N` against `w32tm /stripchart /computer:time.windows.com /samples:3 /dataonly` | run-*/report.md `min`, `p1` |
| L1 at scale: mirror on canary pairs | mirror = … / mismatch = … / missing = … | mirror.csv |
| Reconnects per run (count "reconnecting" lines) | … / … / … | results/run-*.log |
| pm-api on live traffic: parse errors | … per … frames; shapes to fix: … | run-*/errors.jsonl |
