# Daily sync with upstream (tidewarden/tli-vorax-simulator):
#   1. pulls upstream/ (clones it the first time)
#   2. exports its data (odds, cards, board-value model, the screen reader's references) to data/
#   3. re-records the reference traces from its TypeScript engine, bot, advisor and screen reader into fixtures/
#   4. runs the Rust parity tests against them and rebuilds the release binaries (vorax.exe, vorax-overlay.exe)
# Exit code 0: the Rust port plays exactly like upstream with today's data. Non-zero: something to port (the
# output names the card, instrument or step that differs); the previous binary is left in place.
#   pwsh scripts/sync.ps1            # full sync
#   pwsh scripts/sync.ps1 -Quick     # skip the advisor traces (about a minute faster)
param([switch]$Quick)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$up = Join-Path $root 'upstream'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) { $cargo = 'cargo' }
$log = Join-Path $root 'sync.log'
function Say($m) { $line = "$(Get-Date -Format 'yyyy-MM-dd HH:mm') $m"; Write-Host $line; Add-Content -Path $log -Value $line }
# '[n/6] what': the overlay's "Sync and rebuild" shows these as its progress
function Step($n, $m) { Write-Host "[$n/6] $m" }

Step 1 'Pulling upstream'

if (-not (Test-Path (Join-Path $up '.git'))) {
  Say 'Cloning upstream'
  git clone --quiet https://github.com/tidewarden/tli-vorax-simulator.git $up
  if ($LASTEXITCODE) { throw 'git clone failed' }
}

$before = git -C $up rev-parse HEAD
git -C $up pull --ff-only --quiet
if ($LASTEXITCODE) { throw 'git pull failed (local changes in upstream/?)' }
$after = git -C $up rev-parse HEAD

if ($before -ne $after) {
  $changed = git -C $up diff --name-only $before $after
  # rule code (not just data): what may need porting by hand
  $rules = $changed | Where-Object { $_ -match '^src/(engine|strategy)/|^src/data/(potions/|instruments|archetypes|defs|shelley)|^src/solver/(scan/.*\.ts|track\.ts|live/(triggers|windowCrop)\.ts)' }
  Say "Upstream $($before.Substring(0,7)) -> $($after.Substring(0,7)): $(@($changed).Count) files changed"
  if ($rules) { Say "Rule code changed upstream (check the port): $($rules -join ', ')" }
  if ($changed -contains 'package-lock.json') { $needInstall = $true }
} else {
  Say "Upstream unchanged at $($after.Substring(0,7))"
}
if ($needInstall -or -not (Test-Path (Join-Path $up 'node_modules'))) {
  Step 2 'Installing upstream dependencies'
  Say 'Installing upstream dependencies'
  Push-Location $up; npm ci --no-audit --no-fund --silent; $code = $LASTEXITCODE; Pop-Location
  if ($code) { throw 'npm ci failed' }
}

Push-Location $up
try {
  Step 3 "Exporting upstream's data"
  npx vite-node ../tools/export-data.ts ../data
  if ($LASTEXITCODE) { throw 'data export failed' }
  npx vite-node ../tools/export-scan.ts ../data/scan
  if ($LASTEXITCODE) { throw 'reader data export failed' }
  Step 4 "Recording upstream's traces"
  if ($Quick) { $env:ADVISED = '0' } else { $env:ADVISED = '8' }
  npx vite-node ../tools/record-traces.ts ../fixtures 300
  if ($LASTEXITCODE) { throw 'trace recording failed' }
  npx vite-node ../tools/record-scan.ts ../fixtures
  if ($LASTEXITCODE) { throw 'reader trace recording failed' }
  npx vite-node ../tools/record-live.ts ../fixtures
  if ($LASTEXITCODE) { throw 'log trace recording failed' }
} finally {
  Remove-Item Env:ADVISED -ErrorAction SilentlyContinue
  Pop-Location
}

Push-Location $root
try {
  Step 5 'Running the parity tests'
  & $cargo test --release -p vorax -p vorax-scan --test parity -q 2>&1 | Tee-Object -Variable testOut | Out-Host
  if ($LASTEXITCODE) {
    Say 'PARITY FAILED: the Rust port no longer plays like upstream. See the test output above; port the change in crates/vorax/src.'
    exit 1
  }
  Step 6 'Rebuilding vorax.exe and the overlay'
  & $cargo build --release -p vorax-cli -p vorax-overlay -q
  if ($LASTEXITCODE) { throw 'cargo build failed' }
  $info = & (Join-Path $root 'target\release\vorax.exe') info
  Say "OK: parity passed, binary rebuilt. $info"
} finally {
  Pop-Location
}
