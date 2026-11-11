$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$previousGoos = $env:GOOS
$previousGoarch = $env:GOARCH
$previousCgo = $env:CGO_ENABLED
try {
    $env:GOOS = 'windows'
    $env:GOARCH = 'amd64'
    $env:CGO_ENABLED = '0'
    New-Item -ItemType Directory -Force 'build/windows' | Out-Null
    & cargo build --manifest-path 'native-ui/Cargo.toml' --release
    if ($LASTEXITCODE -ne 0) { throw 'DevShell native UI build failed.' }
    Copy-Item -Force 'native-ui/target/release/devshell-native-ui.exe' 'build/windows/DevShellUI.exe'
    & go build -buildvcs=false -trimpath -tags 'desktop,production' -ldflags '-s -w -H windowsgui' -o 'build/windows/DevShell.exe' .
    if ($LASTEXITCODE -ne 0) { throw 'DevShell Windows build failed.' }
    Write-Host 'Built: build/windows/DevShell.exe'
} finally {
    $env:GOOS = $previousGoos
    $env:GOARCH = $previousGoarch
    $env:CGO_ENABLED = $previousCgo
}
