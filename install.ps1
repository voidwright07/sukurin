# Installs or upgrades Sukurin on Windows.
#   powershell -c "irm https://raw.githubusercontent.com/voidwright07/sukurin/main/install.ps1 | iex"
# SUKURIN_VERSION=v0.2.0 pins a release; SUKURIN_DOWNLOAD_BASE points at a mirror of the releases URL.

# The script block keeps preferences and variables out of the caller's session when piped to iex.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    function Say([string]$Message) { Write-Host "sukurin: $Message" }

    $tmp = $null
    try {
        if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
            throw 'this installer is for Windows. On macOS or Linux run: curl -fsSL https://raw.githubusercontent.com/voidwright07/sukurin/main/install.sh | sh'
        }
        $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
        # Windows 11 on ARM runs the x86_64 build under emulation.
        if ($arch -ne 'AMD64' -and $arch -ne 'ARM64') {
            throw "Windows on $arch is not supported; releases are built for x86_64 only."
        }

        $base = if ($env:SUKURIN_DOWNLOAD_BASE) { $env:SUKURIN_DOWNLOAD_BASE } else { 'https://github.com/voidwright07/sukurin/releases' }
        $download = if ($env:SUKURIN_VERSION) { "$base/download/$($env:SUKURIN_VERSION)" } else { "$base/latest/download" }
        $asset = 'sukurin-windows-x86_64.zip'
        $url = "$download/$asset"

        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\Sukurin'
        $exe = Join-Path $installDir 'sukurin.exe'
        $shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Sukurin.lnk'

        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("sukurin-" + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tmp | Out-Null
        $zip = Join-Path $tmp $asset

        Say "downloading $url"
        try {
            Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        } catch {
            throw "download failed: $url ($($_.Exception.Message))"
        }
        Expand-Archive -Path $zip -DestinationPath (Join-Path $tmp 'x') -Force

        $running = Get-Process -Name sukurin -ErrorAction SilentlyContinue
        if ($running) {
            Say 'stopping the running Sukurin'
            $running | Stop-Process -Force
            $running | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
        }

        Say "installing to $exe"
        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
        # A just-stopped exe can stay locked for a moment, but Windows lets a locked file be renamed.
        $old = "$exe.old"
        Remove-Item -Path $old -Force -ErrorAction SilentlyContinue
        if (Test-Path $exe) { Move-Item -Path $exe -Destination $old -Force }
        Copy-Item -Path (Join-Path $tmp 'x\sukurin.exe') -Destination $exe -Force
        Remove-Item -Path $old -Force -ErrorAction SilentlyContinue

        New-Item -ItemType Directory -Path (Split-Path $shortcut) -Force | Out-Null
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        $link.TargetPath = $exe
        $link.WorkingDirectory = $installDir
        $link.Description = 'Sukurin'
        $link.Save()

        Start-Process -FilePath $exe
        Say 'Sukurin is running in the system tray (check the ^ overflow if you do not see it).'
    } catch {
        Write-Host "sukurin: error: $($_.Exception.Message)" -ForegroundColor Red
    } finally {
        if ($tmp -and (Test-Path $tmp)) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
    }
}
