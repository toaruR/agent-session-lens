# A Windows cargo build produces a PE executable; build the helper inside Linux.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Distro,
    [string] $HelperPath = '/usr/local/bin/lens-wsl-helper'
)
$ErrorActionPreference = 'Stop'
if (-not $HelperPath.StartsWith('/') -or $HelperPath -match "[\r\n]") {
    throw 'HelperPath must be an absolute Linux file path.'
}
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$linuxRoot = & wsl.exe -d $Distro --exec wslpath -u $repoRoot
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the repository path inside WSL.' }
$buildScript = @'
set -eu
cargo_bin="$(command -v cargo || true)"
if [ -z "$cargo_bin" ]; then cargo_bin="$HOME/.cargo/bin/cargo"; fi
if [ ! -x "$cargo_bin" ]; then
    printf 'Install Rust in this WSL distribution first.\n' >&2
    exit 1
fi
target_dir="$HOME/.cache/agent-session-lens/build"
"$cargo_bin" build --manifest-path "$1/Cargo.toml" -p lens-wsl-helper --release --target-dir "$target_dir" >&2
printf '%s\n' "$target_dir/release/lens-wsl-helper"
'@
$artifact = & wsl.exe -d $Distro --exec sh -c $buildScript sh $linuxRoot.Trim()
if ($LASTEXITCODE -ne 0) { throw 'WSL helper build failed; nothing was installed.' }
$installScript = @'
set -eu
mkdir -p -- "$(dirname -- "$2")"
install -m 755 -- "$1" "$2"
'@
& wsl.exe -d $Distro -u root --exec sh -c $installScript sh $artifact.Trim() $HelperPath
if ($LASTEXITCODE -ne 0) { throw 'WSL helper installation failed.' }
& wsl.exe -d $Distro --exec $HelperPath --version
if ($LASTEXITCODE -ne 0) { throw 'Installed helper could not run.' }
Write-Host "Installed in ${Distro}: $HelperPath"
Write-Host 'For a custom HelperPath, set the same helperPath in settings.json.'
