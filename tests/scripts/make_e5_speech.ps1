[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
$root=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$destination=[System.IO.Path]::GetFullPath($OutputDirectory)
if (-not $destination.StartsWith($root+[System.IO.Path]::DirectorySeparatorChar,[System.StringComparison]::OrdinalIgnoreCase)) { throw 'Speech fixtures must remain inside V2.' }
[System.IO.Directory]::CreateDirectory($destination) > $null
Add-Type -AssemblyName System.Speech
$speaker=New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    $speaker.Rate=-1
    $format=New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000,[System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,[System.Speech.AudioFormat.AudioChannel]::Mono)
    $texts=@{
        'speech-a.wav'='This is a synthetic recording for a local software test. The blue notebook is on the table. We will meet tomorrow at nine.'
        'speech-b.wav'='A second speaker reads a different invented sentence. The green bicycle is parked near the garden. No private recording is used.'
    }
    foreach ($name in $texts.Keys) {
        $path=Join-Path $destination $name
        if (Test-Path -LiteralPath $path) { throw 'Use a new fixture directory; originals are never overwritten.' }
        $speaker.SetOutputToWaveFile($path,$format)
        $speaker.Speak($texts[$name])
        $speaker.SetOutputToNull()
        Get-FileHash -LiteralPath $path -Algorithm SHA256
    }
} finally { $speaker.Dispose() }
