#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Sample script for syntax highlighting.
#>

param(
    [string]$Name = "world",
    [int]$Count = 3
)

# A function with a loop and string escapes.
function Get-Greeting {
    param([string]$Who)
    return "Hello, $Who!`n"
}

function Add-Numbers {
    param([int]$A, [int]$B)
    $A + $B
}

$items = @(1, 2, 3.5, 0x1F)
$table = @{ first = 'one'; second = 'two' }

foreach ($item in $items) {
    if ($item -gt 2) {
        Write-Host "big: $item"
    }
    elseif ($item -eq 1) {
        Write-Host 'one'
    }
    else {
        Write-Host "small"
    }
}

for ($i = 0; $i -lt $Count; $i++) {
    Get-Greeting -Who $Name
}

$i = 0
while ($i -lt 3) {
    $i++
    if ($i -eq 2) { continue }
}

try {
    $result = Add-Numbers -A 1 -B 2
    [System.Console]::WriteLine($result)
    throw "oops"
}
catch {
    Write-Error $_.Exception.Message
}
finally {
    Write-Host "done"
}

switch ($Name) {
    "world" { Write-Host "default" }
    default { Write-Host "other" }
}

Get-ChildItem -Path . | Where-Object { $_.Length -gt 100 } | Select-Object Name
[string[]]$names = "a", "b"
$names -join ", "
