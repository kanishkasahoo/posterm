param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\posterm"
)

$ErrorActionPreference = "Stop"

$ForgejoBaseUrl = "https://git.ksahoo.com"
$Repo = "kanishkasahoo/posterm"
$AssetName = "posterm-windows-x86_64.zip"

if (-not [Environment]::Is64BitOperatingSystem) {
    throw "posterm Windows releases currently support x86_64 Windows only."
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("posterm-install-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    if ($Version -eq "latest") {
        $latest = Invoke-RestMethod "$ForgejoBaseUrl/api/v1/repos/$Repo/releases/latest"
        $releaseTag = $latest.tag_name
    } else {
        $releaseTag = $Version
    }

    $archivePath = Join-Path $tmp $AssetName
    $checksumPath = Join-Path $tmp "$AssetName.sha256"
    $assetUrl = "$ForgejoBaseUrl/$Repo/releases/download/$releaseTag/$AssetName"
    $checksumUrl = "$ForgejoBaseUrl/$Repo/releases/download/$releaseTag/$AssetName.sha256"

    Write-Host "Downloading $Repo $releaseTag..."
    Invoke-WebRequest -Uri $assetUrl -OutFile $archivePath
    Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath

    $expected = ((Get-Content $checksumPath -Raw) -split "\s+")[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Checksum verification failed for $AssetName"
    }

    Expand-Archive -Path $archivePath -DestinationPath $tmp -Force
    $binary = Join-Path $tmp "posterm.exe"
    if (-not (Test-Path $binary -PathType Leaf)) {
        throw "Archive did not contain posterm.exe"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item $binary (Join-Path $InstallDir "posterm.exe") -Force

    Write-Host "Installed posterm to $InstallDir\posterm.exe"
    Write-Host "Add $InstallDir to PATH if it is not already present."
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
