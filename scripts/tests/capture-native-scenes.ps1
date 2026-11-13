param(
    [string]$Fixture = 'build/ui-qa/reference/fixture.json',
    [string]$Output = 'build/ui-qa/native-scenes',
    [string]$Executable = 'native-ui/target/release/devshell-native-ui.exe'
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$taskFixture = Get-Content -LiteralPath (Join-Path $taskRoot $Fixture) -Encoding utf8 | ConvertFrom-Json
if ($taskFixture.base -notmatch '^http://127\.0\.0\.1:\d+$') { throw 'The fixture must use an isolated loopback backend.' }
$taskOutput = [IO.Path]::GetFullPath((Join-Path $taskRoot $Output))
if (-not $taskOutput.StartsWith($taskRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Capture output must stay inside the workspace.' }
New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
$taskCaptureBinary = Join-Path $taskOutput 'DevShellUI.capture.exe'
Copy-Item -LiteralPath (Join-Path $taskRoot $Executable) -Destination $taskCaptureBinary -Force
$env:DEVSHELL_VISUAL_TEST_DIR = $taskOutput
$env:DEVSHELL_VISUAL_TEST_FIXTURE = Join-Path $taskRoot $Fixture
try {
    $taskCaptureProcess = Start-Process -FilePath $taskCaptureBinary -ArgumentList @('--base', $taskFixture.base, '--token', $taskFixture.token) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $taskOutput 'stdout.log') -RedirectStandardError (Join-Path $taskOutput 'stderr.log') -PassThru
    if (-not $taskCaptureProcess.WaitForExit(60000)) {
        Stop-Process -Id $taskCaptureProcess.Id
        throw 'Native scene capture did not finish within 60 seconds.'
    }
    if ($taskCaptureProcess.ExitCode -ne 0) { throw "Native scene capture failed; inspect $taskOutput\stderr.log." }
    $taskScenes = @(Get-ChildItem -LiteralPath $taskOutput -Filter '*.png')
    if ($taskScenes.Count -ne 72) { throw "Expected seventy-two native scene captures; found $($taskScenes.Count)." }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'native-ui-comparison.html') -Destination (Join-Path (Split-Path $taskOutput -Parent) 'comparison.html') -Force
    Write-Output "Captured seventy-two GPUI scenes in $taskOutput"
} finally {
    Remove-Item Env:DEVSHELL_VISUAL_TEST_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:DEVSHELL_VISUAL_TEST_FIXTURE -ErrorAction SilentlyContinue
}
