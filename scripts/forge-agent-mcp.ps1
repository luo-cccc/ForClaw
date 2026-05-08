$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$releaseBin = Join-Path $root "target\release\forge-agent-mcp.exe"
$debugBin = Join-Path $root "target\debug\forge-agent-mcp.exe"

if (Test-Path $releaseBin) {
    $bin = $releaseBin
} elseif (Test-Path $debugBin) {
    $bin = $debugBin
} else {
    Write-Error "forge-agent-mcp.exe was not found. Build it with: cargo build -p forge-agent-mcp"
    exit 1
}

if (-not $env:FORGE_AGENT_DATA_DIR) {
    $env:FORGE_AGENT_DATA_DIR = Join-Path $root ".forge-agent-data"
}

if ($args.Count -eq 0) {
    & $bin stdio
} else {
    & $bin @args
}
