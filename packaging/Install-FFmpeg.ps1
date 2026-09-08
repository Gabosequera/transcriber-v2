param([string]$ArchivePath)
$ErrorActionPreference = 'Stop'
# Optional third-party dependency downloaded from its distributor, under GPLv3.
$url = 'https://github.com/GyanD/codexffmpeg/releases/download/8.0.1/ffmpeg-8.0.1-essentials_build.zip'
$expected = @{
    'ffmpeg.exe' = '5af82a0d4fe2b9eae211b967332ea97edfc51c6b328ca35b827e73eac560dc0d'
    'ffprobe.exe' = '192a1d6899059765ac8c39764fc3148d4e6049955956dc2029f81f4bd6a8972d'
}
$destination = Join-Path $PSScriptRoot 'third-party/ffmpeg'
if (Test-Path -LiteralPath $destination) {
    foreach ($name in $expected.Keys) {
        $file = Join-Path $destination $name
        if (-not (Test-Path -LiteralPath $file) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $expected[$name]) {
            throw "Ya existe $destination con contenido distinto. No se sobrescribe. Elige otra carpeta o configura TRANSCRIPTOR_FFMPEG_DIR."
        }
    }
    Write-Output 'FFmpeg 8.0.1 ya instalado y hashes correctos.'
    exit 0
}
$temp = Join-Path ([IO.Path]::GetTempPath()) ('Transcriptor-ffmpeg-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    if (-not $ArchivePath) {
        $ArchivePath = Join-Path $temp 'upstream.zip'
        Write-Output "Descargando FFmpeg GPLv3 desde $url"
        Invoke-WebRequest -Uri $url -OutFile $ArchivePath -UseBasicParsing
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $ArchivePath).Path)
    try {
        $entries = @{
            'ffmpeg.exe' = 'ffmpeg-8.0.1-essentials_build/bin/ffmpeg.exe'
            'ffprobe.exe' = 'ffmpeg-8.0.1-essentials_build/bin/ffprobe.exe'
            'LICENSE.txt' = 'ffmpeg-8.0.1-essentials_build/LICENSE'
            'README-gyan.txt' = 'ffmpeg-8.0.1-essentials_build/README.txt'
        }
        foreach ($name in $entries.Keys) {
            $entry = $zip.GetEntry($entries[$name])
            if (-not $entry) { throw "Falta $($entries[$name]) en el paquete oficial" }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $temp $name), $false)
        }
    } finally { $zip.Dispose() }
    foreach ($name in $expected.Keys) {
        if ((Get-FileHash -LiteralPath (Join-Path $temp $name) -Algorithm SHA256).Hash -ne $expected[$name]) {
            throw "Hash inesperado: $name. No se instala ni ejecuta."
        }
    }
    New-Item -ItemType Directory -Path $destination | Out-Null
    foreach ($name in $entries.Keys) { Move-Item -LiteralPath (Join-Path $temp $name) -Destination (Join-Path $destination $name) }
    Write-Output 'FFmpeg 8.0.1 instalado. Licencia GPLv3 y procedencia conservadas. Abre Transcriptor.exe.'
} finally {
    $resolved = [IO.Path]::GetFullPath($temp)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf).StartsWith('Transcriptor-ffmpeg-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
