# Measures everything cute-trading puts on disk and fails above the 30 GB budget.
# Docker Desktop's VM can't see the Windows drive filling up (smoke test 2026-09-25), and its
# shared virtual disk never shrinks on its own, so this counts the project's own items instead:
# the repo folder (target/, venv/, .git), its Docker volumes and images, the whole Docker build
# cache (shared, counted in full to stay conservative), and the container log caps.
#
#   powershell -ExecutionPolicy Bypass -File scripts\disk-budget.ps1
#
# Exit code: 0 within budget, 1 over budget, 2 docker unavailable.
param(
    [double]$LimitGB = 30,
    [double]$WarnGB = 25
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

# Docker prints decimal units ("2.882GB", "55.4MB", "4.1kB", "0B").
function ConvertFrom-DockerSize([string]$s) {
    if ($s -notmatch '^\s*([\d.]+)\s*([kKMGT]?B)') { return [double]0 }
    $mult = @{ 'B' = 1; 'kB' = 1e3; 'MB' = 1e6; 'GB' = 1e9; 'TB' = 1e12 }[$Matches[2]]
    return [double]$Matches[1] * $mult
}

docker info --format '{{.ServerVersion}}' *> $null
if ($LASTEXITCODE -ne 0) { Write-Host 'docker is not running: cannot measure the volumes'; exit 2 }

$rows = New-Object System.Collections.Generic.List[object]
function Add-Row([string]$item, [double]$bytes) { $rows.Add([pscustomobject]@{ Item = $item; GB = [math]::Round($bytes / 1e9, 2) }) }

$repoBytes = (Get-ChildItem -LiteralPath $repo -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
Add-Row 'repo folder (target, venv, .git)' $repoBytes

$volumes = docker system df -v --format '{{json .Volumes}}' | ConvertFrom-Json
foreach ($v in $volumes) {
    if ($v.Name -like 'cute-trading_*' -or $v.Name -eq 'probe-data') { Add-Row "volume $($v.Name)" (ConvertFrom-DockerSize $v.Size) }
}

$images = @('cute-trading/rust:dev', 'cute-probe:latest')
$pinned = Select-String -Path (Join-Path $repo 'compose.yml') -Pattern 'image:\s*(postgres:\S+)' | Select-Object -First 1
if ($pinned) { $images += $pinned.Matches[0].Groups[1].Value }
foreach ($img in $images) {
    $size = docker image inspect -f '{{.Size}}' $img 2>$null
    if ($LASTEXITCODE -eq 0) { Add-Row "image $img" ([double]$size) }
}

$cache = docker system df --format '{{json .}}' | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object Type -eq 'Build Cache'
Add-Row 'docker build cache (all projects)' (ConvertFrom-DockerSize $cache.Size)
Add-Row 'container log caps (3 x 50 MB)' 150e6

$total = ($rows | Measure-Object GB -Sum).Sum
$rows | Format-Table -AutoSize | Out-String | Write-Host
Write-Host ('total {0:N2} GB of {1} GB' -f $total, $LimitGB)
if ($total -gt $LimitGB) { Write-Host 'OVER BUDGET: prune before continuing'; exit 1 }
if ($total -gt $WarnGB) { Write-Host 'WARNING: close to the budget (a soak can add ~13 GB: capture 10 + Postgres ~1 + replay prefix ~3)' }
exit 0
