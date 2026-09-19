# Deterministically build the redistributable TTC used by the collection
# reference fixture. Both inputs are already committed with their licenses.
param()

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')

function Read-U16BE([byte[]]$data, [int]$offset) {
    return ([int]$data[$offset] -shl 8) -bor [int]$data[$offset + 1]
}

function Read-U32BE([byte[]]$data, [int]$offset) {
    return ([uint32]$data[$offset] -shl 24) -bor
        ([uint32]$data[$offset + 1] -shl 16) -bor
        ([uint32]$data[$offset + 2] -shl 8) -bor
        [uint32]$data[$offset + 3]
}

function Write-U16BE([byte[]]$data, [int]$offset, [int]$value) {
    $data[$offset] = [byte](($value -shr 8) -band 0xff)
    $data[$offset + 1] = [byte]($value -band 0xff)
}

function Write-U32BE([byte[]]$data, [int]$offset, [uint32]$value) {
    $data[$offset] = [byte](($value -shr 24) -band 0xff)
    $data[$offset + 1] = [byte](($value -shr 16) -band 0xff)
    $data[$offset + 2] = [byte](($value -shr 8) -band 0xff)
    $data[$offset + 3] = [byte]($value -band 0xff)
}

function Find-Table([byte[]]$font, [string]$wanted) {
    $count = Read-U16BE $font 4
    for ($i = 0; $i -lt $count; $i++) {
        $record = 12 + 16 * $i
        $tag = [Text.Encoding]::ASCII.GetString($font, $record, 4)
        if ($tag -eq $wanted) {
            return [int](Read-U32BE $font ($record + 8))
        }
    }
    throw "missing $wanted table"
}

function Replace-Family([byte[]]$font) {
    # Same UTF-16BE length as "DejaVu Sans", so table offsets stay stable.
    $from = [Text.Encoding]::BigEndianUnicode.GetBytes('DejaVu Sans')
    $to = [Text.Encoding]::BigEndianUnicode.GetBytes('Subrass One')
    for ($i = 0; $i -le $font.Length - $from.Length; $i++) {
        $match = $true
        for ($j = 0; $j -lt $from.Length; $j++) {
            if ($font[$i + $j] -ne $from[$j]) { $match = $false; break }
        }
        if ($match) {
            [Array]::Copy($to, 0, $font, $i, $to.Length)
            $i += $from.Length - 1
        }
    }
}

function Make-DejaVuFace([bool]$boldItalic) {
    [byte[]]$font = [IO.File]::ReadAllBytes((Join-Path $root 'fonts/DejaVuSans.ttf'))
    Replace-Family $font
    if ($boldItalic) {
        $os2 = Find-Table $font 'OS/2'
        Write-U16BE $font ($os2 + 4) 700
        $selection = Read-U16BE $font ($os2 + 62)
        Write-U16BE $font ($os2 + 62) ($selection -bor 0x21)
        $head = Find-Table $font 'head'
        $macStyle = Read-U16BE $font ($head + 44)
        Write-U16BE $font ($head + 44) ($macStyle -bor 0x03)
    }
    return $font
}

[byte[][]]$faces = @(
    (Make-DejaVuFace $false),
    (Make-DejaVuFace $true),
    [IO.File]::ReadAllBytes((Join-Path $root 'fonts/NotoSansDevanagari.ttf'))
)

$headerSize = 12 + 4 * $faces.Count
$bytes = [Collections.Generic.List[byte]]::new()
for ($i = 0; $i -lt $headerSize; $i++) { $bytes.Add(0) }
$offsets = [Collections.Generic.List[uint32]]::new()

foreach ($source in $faces) {
    while (($bytes.Count % 4) -ne 0) { $bytes.Add(0) }
    $base = $bytes.Count
    [byte[]]$face = $source.Clone()
    $tableCount = Read-U16BE $face 4
    for ($i = 0; $i -lt $tableCount; $i++) {
        $record = 12 + 16 * $i
        $relative = Read-U32BE $face ($record + 8)
        Write-U32BE $face ($record + 8) ([uint32]($base + $relative))
    }
    $offsets.Add([uint32]$base)
    $bytes.AddRange($face)
}

[byte[]]$output = $bytes.ToArray()
[Array]::Copy([Text.Encoding]::ASCII.GetBytes('ttcf'), 0, $output, 0, 4)
Write-U32BE $output 4 0x00010000
Write-U32BE $output 8 ([uint32]$faces.Count)
for ($i = 0; $i -lt $offsets.Count; $i++) {
    Write-U32BE $output (12 + 4 * $i) $offsets[$i]
}

$destination = Join-Path $root 'fonts/SubrassTestCollection.ttc'
[IO.File]::WriteAllBytes($destination, $output)
Write-Host "wrote $destination ($($output.Length) bytes, $($faces.Count) faces)"
