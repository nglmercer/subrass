# Generate libass reference frames for tests/reference/ (plan #41).
#
# Requires an ffmpeg build with the libass `ass` filter:
#   $env:FFMPEG = path to ffmpeg.exe (defaults to `ffmpeg` on PATH)
#
# For every tests/golden/<name>.ass, renders one 256x144 raw-RGBA frame
# at the manifest time over a black background, using ONLY the repo's
# fonts/ directory (fontsdir), and writes tests/reference/<name>.rgba
# plus tests/reference/provenance.json.
#
# Never runs during normal `cargo test`; see tests/reference.rs.
param()

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
Set-Location $root

$ffmpeg = if ($env:FFMPEG) { $env:FFMPEG } else { 'ffmpeg' }
& $ffmpeg -hide_banner -h filter=ass 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'ffmpeg with libass ass filter not found' }

$manifest = Get-Content 'tests/golden/manifest.json' -Raw | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path 'tests/reference' | Out-Null

$version = & $ffmpeg -version 2>&1 | Select-Object -First 1
$buildconf = & $ffmpeg -hide_banner -buildconf 2>&1 | Out-String

foreach ($prop in $manifest.fixtures.PSObject.Properties) {
    $name = $prop.Name
    $timeMs = $prop.Value.time_ms
    $sec = [double]$timeMs / 1000.0
    $ass = "tests/golden/$name.ass"
    $out = "tests/reference/$name.rgba"
    Write-Host "rendering $name @ ${timeMs}ms"
    & $ffmpeg -hide_banner -loglevel error -y `
        -f lavfi -i 'color=c=black:s=256x144:r=10:d=6' `
        -vf "format=rgba,ass='$ass':fontsdir='fonts'" `
        -ss $sec -frames:v 1 -f rawvideo -pix_fmt rgba $out
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $name" }
}

$sha = [System.Security.Cryptography.SHA256]::Create()
$fontHash = [System.BitConverter]::ToString(
    $sha.ComputeHash([System.IO.File]::ReadAllBytes('fonts/DejaVuSans.ttf'))
).Replace('-', '').ToLower()
$prov = [ordered]@{
    generator      = 'tests/reference/gen_references.ps1'
    ffmpeg_version = "$version"
    ffmpeg_buildconf_sha256 = [System.BitConverter]::ToString(
        [System.Security.Cryptography.SHA256]::Create().ComputeHash(
            [System.Text.Encoding]::UTF8.GetBytes($buildconf))).Replace('-','').ToLower()
    font_file      = 'fonts/DejaVuSans.ttf'
    font_sha256    = $fontHash
    video          = @(256, 144)
    background     = 'black (ass composited over opaque black)'
    generated_utc  = (Get-Date).ToUniversalTime().ToString('o')
    note           = 'effect-banner, effect-scroll and font-fallback are known-divergent (libass ignores legacy effects; fontconfig fallback is environment-dependent)'
}
$prov | ConvertTo-Json | Set-Content 'tests/reference/provenance.json'
Write-Host 'wrote tests/reference/provenance.json'
