[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$SourceDirectory = 'target/release',
    [string]$OutputDirectory = 'dist',
    [string]$IconFile = 'crates/mt-app/resources/icon.ico',
    [string]$InstallerDefinition = 'scripts/windows-installer.nsi'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-SemanticVersionInfo {
    param([Parameter(Mandatory = $true)][string]$Value)

    $normalized = $Value.Trim()
    if ($normalized.StartsWith('v', [StringComparison]::OrdinalIgnoreCase)) {
        $normalized = $normalized.Substring(1)
    }

    $pattern = '^(?<major>0|[1-9][0-9]*)\.(?<minor>0|[1-9][0-9]*)\.(?<patch>0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$'
    $match = [regex]::Match($normalized, $pattern)
    if (-not $match.Success) {
        throw "version must be SemVer and filename-safe: $Value"
    }

    $parts = @()
    foreach ($name in @('major', 'minor', 'patch')) {
        [int]$part = 0
        if (-not [int]::TryParse($match.Groups[$name].Value, [ref]$part) -or $part -gt 65535) {
            throw "version component $name must fit an unsigned 16-bit resource field: $Value"
        }
        $parts += $part
    }

    [pscustomobject]@{
        Semantic = $normalized
        Numeric = '{0}.{1}.{2}.0' -f $parts[0], $parts[1], $parts[2]
    }
}

function Resolve-ExistingPath {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label,
        [ValidateSet('Leaf', 'Container')][string]$PathType
    )

    if (-not (Test-Path -LiteralPath $Path -PathType $PathType)) {
        throw "$Label does not exist: $Path"
    }
    (Resolve-Path -LiteralPath $Path).Path
}

function Resolve-MakeNsis {
    foreach ($name in @('makensis.exe', 'makensis')) {
        $command = Get-Command $name -ErrorAction SilentlyContinue
        if ($null -ne $command) {
            return $command.Source
        }
    }

    foreach ($path in @(
        'C:\Program Files (x86)\NSIS\makensis.exe',
        'C:\Program Files\NSIS\makensis.exe',
        'C:\ProgramData\chocolatey\bin\makensis.exe'
    )) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            return $path
        }
    }

    $null
}

$versionInfo = Get-SemanticVersionInfo -Value $Version
$source = Resolve-ExistingPath -Path $SourceDirectory -Label 'staged payload directory' -PathType Container
$icon = Resolve-ExistingPath -Path $IconFile -Label 'installer icon' -PathType Leaf
$definition = Resolve-ExistingPath -Path $InstallerDefinition -Label 'NSIS definition' -PathType Leaf

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$outputRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$output = Join-Path $outputRoot "Mini-Term_$($versionInfo.Semantic)_x64-setup.exe"
Remove-Item -LiteralPath $output -Force -ErrorAction SilentlyContinue

# Level 1: runner image or an existing installation.
$makeNsis = Resolve-MakeNsis

# Level 2: Chocolatey. Re-resolve because its shim location differs from NSIS's
# traditional Program Files location.
if (-not $makeNsis) {
    Write-Host '::group::choco install nsis'
    $choco = Get-Command choco.exe -ErrorAction SilentlyContinue
    if ($null -ne $choco) {
        try {
            & $choco.Source install nsis -y --no-progress
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "choco install nsis exited with $LASTEXITCODE"
            }
        } catch {
            Write-Warning "choco install nsis failed: $_"
        }
    } else {
        Write-Warning 'Chocolatey is not available on this runner'
    }
    Write-Host '::endgroup::'
    $makeNsis = Resolve-MakeNsis
}

# Level 3: official portable NSIS archive. It carries the stubs and plugins
# beside makensis, so the extracted directory is directly runnable.
if (-not $makeNsis) {
    Write-Host '::group::download portable NSIS'
    $tempRoot = if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
        [IO.Path]::GetTempPath()
    } else {
        $env:RUNNER_TEMP
    }
    $archive = Join-Path $tempRoot 'mini-term-nsis.zip'
    $directory = Join-Path $tempRoot 'mini-term-nsis'
    foreach ($url in @(
        'https://downloads.sourceforge.net/project/nsis/NSIS%203/3.10/nsis-3.10.zip',
        'https://cfhcable.dl.sourceforge.net/project/nsis/NSIS%203/3.10/nsis-3.10.zip'
    )) {
        try {
            Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $directory -Recurse -Force -ErrorAction SilentlyContinue
            Invoke-WebRequest -Uri $url -OutFile $archive -MaximumRetryCount 3 -RetryIntervalSec 10
            Expand-Archive -LiteralPath $archive -DestinationPath $directory -Force
            $candidate = Get-ChildItem -LiteralPath $directory -Recurse -Filter makensis.exe -File |
                Select-Object -First 1
            if ($null -ne $candidate) {
                $makeNsis = $candidate.FullName
                break
            }
        } catch {
            Write-Warning "$url unavailable: $_"
        }
    }
    Write-Host '::endgroup::'
}

if (-not $makeNsis) {
    throw 'all makensis discovery levels failed (runner image, Chocolatey, portable NSIS)'
}

Write-Host "makensis: $makeNsis"
Write-Host "installer version: $($versionInfo.Semantic) (resource $($versionInfo.Numeric))"
& $makeNsis "/DVERSION=$($versionInfo.Semantic)" "/DVERSION_NUM=$($versionInfo.Numeric)" `
    "/DSOURCE_DIR=$source" "/DICON_FILE=$icon" "/DOUT_FILE=$output" $definition
if ($LASTEXITCODE -ne 0) {
    throw "makensis exited with $LASTEXITCODE"
}

$artifact = Get-Item -LiteralPath $output
if ($artifact.Length -le 0) {
    throw "makensis produced an empty installer: $output"
}

Write-Host "built $($artifact.FullName) ($($artifact.Length) bytes)"
