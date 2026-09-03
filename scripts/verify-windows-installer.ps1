[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [string]$AppVersion,
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$StageDirectory = 'target/release',
    [string]$OutputPath = 'dist/windows-package-validation.json'
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
        Parts = [int[]]@($parts[0], $parts[1], $parts[2], 0)
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

function Get-PeMachine {
    param([Parameter(Mandatory = $true)][string]$Path)

    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -lt 0x40 -or $reader.ReadUInt16() -ne 0x5a4d) {
            throw "not a PE file (missing MZ header): $Path"
        }
        $stream.Seek(0x3c, [IO.SeekOrigin]::Begin) | Out-Null
        $peOffset = $reader.ReadUInt32()
        if ($peOffset + 6 -gt $stream.Length) {
            throw "not a PE file (invalid PE offset): $Path"
        }
        $stream.Seek($peOffset, [IO.SeekOrigin]::Begin) | Out-Null
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "not a PE file (missing PE signature): $Path"
        }
        [int]$reader.ReadUInt16()
    } finally {
        $reader.Dispose()
    }
}

function Format-PeMachine {
    param([Parameter(Mandatory = $true)][int]$Machine)
    '0x{0:x4}' -f $Machine
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Initialize-ResourceInspector {
    if ('MiniTerm.PackageResourceInspector' -as [type]) {
        return
    }

    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;

namespace MiniTerm
{
    public static class PackageResourceInspector
    {
        private const uint LoadLibraryAsDataFile = 0x00000002;
        private const uint LoadLibraryAsImageResource = 0x00000020;

        private delegate bool EnumResourceTypeCallback(IntPtr module, IntPtr type, IntPtr parameter);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryEx(string fileName, IntPtr file, uint flags);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool EnumResourceTypes(
            IntPtr module,
            EnumResourceTypeCallback callback,
            IntPtr parameter);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool FreeLibrary(IntPtr module);

        public static string[] GetResourceTypes(string path)
        {
            IntPtr module = LoadLibraryEx(
                path,
                IntPtr.Zero,
                LoadLibraryAsDataFile | LoadLibraryAsImageResource);
            if (module == IntPtr.Zero)
            {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "LoadLibraryEx failed for " + path);
            }

            try
            {
                var types = new List<string>();
                EnumResourceTypeCallback callback = delegate(IntPtr _, IntPtr type, IntPtr __)
                {
                    ulong value = unchecked((ulong)type.ToInt64());
                    if ((value >> 16) == 0)
                    {
                        types.Add((value & 0xffff).ToString());
                    }
                    else
                    {
                        types.Add(Marshal.PtrToStringUni(type) ?? string.Empty);
                    }
                    return true;
                };

                if (!EnumResourceTypes(module, callback, IntPtr.Zero))
                {
                    throw new Win32Exception(
                        Marshal.GetLastWin32Error(),
                        "EnumResourceTypes failed for " + path);
                }
                GC.KeepAlive(callback);
                return types.ToArray();
            }
            finally
            {
                FreeLibrary(module);
            }
        }
    }
}
'@
}

function Get-VersionResourceEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][pscustomobject]$ExpectedVersion,
        [Parameter(Mandatory = $true)][string]$ExpectedDescription
    )

    $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
    $productVersion = ([string]$version.ProductVersion).Trim()
    if ($productVersion -ne $ExpectedVersion.Semantic) {
        throw "ProductVersion mismatch for $Path`: expected $($ExpectedVersion.Semantic), got $productVersion"
    }
    if (([string]$version.ProductName).Trim() -ne 'Mini-Term') {
        throw "ProductName mismatch for $Path"
    }
    if (([string]$version.FileDescription).Trim() -ne $ExpectedDescription) {
        throw "FileDescription mismatch for $Path`: expected $ExpectedDescription"
    }

    $fileParts = [int[]]@(
        $version.FileMajorPart,
        $version.FileMinorPart,
        $version.FileBuildPart,
        $version.FilePrivatePart
    )
    $productParts = [int[]]@(
        $version.ProductMajorPart,
        $version.ProductMinorPart,
        $version.ProductBuildPart,
        $version.ProductPrivatePart
    )
    $expectedParts = [int[]]$ExpectedVersion.Parts
    if (($fileParts -join '.') -ne ($expectedParts -join '.')) {
        throw "numeric FileVersion mismatch for $Path`: expected $($ExpectedVersion.Numeric), got $($fileParts -join '.')"
    }
    if (($productParts -join '.') -ne ($expectedParts -join '.')) {
        throw "numeric ProductVersion mismatch for $Path`: expected $($ExpectedVersion.Numeric), got $($productParts -join '.')"
    }

    $resourceTypes = @([MiniTerm.PackageResourceInspector]::GetResourceTypes($Path) | Sort-Object -Unique)
    foreach ($required in @('3', '14', '16', '24')) {
        if ($required -notin $resourceTypes) {
            throw "required PE resource type $required is missing from $Path"
        }
    }

    [ordered]@{
        product_name = ([string]$version.ProductName).Trim()
        product_version = $productVersion
        file_version = ([string]$version.FileVersion).Trim()
        file_description = ([string]$version.FileDescription).Trim()
        numeric_file_version = $fileParts -join '.'
        numeric_product_version = $productParts -join '.'
        resource_types = $resourceTypes
    }
}

function Resolve-SevenZip {
    foreach ($name in @('7z.exe', '7z', '7zz.exe', '7zz')) {
        $command = Get-Command $name -ErrorAction SilentlyContinue
        if ($null -ne $command) {
            return $command.Source
        }
    }

    $knownPaths = @('C:\ProgramData\chocolatey\bin\7z.exe')
    if ($env:ProgramFiles) {
        $knownPaths += Join-Path $env:ProgramFiles '7-Zip\7z.exe'
    }
    $programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    if ($programFilesX86) {
        $knownPaths += Join-Path $programFilesX86 '7-Zip\7z.exe'
    }

    foreach ($path in $knownPaths) {
        if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) {
            return $path
        }
    }

    $null
}

function Resolve-ExtractedPayload {
    param(
        [Parameter(Mandatory = $true)][string]$ExtractionRoot,
        [Parameter(Mandatory = $true)][string]$RelativePath,
        [Parameter(Mandatory = $true)][object[]]$ExtractedFiles
    )

    $direct = Join-Path $ExtractionRoot $RelativePath
    if (Test-Path -LiteralPath $direct -PathType Leaf) {
        return (Resolve-Path -LiteralPath $direct).Path
    }

    $normalized = $RelativePath.Replace('\', '/').TrimStart('/')
    $suffix = "/$normalized"
    $matches = @($ExtractedFiles | Where-Object {
        $relative = [IO.Path]::GetRelativePath($ExtractionRoot, $_.FullName).Replace('\', '/')
        $relative.Equals($normalized, [StringComparison]::OrdinalIgnoreCase) -or
            $relative.EndsWith($suffix, [StringComparison]::OrdinalIgnoreCase)
    })
    if ($matches.Count -ne 1) {
        throw "expected one extracted payload ending in $normalized, found $($matches.Count)"
    }
    $matches[0].FullName
}

$packageVersion = Get-SemanticVersionInfo -Value $Version
$applicationVersion = Get-SemanticVersionInfo -Value $AppVersion
$installer = Resolve-ExistingPath -Path $InstallerPath -Label 'installer' -PathType Leaf
$stage = Resolve-ExistingPath -Path $StageDirectory -Label 'staged payload directory' -PathType Container

$outputDirectory = Split-Path -Parent $OutputPath
if (-not $outputDirectory) {
    $outputDirectory = '.'
}
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$validationOutput = [IO.Path]::GetFullPath($OutputPath)
Remove-Item -LiteralPath $validationOutput -Force -ErrorAction SilentlyContinue

Initialize-ResourceInspector

$sevenZip = Resolve-SevenZip
if (-not $sevenZip) {
    Write-Host '::group::choco install 7zip'
    $choco = Get-Command choco.exe -ErrorAction SilentlyContinue
    if ($null -ne $choco) {
        try {
            & $choco.Source install 7zip -y --no-progress
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "choco install 7zip exited with $LASTEXITCODE"
            }
        } catch {
            Write-Warning "choco install 7zip failed: $_"
        }
    }
    Write-Host '::endgroup::'
    $sevenZip = Resolve-SevenZip
}
if (-not $sevenZip) {
    throw '7-Zip is required to extract and inspect the NSIS installer'
}

$tempRoot = if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    [IO.Path]::GetTempPath()
} else {
    $env:RUNNER_TEMP
}
$extractionRoot = Join-Path $tempRoot ("mini-term-installer-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $extractionRoot | Out-Null

try {
    & $sevenZip x $installer "-o$extractionRoot" -y
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip extraction exited with $LASTEXITCODE"
    }

    $extractedFiles = @(Get-ChildItem -LiteralPath $extractionRoot -Recurse -File)
    $payloadSpecs = @(
        [pscustomobject]@{ RelativePath = 'mini-term.exe'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'miniterm-hook.exe'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'mt-ssh-cli.exe'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'mt-ssh-mcp.exe'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'mt-terminal-host.exe'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'portable-conpty/conpty.dll'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'portable-conpty/x64/OpenConsole.exe'; Machine = 0x8664 },
        [pscustomobject]@{ RelativePath = 'portable-conpty/arm64/OpenConsole.exe'; Machine = 0xaa64 }
    )

    $payloadEvidence = @()
    foreach ($spec in $payloadSpecs) {
        $stagedPath = Resolve-ExistingPath `
            -Path (Join-Path $stage $spec.RelativePath) `
            -Label "staged payload $($spec.RelativePath)" `
            -PathType Leaf
        $extractedPath = Resolve-ExtractedPayload `
            -ExtractionRoot $extractionRoot `
            -RelativePath $spec.RelativePath `
            -ExtractedFiles $extractedFiles

        $stagedMachine = Get-PeMachine -Path $stagedPath
        $extractedMachine = Get-PeMachine -Path $extractedPath
        if ($stagedMachine -ne $spec.Machine -or $extractedMachine -ne $spec.Machine) {
            throw "PE machine mismatch for $($spec.RelativePath)"
        }

        $stagedHash = Get-Sha256 -Path $stagedPath
        $extractedHash = Get-Sha256 -Path $extractedPath
        if ($stagedHash -ne $extractedHash) {
            throw "installer payload hash mismatch for $($spec.RelativePath)"
        }

        $stagedItem = Get-Item -LiteralPath $stagedPath
        $extractedItem = Get-Item -LiteralPath $extractedPath
        if ($stagedItem.Length -ne $extractedItem.Length) {
            throw "installer payload size mismatch for $($spec.RelativePath)"
        }

        $payloadEvidence += [ordered]@{
            path = $spec.RelativePath.Replace('\', '/')
            staged_size = $stagedItem.Length
            extracted_size = $extractedItem.Length
            staged_sha256 = $stagedHash
            extracted_sha256 = $extractedHash
            hash_match = $true
            expected_machine = (Format-PeMachine -Machine $spec.Machine)
            staged_machine = (Format-PeMachine -Machine $stagedMachine)
            extracted_machine = (Format-PeMachine -Machine $extractedMachine)
        }
    }

    $miniTermPath = Join-Path $stage 'mini-term.exe'
    $miniTermResources = Get-VersionResourceEvidence `
        -Path $miniTermPath `
        -ExpectedVersion $applicationVersion `
        -ExpectedDescription 'Mini-Term'

    $installerMachine = Get-PeMachine -Path $installer
    if ($installerMachine -ne 0x014c) {
        throw "NSIS installer PE machine mismatch: expected 0x014c, got $(Format-PeMachine -Machine $installerMachine)"
    }
    $installerResources = Get-VersionResourceEvidence `
        -Path $installer `
        -ExpectedVersion $packageVersion `
        -ExpectedDescription 'Mini-Term Installer'

    $markers = @(
        'MINI_TERM_LEGACY_SHELL',
        'MINI_TERM_TERMINAL_HOST',
        'MINI_TERM_REMOTE_RUNTIME',
        'MINI_TERM_REMOTE_AGENT_STATUS',
        'MINI_TERM_ORCA_WORKTREE_CONTEXT',
        'MINI_TERM_GITHUB_PROJECT_TASKS',
        'MINI_TERM_GLOBAL_AGENT_ACTIVITY'
    )
    $miniTermText = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($miniTermPath))
    $markerEvidence = @()
    foreach ($marker in $markers) {
        $present = $miniTermText.Contains($marker, [StringComparison]::Ordinal)
        if (-not $present) {
            throw "stable feature marker is missing from mini-term.exe: $marker"
        }
        $markerEvidence += [ordered]@{ name = $marker; present = $present }
    }

    $installerItem = Get-Item -LiteralPath $installer
    $validation = [ordered]@{
        schema_version = 1
        status = 'passed'
        generated_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        repository = $env:GITHUB_REPOSITORY
        commit = $env:GITHUB_SHA
        run_id = $env:GITHUB_RUN_ID
        run_number = $env:GITHUB_RUN_NUMBER
        target = 'x86_64-pc-windows-msvc'
        package_version = $packageVersion.Semantic
        app_version = $applicationVersion.Semantic
        installer = [ordered]@{
            file = $installerItem.Name
            size = $installerItem.Length
            sha256 = (Get-Sha256 -Path $installer)
            machine = (Format-PeMachine -Machine $installerMachine)
            resources = $installerResources
        }
        mini_term_resources = $miniTermResources
        feature_markers = $markerEvidence
        payloads = $payloadEvidence
        extraction = [ordered]@{
            tool = $sevenZip
            discovered_files = $extractedFiles.Count
        }
    }

    $validation | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $validationOutput -Encoding utf8
    Write-Host "validated installer and wrote $validationOutput"
} finally {
    Remove-Item -LiteralPath $extractionRoot -Recurse -Force -ErrorAction SilentlyContinue
}
