# Phase 0 results (fill in after the run)

Run date / machine: …

| Question | Answer | Evidence |
|---|---|---|
| Q7a: per-IP websocket connection cap | opened = …, first refusal at n = … (or none up to 40) | conncap.csv |
| Q7b: message rate for the NegRisk universe (run-5000 covers it) | … frames/s | run-5000/report.md |
| Q7c: bytes/day of raw capture (uncompressed) | … | run-5000/report.md |
| Q7d: stall rate vs shard size (50 / 500 / 5000) | … / … / … stalls | run-*/report.md |
| Q7e: chosen shard size + connection budget for S2c | … tokens/conn, … connections | reasoning: … |
| Q17: WSL2 clock offset (min lag; negative = server ahead) | … ms | run-*/report.md `min` |
| L1 at scale: mirror on canary pairs | mirror = … / mismatch = … / missing = … | mirror.csv |
| Reconnects per run (count "reconnecting" lines) | … / … / … | results/run-*.log |
| pm-api on live traffic: parse errors | … per … frames; shapes to fix: … | run-*/errors.jsonl |
