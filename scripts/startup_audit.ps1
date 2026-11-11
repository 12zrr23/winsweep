# startup_audit.ps1 —— 开机启动项体检（只读，不修改任何东西）
# 用法： powershell -NoProfile -ExecutionPolicy Bypass -File .\winsweep\scripts\startup_audit.ps1
# 输出：目标不存在 / 路径未加引号（含空格）的启动项

$ErrorActionPreference = "SilentlyContinue"
$sys32 = Join-Path $env:SystemRoot "System32"

function Resolve-Exe([string]$cmd) {
    if ([string]::IsNullOrWhiteSpace($cmd)) { return "" }
    $c = $cmd.Trim()
    $exe = ""
    if ($c.StartsWith('"')) {
        $i = $c.IndexOf('"', 1)
        if ($i -gt 0) { $exe = $c.Substring(1, $i - 1) }
    }
    if (-not $exe) {
        # 未加引号：优先按 .exe 边界切（兼容 C:\Program Files\...
        $m = [regex]::Match($c, '^(.*?\.exe)(\s|$)', 'IgnoreCase')
        if ($m.Success) { $exe = $m.Groups[1].Value } else { $exe = ($c -split '\s+')[0] }
    }
    if (-not $exe) { return "" }
    $exe = [Environment]::ExpandEnvironmentVariables($exe)
    if ($exe -notmatch '^[A-Za-z]:\\' -and -not $exe.StartsWith('\\')) {
        $cand = Join-Path $sys32 $exe
        if (-not (Test-Path -LiteralPath $cand)) {
            $alt = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\$exe"
            if (Test-Path -LiteralPath $alt) { $cand = $alt }
        }
        if (-not (Test-Path -LiteralPath $cand)) {
            $cmdInfo = Get-Command $exe -ErrorAction SilentlyContinue
            if ($cmdInfo -and $cmdInfo.Source) { $cand = $cmdInfo.Source }
        }
        $exe = $cand
    }
    return $exe
}

function Get-QuotedState([string]$cmd) {
    $c = ([string]$cmd).Trim()
    if ([string]::IsNullOrWhiteSpace($c)) { return "ok" }
    if ($c.StartsWith('"')) { return "quoted" }
    # 只当“可执行文件路径本身”含空格且未加引号才算问题（svchost.exe -k xxx 属正常）
    $m = [regex]::Match($c, '^(.*?\.exe)(\s|$)', 'IgnoreCase')
    $exe = if ($m.Success) { $m.Groups[1].Value } else { ($c -split '\s+')[0] }
    if ($exe -match '\s') { return "unquoted-space" }
    return "ok"
}

$rows = New-Object System.Collections.ArrayList

# 1) HKCU / HKLM Run、RunOnce
$keys = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce",
    "HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Run",
    "HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\RunOnce"
)
foreach ($k in $keys) {
    if (-not (Test-Path $k)) { continue }
    foreach ($p in (Get-ItemProperty $k).PSObject.Properties) {
        if ($p.Name -like "PS*") { continue }
        $raw = [string]$p.Value
        $exe = Resolve-Exe $raw
        [void]$rows.Add([pscustomobject]@{
            Kind   = "Run"
            Name   = $p.Name
            Exists = (Test-Path -LiteralPath $exe)
            Quote  = (Get-QuotedState $raw)
            Target = $exe
        })
    }
}

# 2) 启动文件夹
foreach ($d in @("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup", "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Startup")) {
    if (-not (Test-Path $d)) { continue }
    foreach ($f in (Get-ChildItem $d -File)) {
        $target = $f.FullName
        $exist = $true
        if ($f.Extension -eq ".lnk") {
            $sh = New-Object -ComObject WScript.Shell
            $target = $sh.CreateShortcut($f.FullName).TargetPath
            $exist = Test-Path -LiteralPath $target
        }
        [void]$rows.Add([pscustomobject]@{ Kind = "Startup"; Name = $f.Name; Exists = $exist; Quote = "ok"; Target = $target })
    }
}

# 3) 自动启动服务（仅本机可执行文件，且排除系统目录内的相对名误报）
foreach ($s in (Get-CimInstance Win32_Service | Where-Object { $_.StartMode -eq "Auto" })) {
    $exe = Resolve-Exe ([string]$s.PathName)
    [void]$rows.Add([pscustomobject]@{ Kind = "Service"; Name = $s.Name; Exists = (Test-Path -LiteralPath $exe); Quote = (Get-QuotedState ([string]$s.PathName)); Target = $exe })
}

# 4) 计划任务（仅非系统命名空间）
foreach ($t in (Get-ScheduledTask)) {
    if ($t.TaskPath -like "\Microsoft\Windows\*") { continue }
    foreach ($a in $t.Actions) {
        $exe = Resolve-Exe ([string]$a.Execute)
        [void]$rows.Add([pscustomobject]@{ Kind = "Task"; Name = ($t.TaskPath + $t.TaskName); Exists = (Test-Path -LiteralPath $exe); Quote = "ok"; Target = $exe })
    }
}

Write-Output "===== 有问题的启动项（目标不存在 / 路径未加引号）====="
$bad = $rows | Where-Object { (-not $_.Exists) -or ($_.Quote -eq "unquoted-space") }
if (-not $bad) {
    Write-Output "OK：未发现问题。"
} else {
    $bad | Format-Table -AutoSize -Wrap | Out-String | Write-Output
}

Write-Output "===== 统计 ====="
$rows | Group-Object Kind | ForEach-Object { "{0,-10} {1}" -f $_.Name, $_.Count }
Write-Output ("总计: " + $rows.Count)
