# Phase 0 probe: runbook (≈ 1.5 h, mostly waiting)

Everything runs in Docker Desktop (WSL2), so it measures the same environment S2c will run in.
Keep the PC awake for the whole run.

```powershell
# from the repo root
docker build -f spikes/probe/Dockerfile -t cute-probe .
docker volume create probe-data

# 1. universe (≈ 1 min)
docker run --rm -v probe-data:/data cute-probe universe --out /data/universe.json

# 2. connection cap (≈ 3 min: up to 40 connections, 3 s apart, stops at the first refusal)
docker run --rm -v probe-data:/data cute-probe conncap --universe /data/universe.json --out /data/conncap.csv

# If conncap `opened` < 16, add `--max-conns <opened − 2>` to every capture below.

# 3. captures, 20 min each; shard size is the variable (max 16 connections)
New-Item -ItemType Directory -Force spikes/probe/results | Out-Null
docker rm -f probe-run-50 2>$null
docker run --name probe-run-50 -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 50 --minutes 20 --out /data/run-50
cmd /c "docker logs probe-run-50 > spikes\probe\results\run-50.log 2>&1"
docker rm probe-run-50
docker rm -f probe-run-500 2>$null
docker run --name probe-run-500 -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 500 --minutes 20 --out /data/run-500
cmd /c "docker logs probe-run-500 > spikes\probe\results\run-500.log 2>&1"
docker rm probe-run-500
docker rm -f probe-run-5000 2>$null
docker run --name probe-run-5000 -v probe-data:/data cute-probe capture --universe /data/universe.json --shard-tokens 5000 --minutes 20 --out /data/run-5000
cmd /c "docker logs probe-run-5000 > spikes\probe\results\run-5000.log 2>&1"
docker rm probe-run-5000

# 4. mirror check (≈ 10 s)
docker run --rm -v probe-data:/data cute-probe mirror --universe /data/universe.json --out /data/mirror.csv

# 5. reports
docker run --rm -v probe-data:/data cute-probe report --dir /data/run-50
docker run --rm -v probe-data:/data cute-probe report --dir /data/run-500
docker run --rm -v probe-data:/data cute-probe report --dir /data/run-5000

# 6. copy the small outputs to spikes/probe/results (git-ignored; raw captures stay in the volume)
#    single quotes: PowerShell must not expand the $(...) meant for sh
docker run --rm -v probe-data:/data -v "${PWD}/spikes/probe/results:/out" debian:bookworm-slim sh -c 'cp /data/universe.json /data/conncap.csv /data/mirror.csv /out/ && for d in /data/run-*; do mkdir -p /out/$(basename $d); cp $d/report.md $d/shards.csv $d/errors.jsonl /out/$(basename $d)/; ls -l $d/raw.jsonl.zst > /out/$(basename $d)/raw_size.txt; done'
```

Then fill in `spikes/probe/RESULTS.md` (or paste the outputs to Claude, which will fill it in).
