# Runbook: scanner and analyze come back after a VPS reboot

Two changes: `analyze` → `restart: unless-stopped` (compose.yml), plus a systemd unit that starts the
scanner at boot. All steps run on the VPS as root, in `/home/isoca/projects/cute-trading`.

1. Docker path the unit uses
   ```bash
   command -v docker
   ```
   → expected: `/usr/bin/docker`
   if not: edit `ExecStart` in step 3's file to the printed path.

2. New compose.yml in place (from the deploy bundle or the repo) and valid
   ```bash
   cd /home/isoca/projects/cute-trading
   grep -n -A1 'restart: unless-stopped' compose.yml | grep -c unless-stopped
   docker compose --env-file .env.example -f compose.yml -f compose.web.yml --profile core --profile web config -q && echo config-ok
   ```
   → expected: `2` (postgres and analyze), then `config-ok` — **STOP** if not.

3. Install the unit
   ```bash
   cat > /etc/systemd/system/isoca-radar-scanner.service <<'EOF'
   [Unit]
   Description=isoca-radar: start the scanner at boot
   Requires=docker.service
   After=docker.service network-online.target
   Wants=network-online.target

   [Service]
   Type=oneshot
   RemainAfterExit=yes
   WorkingDirectory=/home/isoca/projects/cute-trading
   ExecStart=/usr/bin/docker compose --profile core up -d --no-build scanner
   TimeoutStartSec=300

   [Install]
   WantedBy=multi-user.target
   EOF
   systemctl daemon-reload && systemctl enable isoca-radar-scanner.service && systemctl is-enabled isoca-radar-scanner.service
   ```
   → expected: `Created symlink …` then `enabled`.

4. Apply analyze's new restart policy (recreates analyze only; the scanner keeps running)
   ```bash
   docker compose --profile core up -d --no-build --no-deps analyze
   docker inspect -f '{{.HostConfig.RestartPolicy.Name}}' cute-trading-analyze-1 cute-trading-scanner-1
   ```
   → expected: `unless-stopped` then `on-failure`.

5. Planned reboot (1–2 min without capture)
   ```bash
   sudo reboot
   ```
   → expected: the SSH session closes. Reconnect after ~1 min.

6. Everything came back by itself
   ```bash
   cd /home/isoca/projects/cute-trading
   systemctl status isoca-radar-scanner.service --no-pager | grep -E 'Active|status='
   docker compose ps --format '{{.Service}} {{.Status}}'
   ```
   → expected: `Active: active (exited)` with `status=0/SUCCESS`; postgres, scanner, analyze, api, web, oauth2-proxy all `Up`.
   if not: `journalctl -u isoca-radar-scanner --no-pager | tail -20` and stop here.

7. A new live scanner run, and capture flowing
   ```bash
   docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT run_id, version, mode, exit_reason FROM runs ORDER BY run_id DESC LIMIT 1"
   sleep 90; docker compose exec -T postgres psql -U ct_owner -d cute_trading -tAc "SELECT to_char(to_timestamp(minute_ms/1000) AT TIME ZONE 'UTC','HH24:MI'), msgs, lag_p50_ms, clock_offset_ms FROM health ORDER BY minute_ms DESC LIMIT 2"
   ```
   → expected: a new run, `live`, empty exit reason; two health rows from the last minutes, msgs > 0, lag p50 ~75, clock offset within ±250.

Keep the scanner down across a reboot (maintenance):
```bash
systemctl disable isoca-radar-scanner.service      # after: systemctl enable isoca-radar-scanner.service
```
→ expected: `Removed …`.

Rollback:
```bash
systemctl disable --now isoca-radar-scanner.service && rm /etc/systemd/system/isoca-radar-scanner.service && systemctl daemon-reload
```
→ expected: `Removed …`; compose.yml's old `on-failure:5` for analyze returns with the previous bundle.
