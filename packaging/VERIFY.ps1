$ErrorActionPreference = 'Stop'
$count = 0
Get-Content -LiteralPath (Join-Path $PSScriptRoot 'CHECKSUMS.sha256') -Encoding UTF8 | ForEach-Object {
    $parts = $_ -split '  ', 2
    if ($parts.Count -ne 2) { throw 'Entrada de checksum inválida' }
    $path = Join-Path $PSScriptRoot $parts[1]
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    if ($actual -ne $parts[0]) { throw "Hash diferente: $($parts[1])" }
    $count++
}
Write-Output "OK: $count archivos verificados"
