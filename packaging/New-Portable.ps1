param(
    [string]$Binary = "target/release/agent-deck.exe",
    [string]$OutputDirectory = "artifacts",
    [string]$TeamArchive,
    [switch]$Native
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = (Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
    $release = "AgentDeck-$version-windows-x64"
    if ($Native) {
        if ($TeamArchive) { throw 'Use -TeamArchive with the WebView edition.' }
        $release += '-native'
        if (-not $PSBoundParameters.ContainsKey('Binary')) { $Binary = 'target/release/agent-deck-native.exe' }
    }
    $expanded = $null
    if ($TeamArchive) {
        $release += '-team'
        $archive = Get-Item -LiteralPath $TeamArchive
        $expected = 'a66b6be7c13c6fa0dc3660a267b9e34eaa91a4eb7f07fbd55a0468018bd0f721'
        if ((Get-FileHash -LiteralPath $archive.FullName -Algorithm SHA256).Hash -ne $expected) {
            throw 'Use the pinned official psmux v3.4.0 Windows x64 release ZIP.'
        }
        $expanded = Join-Path ([IO.Path]::GetTempPath()) ("agent-deck-team-package-" + [Guid]::NewGuid())
        Expand-Archive -LiteralPath $archive.FullName -DestinationPath $expanded
        $expectedExe = 'bbe825a30f7cddc91b043084049143a381aba5f7e9e36e4d0bb587e296d262ca'
        if ((Get-FileHash -LiteralPath (Join-Path $expanded 'psmux.exe') -Algorithm SHA256).Hash -ne $expectedExe) {
            throw 'Unexpected psmux executable hash.'
        }
    }
    $output = New-Item -ItemType Directory -Path $OutputDirectory -Force
    $stage = New-Item -ItemType Directory -Path (Join-Path $output.FullName $release) -Force
    $binaryFile = Get-Item -LiteralPath $Binary
    $exe = Join-Path $stage.FullName "AgentDeck.exe"
    Copy-Item -LiteralPath $binaryFile.FullName -Destination $exe -Force
    $team = Join-Path $stage.FullName 'team'
    if ($expanded) {
        $bridge = Join-Path $binaryFile.DirectoryName 'deck-team-mux.exe'
        if (-not (Test-Path -LiteralPath $bridge -PathType Leaf)) {
            throw 'Build the deck-team-mux binary in Release mode before packaging Team support.'
        }
        New-Item -ItemType Directory -Path $team -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $expanded 'psmux.exe') -Destination $team -Force
        Copy-Item -LiteralPath (Join-Path $expanded 'LICENSE') -Destination (Join-Path $team 'LICENSE.psmux.txt') -Force
        Copy-Item -LiteralPath $bridge -Destination $team -Force
    }
    # The GNU/LLVM target links the SDK loader dynamically. MSVC normally links it statically.
    $loader = Join-Path $binaryFile.DirectoryName "WebView2Loader.dll"
    if (-not $Native -and (Test-Path -LiteralPath $loader -PathType Leaf)) {
        Copy-Item -LiteralPath $loader -Destination $stage.FullName -Force
    } else {
        $staleLoader = Join-Path $stage.FullName "WebView2Loader.dll"
        if (Test-Path -LiteralPath $staleLoader -PathType Leaf) { Remove-Item -LiteralPath $staleLoader }
    }
    Copy-Item -LiteralPath docs/enterprise-deployment.md -Destination (Join-Path $stage.FullName "DEPLOYMENT.md") -Force
    Copy-Item -LiteralPath docs/user-guide.md -Destination (Join-Path $stage.FullName "USER-GUIDE.md") -Force
    Copy-Item -LiteralPath docs/compatibility.md -Destination (Join-Path $stage.FullName "COMPATIBILITY.md") -Force
    Copy-Item -LiteralPath docs/gjc-session-status.md -Destination (Join-Path $stage.FullName "gjc-session-status.md") -Force
    Copy-Item -LiteralPath docs/gjc-presets.md -Destination (Join-Path $stage.FullName "GJC-PRESETS.md") -Force
    Copy-Item -LiteralPath docs/dependencies.md -Destination (Join-Path $stage.FullName "DEPENDENCIES.md") -Force
    if ($Native) { Copy-Item -LiteralPath docs/native.md -Destination (Join-Path $stage.FullName "NATIVE-GUIDE.md") -Force }
    Set-Content -LiteralPath (Join-Path $stage.FullName "VERSION.txt") -Value "Agent Deck $version (Windows x64)" -Encoding ascii
    $contents = Get-ChildItem -LiteralPath $stage.FullName -File -Recurse | Where-Object Name -ne 'SHA256SUMS.txt'
    $contents | ForEach-Object {
        $relative = $_.FullName.Substring($stage.FullName.Length + 1).Replace('\', '/')
        "{0}  {1}" -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
    } | Set-Content -LiteralPath (Join-Path $stage.FullName "SHA256SUMS.txt") -Encoding ascii
    $zip = Join-Path $output.FullName "$release-portable.zip"
    Compress-Archive -LiteralPath $stage.FullName -DestinationPath $zip -Force
    "{0}  {1}" -f (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path $zip -Leaf) |
        Set-Content -LiteralPath "$zip.sha256" -Encoding ascii
    Get-Item -LiteralPath $zip, "$zip.sha256"
} finally {
    if ($expanded -and (Test-Path -LiteralPath $expanded -PathType Container)) {
        $resolved = (Resolve-Path -LiteralPath $expanded).Path
        $temp = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path.TrimEnd('\')
        if ((Split-Path -Parent $resolved) -eq $temp -and (Split-Path -Leaf $resolved) -like 'agent-deck-team-package-*') {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
    Pop-Location
}

