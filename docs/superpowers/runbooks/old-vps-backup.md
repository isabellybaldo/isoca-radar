# Back up the old VPS before it expires (2026-11-13): the database, the archive, the raw capture, the frozen JSON

Note: this copies everything isoca-radar has on the old VPS to `E:\isoca-radar-backup` on the PC, checks every file against a sha256 list made on the VPS, test-restores the database and queries the archive. Nothing on the VPS is changed except a temporary folder `/root/ct-backup` (removed in step 14).
Note: order of importance: database (steps 5–7) → Parquet archive (8–9) → raw capture (10–11). If time runs out, raw capture is the one part that may be skipped.
Note: PC steps run in **Windows PowerShell** (`$B` is set again at the top of each one). VPS steps run in the root shell that step 1 opens.
Note: the VPS asks for the root password 5 times: steps 1, 6, 8, 10, 14 (one connection each).
Note: never write a file with PowerShell's `>` here: it writes UTF-16 and breaks binary and JSON files. Every file is made on the VPS and downloaded with `sftp`.
Note: on the VPS: never print `.env` or a container's environment, never restart Docker, install nothing, never start scanner, analyze or compact.
Note: steps 8 and 10 land in `archive\_data` and `capture\_data` (sftp nests the remote folder inside the existing one); the checks in steps 9, 11 and 13 read from there.
Note: the transfers resume. If a download is cut (sleep, Wi-Fi), run the same step again: `reget` continues partial files and the check after it finds anything missing or broken.

## Prepare

1. (PC) The backup folder, the `isoca-old` SSH alias, the check script, then log in to the VPS.
```powershell
$B='E:\isoca-radar-backup'; New-Item -ItemType Directory -Force "$B\archive","$B\capture","$HOME\.ssh" | Out-Null
if (-not ((Test-Path "$HOME\.ssh\config") -and (Select-String -Quiet -Pattern '^Host isoca-old$' -Path "$HOME\.ssh\config"))) { Add-Content -Encoding ascii -Path "$HOME\.ssh\config" -Value "`r`nHost isoca-old`r`n  HostName 31.97.23.123`r`n  Port 50158`r`n  User root`r`n  ServerAliveInterval 30" }
@'
param([string]$Manifest, [string]$Root)
$n = 0; $bad = 0
foreach ($line in [IO.File]::ReadLines($Manifest)) {
  if ($line -notmatch '^([0-9a-f]{64}) [ *]\./(.+)$') { Write-Output "FAIL unparsed: $line"; $bad++; continue }
  $n++; $want = $Matches[1]; $rel = $Matches[2]
  $path = Join-Path $Root ($rel -replace '/', '\')
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { Write-Output "FAIL $rel (missing)"; $bad++; continue }
  $got = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLower()
  if ($got -ne $want) { Write-Output "FAIL $rel"; $bad++ }
}
if ($bad -eq 0) { Write-Output "ok $n files" } else { Write-Output "$bad FAILED of $n"; exit 1 }
'@ | Set-Content -Encoding ascii "$B\verify.ps1"
Get-ChildItem $B | Select-Object -ExpandProperty Name; ssh isoca-old
```
→ expected: `archive`, `capture`, `verify.ps1`, then the password prompt (password 1), then a root prompt on the VPS.
if not: `Permission denied` = wrong password, run `ssh isoca-old` again; `Could not resolve hostname isoca-old` = the alias wasn't written: `notepad $HOME\.ssh\config` and check the `Host isoca-old` block is there.

2. (VPS: the window showing `root@srv1125993:~#`) What is running, how big each volume is, how much disk is free.
```bash
docker ps --format '{{.Names}} {{.Status}}' | grep cute-trading; du -sh /var/lib/docker/volumes/cute-trading_{pgdata,archive,data,public-data}/_data; df -h /
```
→ expected: `cute-trading-postgres-1 Up …`, `cute-trading-api-1 Up …`, `cute-trading-web-1 Up …`, `cute-trading-oauth2-proxy-1 Up …` and no scanner, analyze or compact line; four sizes; the `Avail` of `/` bigger than the pgdata size. Write the four sizes down.
if not: a scanner, analyze or compact line = something restarted them: `docker stop cute-trading-scanner-1 cute-trading-analyze-1 cute-trading-compact-1`, then run this step again; `api` or `web` missing = STOP, tell Claude (step 4 needs both); `Avail` smaller than pgdata = go on, but step 5 uses its fallback.

3. (PC, a second PowerShell window; leave the VPS one open) The backup fits on E:.
```powershell
[int]((Get-PSDrive E).Free/1GB)
```
→ expected: a number in GB at least (pgdata + archive + data from step 2) × 1.2.
if not: pick another drive with room (C: has ~137 GB free) and use it as `$B` in every PC step, including `lcd` in steps 6, 8, 10.
**STOP**

## On the VPS: stage the small files

4. (VPS: the window showing `root@srv1125993:~#`, not PowerShell; logged out = `ssh isoca-old` first) The frozen JSON for the portfolio site, and the config files (not `.env`).
```bash
bash -euo pipefail <<'SH'
D=/root/ct-backup; mkdir -p $D/frozen $D/config; cd /home/isoca/projects/cute-trading
cp -r compose.yml compose.web.yml config $D/config/
docker exec cute-trading-web-1 cat /srv/ct-public-root/math-data/public.json > $D/frozen/public.json
for w in 7d 30d all; do docker exec cute-trading-web-1 wget -qO- "http://api:8081/api/cards?win=$w" > $D/frozen/cards-$w.json; done
docker exec cute-trading-web-1 wget -qO- http://api:8081/api/health/now > $D/frozen/health-now.json
wc -c $D/frozen/*.json; grep -c '"card":"SWEET"' $D/frozen/cards-*.json
SH
```
→ expected: five lines `<bytes> /root/ct-backup/frozen/<file>.json`, none `0`, `public.json` below 65536, a total; then `…/cards-7d.json:1`, `…/cards-30d.json:1`, `…/cards-all.json:1`.
if not: `wget: server returned error` or `bad address 'api'` = STOP, tell Claude with the line; a `:0` count = the API answered without a Sweet spots card: STOP, tell Claude.
**STOP**

5. (VPS: the window showing `root@srv1125993:~#`) The database dump and the sha256 lists (a few minutes; the lists read every file once).
```bash
bash -euo pipefail <<'SH'
D=/root/ct-backup
docker exec cute-trading-postgres-1 pg_dump -U ct_owner -d cute_trading -Fc -Z 6 > $D/cute_trading.dump
for v in archive data; do (cd /var/lib/docker/volumes/cute-trading_$v/_data && find . -type f -print0 | sort -z | xargs -0 -r nice -n 19 sha256sum) > $D/$v.sha256; done
(cd $D && find . -type f ! -name '*.sha256' -print0 | sort -z | xargs -0 sha256sum) > $D/ct-backup.sha256
ls -l $D/cute_trading.dump; wc -l $D/*.sha256; df -h /
SH
exit
```
→ expected: the dump's line with a size of many MB, three line counts (`archive.sha256` in the hundreds or more, `ct-backup.sha256` around 10–30, `data.sha256` may be small), `/` not at 100 %, then the VPS shell closes. Write the `archive.sha256` and `data.sha256` counts down.
if not: `No space left on device` = `rm -f /root/ct-backup/cute_trading.dump`, run this step again without the `docker exec … pg_dump` line, and after step 7 run the fallback below (one more password).
Fallback (PC, only when the dump couldn't be staged): `cmd`'s `>` is binary-safe, PowerShell's isn't.
```powershell
cmd /c "ssh isoca-old ""docker exec cute-trading-postgres-1 pg_dump -U ct_owner -d cute_trading -Fc -Z 6"" > E:\isoca-radar-backup\ct-backup\cute_trading.dump"; (Get-Item E:\isoca-radar-backup\ct-backup\cute_trading.dump).Length
```
→ expected: a size of many MB.
**STOP**

## On the PC: download and check

6. (PC) Download the staged folder (password 2). Type the three lines at the `sftp>` prompt.
```powershell
sftp isoca-old
```
```
lcd E:/isoca-radar-backup
reget -r /root/ct-backup
bye
```
→ expected: one progress line per file, ending at `100%`, no `error` or `Couldn't` line, back at the PowerShell prompt.
if not: cut off = run this step again.

7. (PC) Check the staged folder.
```powershell
$B='E:\isoca-radar-backup'; powershell -NoProfile -ExecutionPolicy Bypass -File "$B\verify.ps1" "$B\ct-backup\ct-backup.sha256" "$B\ct-backup"
```
→ expected: `ok <n> files` (the `ct-backup.sha256` count from step 5).
if not: `FAIL …` lines = run step 6 again, then this step.
**STOP**

8. (PC) Download the Parquet archive (password 3; the long one, tens of GB).
```powershell
sftp isoca-old
```
```
lcd E:/isoca-radar-backup
reget -r /var/lib/docker/volumes/cute-trading_archive/_data archive
bye
```
→ expected: progress lines for every day folder's files, ending at `100%`, no `error` or `Couldn't` line.
if not: cut off = run this step again; it continues where it stopped.

9. (PC) Check the archive.
```powershell
$B='E:\isoca-radar-backup'; powershell -NoProfile -ExecutionPolicy Bypass -File "$B\verify.ps1" "$B\ct-backup\archive.sha256" "$B\archive\_data"
```
→ expected: `ok <n> files` with `<n>` = the `archive.sha256` count from step 5.
if not: `FAIL <path> (missing)` = run step 8 again; `FAIL <path>` (no "missing") = delete that file under `$B\archive\_data`, run step 8 again, then this step.
**STOP**

10. (PC) Download the raw capture (password 4). Skippable only if time runs out: write down that it was skipped.
```powershell
sftp isoca-old
```
```
lcd E:/isoca-radar-backup
reget -r /var/lib/docker/volumes/cute-trading_data/_data capture
bye
```
→ expected: progress lines ending at `100%`, no `error` or `Couldn't` line.
if not: cut off = run this step again.

11. (PC) Check the raw capture.
```powershell
$B='E:\isoca-radar-backup'; powershell -NoProfile -ExecutionPolicy Bypass -File "$B\verify.ps1" "$B\ct-backup\data.sha256" "$B\capture\_data"
```
→ expected: `ok <n> files` with `<n>` = the `data.sha256` count from step 5.
if not: as step 9, with step 10 and `$B\capture\_data`.

12. (PC, Docker Desktop running) Restore the dump into a throwaway Postgres and count rows.
```powershell
$B='E:\isoca-radar-backup'; docker run -d --name ct-restore -e POSTGRES_PASSWORD=restore -v "${B}\ct-backup:/b" postgres:17 | Out-Null
do { Start-Sleep 2 } until ((docker exec ct-restore pg_isready -U postgres) -match 'accepting')
docker exec ct-restore sh -c 'pg_restore -l /b/cute_trading.dump | grep -c "TABLE DATA"'
docker exec ct-restore createdb -U postgres cute_trading
docker exec ct-restore pg_restore -U postgres -d cute_trading --no-owner --no-privileges /b/cute_trading.dump
docker exec ct-restore psql -U postgres -d cute_trading -Atc "select (select count(*) from episodes),(select count(*) from card_results),(select max(minute_ms) from health)"
docker rm -f ct-restore
```
→ expected: `26`, no error lines from `pg_restore`, then one line `<episodes>|<card_results>|<ms>` with three numbers above 0, then `ct-restore`. Write that line down: the third number is the frozen moment.
if not: a number other than `26`, or `pg_restore: error` lines = STOP, tell Claude with the output (keep the dump; don't redo step 5 yet).
**STOP**

13. (PC) DuckDB reads the archive.
```powershell
$B='E:\isoca-radar-backup'; python -m venv "$env:TEMP\ddb"; & "$env:TEMP\ddb\Scripts\pip.exe" -q install duckdb
Set-Content -Encoding ascii "$env:TEMP\ddb\q.py" 'import duckdb', 'print(duckdb.sql("select count(*) from read_parquet(''E:/isoca-radar-backup/archive/_data/*/*.parquet'', union_by_name=true)").fetchone())'
& "$env:TEMP\ddb\Scripts\python.exe" "$env:TEMP\ddb\q.py"
```
→ expected: one line like `(123456789,)` with a number above 0.
if not: `No files found` = the archive landed elsewhere: `Get-ChildItem $B\archive\_data | Select -First 3` should list `YYYY-MM-DD` folders; tell Claude what it shows.

14. (PC) Remove the staging folder on the VPS (password 5).
```powershell
ssh isoca-old "rm -r /root/ct-backup && echo cleaned"
```
→ expected: `cleaned`.

15. Tell Claude: step 12's line, whether step 10 was skipped, and that `E:\isoca-radar-backup\ct-backup\frozen` is ready.
