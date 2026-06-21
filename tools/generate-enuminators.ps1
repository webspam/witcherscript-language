$ErrorActionPreference = 'Stop'

$unknownEnumsPath = Join-Path $PSScriptRoot '..\builtins\unknown-enums.ws'
$templatePath = Join-Path $PSScriptRoot 'enuminator.ws'
$outDir = $PSScriptRoot

$template = Get-Content -Path $templatePath -Raw
$unknownEnums = Get-Content -Path $unknownEnumsPath -Raw

foreach ($match in [regex]::Matches($unknownEnums, 'enum\s+(\w+)\s+\{\}')) {
    $enumName = $match.Groups[1].Value
    $content = $template
    $content = $content -replace 'ToEnumMember', "ToEnumMember_$enumName"
    $content = $content -replace 'EnuminateBitFlags', "EnuminateBitFlags_$enumName"
    $content = $content -replace 'EnuminateEnum', "EnuminateEnum_$enumName"
    $content = $content -replace 'EInputKey', $enumName

    $outPath = Join-Path $outDir "enuminate$enumName.ws"
    [System.IO.File]::WriteAllText($outPath, $content)
    Write-Host "Wrote $outPath"
}
