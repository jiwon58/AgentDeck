param(
    [string]$Binary = "target/release/agent-deck.exe",
    [string]$OutputDirectory = "artifacts"
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = (Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
    $release = "AgentDeck-$version-windows-x64"
    $output = New-Item -ItemType Directory -Path $OutputDirectory -Force
    $stage = New-Item -ItemType Directory -Path (Join-Path $output.FullName $release) -Force
    $binaryFile = Get-Item -LiteralPath $Binary
    $exe = Join-Path $stage.FullName "AgentDeck.exe"
    Copy-Item -LiteralPath $binaryFile.FullName -Destination $exe -Force
    # The GNU/LLVM target links the SDK loader dynamically. MSVC normally links it statically.
    $loader = Join-Path $binaryFile.DirectoryName "WebView2Loader.dll"
    if (Test-Path -LiteralPath $loader -PathType Leaf) {
        Copy-Item -LiteralPath $loader -Destination $stage.FullName -Force
    } else {
        $staleLoader = Join-Path $stage.FullName "WebView2Loader.dll"
        if (Test-Path -LiteralPath $staleLoader -PathType Leaf) { Remove-Item -LiteralPath $staleLoader }
    }
    Copy-Item -LiteralPath docs/enterprise-deployment.md -Destination (Join-Path $stage.FullName "DEPLOYMENT.md") -Force
    Copy-Item -LiteralPath docs/compatibility.md -Destination (Join-Path $stage.FullName "COMPATIBILITY.md") -Force
    Copy-Item -LiteralPath docs/gjc-session-status.md -Destination (Join-Path $stage.FullName "gjc-session-status.md") -Force
    Copy-Item -LiteralPath docs/dependencies.md -Destination (Join-Path $stage.FullName "DEPENDENCIES.md") -Force
    Set-Content -LiteralPath (Join-Path $stage.FullName "VERSION.txt") -Value "Agent Deck $version (Windows x64)" -Encoding ascii
    $contents = Get-ChildItem -LiteralPath $stage.FullName -File | Where-Object Name -ne 'SHA256SUMS.txt'
    $contents | ForEach-Object {
        "{0}  {1}" -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name
    } | Set-Content -LiteralPath (Join-Path $stage.FullName "SHA256SUMS.txt") -Encoding ascii
    $zip = Join-Path $output.FullName "$release-portable.zip"
    Compress-Archive -LiteralPath $stage.FullName -DestinationPath $zip -Force
    "{0}  {1}" -f (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path $zip -Leaf) |
        Set-Content -LiteralPath "$zip.sha256" -Encoding ascii
    Get-Item -LiteralPath $zip, "$zip.sha256"
} finally {
    Pop-Location
}

