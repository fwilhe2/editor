[CmdletBinding()]
param(
    [switch]$Build,
    [string]$VisualStudioPath = "$env:ProgramFiles\Microsoft Visual Studio\2022\BuildTools"
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Assert-Windows {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'This script must run on Windows.'
    }
}

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this script from an elevated PowerShell prompt.'
    }
}

function Invoke-Download {
    param(
        [Parameter(Mandatory)] [string]$Uri,
        [Parameter(Mandatory)] [string]$Destination
    )

    Write-Host "Downloading $Uri"
    Invoke-WebRequest -Uri $Uri -OutFile $Destination
}

function Install-VisualStudioBuildTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $requiredComponents = @(
        'Microsoft.VisualStudio.Workload.VCTools',
        'Microsoft.VisualStudio.Workload.NetCoreBuildTools',
        'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
        'Microsoft.VisualStudio.Component.Windows11SDK.26100'
    )

    if (Test-Path $vswhere) {
        $installation = & $vswhere -version '[17.0,18.0)' -products '*' -requires $requiredComponents -latest
        if ($LASTEXITCODE -eq 0 -and $installation) {
            Write-Host 'Visual Studio Build Tools and required components are already installed.'
            return
        }
    }

    $tempRoot = Join-Path $env:TEMP 'editor-windows-dependencies'
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    $bootstrapper = Join-Path $tempRoot 'vs_BuildTools.exe'
    if (-not (Test-Path $bootstrapper)) {
        Invoke-Download -Uri 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -Destination $bootstrapper
    }

    $arguments = @(
        '--quiet',
        '--wait',
        '--norestart',
        '--nocache',
        '--installPath', $VisualStudioPath,
        '--add', 'Microsoft.VisualStudio.Workload.VCTools',
        '--add', 'Microsoft.VisualStudio.Workload.NetCoreBuildTools',
        '--add', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
        '--add', 'Microsoft.VisualStudio.Component.Windows11SDK.26100'
    )

    Write-Host 'Installing Visual Studio Build Tools and Windows SDK.'
    $process = Start-Process -FilePath $bootstrapper -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) {
        throw "Visual Studio Build Tools installer failed with exit code $($process.ExitCode)."
    }
}

function Install-DotNetSdk {
    $dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
    if ($dotnet) {
        $hasSdk = & dotnet --list-sdks | Where-Object { $_ -match '^8\.' }
        if ($hasSdk) {
            Write-Host '.NET 8 SDK is already installed.'
            return
        }
    }

    $tempRoot = Join-Path $env:TEMP 'editor-windows-dependencies'
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
    $installer = Join-Path $tempRoot 'dotnet-install.ps1'
    if (-not (Test-Path $installer)) {
        Invoke-Download -Uri 'https://dot.net/v1/dotnet-install.ps1' -Destination $installer
    }

    Write-Host 'Installing the .NET 8 SDK.'
    & $installer -Channel 8.0 -Quality ga -InstallDir "$env:ProgramFiles\dotnet"
    if ($LASTEXITCODE -ne 0) {
        throw ".NET SDK installer failed with exit code $LASTEXITCODE."
    }
    $env:PATH = "$env:ProgramFiles\dotnet;$env:PATH"
}

function Install-Rust {
    $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    if (-not $rustup) {
        $tempRoot = Join-Path $env:TEMP 'editor-windows-dependencies'
        New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
        $installer = Join-Path $tempRoot 'rustup-init.exe'
        if (-not (Test-Path $installer)) {
            Invoke-Download -Uri 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -Destination $installer
        }

        Write-Host 'Installing Rust with the stable MSVC toolchain.'
        $process = Start-Process -FilePath $installer -ArgumentList '-y', '--default-toolchain', 'stable-msvc', '--profile', 'default' -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            throw "Rust installer failed with exit code $($process.ExitCode)."
        }
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    }

    & rustup toolchain install stable-msvc --profile default
    if ($LASTEXITCODE -ne 0) {
        throw "Rust toolchain installation failed with exit code $LASTEXITCODE."
    }
    & rustup default stable-msvc
    if ($LASTEXITCODE -ne 0) {
        throw "Could not select the stable MSVC Rust toolchain (exit code $LASTEXITCODE)."
    }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

function Install-UniFfiGenerator {
    $generator = Join-Path $env:USERPROFILE '.cargo\bin\uniffi-bindgen-cs.exe'
    if (Test-Path $generator) {
        $version = & $generator --version 2>$null
        if ($version -match '0\.11\.0') {
            Write-Host 'The pinned uniffi-bindgen-cs generator is already installed.'
            return
        }
    }

    Write-Host 'Installing the pinned UniFFI C# generator.'
    & cargo install uniffi-bindgen-cs --git 'https://github.com/NordSecurity/uniffi-bindgen-cs' --tag 'v0.11.0+v0.31.0' --locked --force
    if ($LASTEXITCODE -ne 0) {
        throw "uniffi-bindgen-cs installation failed with exit code $LASTEXITCODE."
    }
}

Assert-Windows
Assert-Administrator
Install-VisualStudioBuildTools
Install-DotNetSdk
Install-Rust
Install-UniFfiGenerator

Write-Host 'Windows build dependencies are installed.'

if ($Build) {
    $repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
    Push-Location $repoRoot
    try {
        cargo build --release -p editor-ffi
        if ($LASTEXITCODE -ne 0) { throw 'Rust FFI build failed.' }

        New-Item -ItemType Directory -Force -Path 'ui_windows/Generated' | Out-Null
        & uniffi-bindgen-cs --library 'target/release/editor_ffi.dll' --out-dir 'ui_windows/Generated'
        if ($LASTEXITCODE -ne 0) { throw 'C# binding generation failed.' }

        dotnet build 'ui_windows/EditorApp.csproj' -c Release -p:Platform=x64
        if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed.' }
    }
    finally {
        Pop-Location
    }
}