# 开机“找不到文件”报错 —— 排查与修复报告

- 时间：2026-11-11 19:35 起
- 现象：开机（19:30:23 登录）后弹出错误，提示“某个文件找不到”
- 结论：**不是 WinSweep 删除了系统/启动文件**；是三条**残留启动项**导致，已定位并修复

---

## 一、结论先行

| 项目 | 判定 |
| :--- | :--- |
| WinSweep 是否删过系统/启动项文件 | **没有**。operations.log 共 1766 条，全部落在 `%LOCALAPPDATA%\Temp`、缩略图缓存、`Downloads` 测试安装包、以及工作区 `_e2e_proj\target` 测试目录 |
| 报错直接原因 | 登录时 Windows 执行 `Run` 启动项，目标解析失败 → 弹“找不到文件” |
| 具体三条 | ① `AMDNoiseSuppression`（目标文件不存在）② `TeamsMachineInstaller`（目标文件不存在）③ `Docker Desktop`（路径含空格但**没加引号**，Windows 实际去执行 `C:\Program`） |
| 处理 | 已删除 ①②，给 ③ 补引号；全部启动项复核通过 |

---

## 二、证据

### 1. WinSweep 的删除记录（可逐条核对）

`%LOCALAPPDATA%\winsweep\operations.log`（268 KB / 1766 条）：

| 类别 | 条数 | 说明 |
| :--- | ---: | :--- |
| `CLEAN_FILE` | 1739 | 全部在 `AppData\Local\Temp`（`cco*`、`node-compile-cache`、`.tmp.wav/.ico`）+ `Explorer\thumbcache_*.db` |
| `DELETE_INSTALLER` | 14 | `Downloads\winsweep_e2e_*`、`browser_demo_*`（我们自己造的测试安装包） |
| `PURGE_ARTIFACT` | 11 | `Desktop\owen-work\_e2e_proj\target`（测试工程） |
| `CLEAN_SKIPPED_LOCKED` | 2 | 被占用跳过 |

**没有任何一条**位于 `C:\Windows`、`Program Files`、用户目录根、或注册表启动项目标文件。

### 2. 报错与清理时间无关的旁证

- `luafv` 驱动启动失败（另一条无关错误，见第四节）最早记录为 **2026-09-23 19:50**，昨晚（11-10）之前就长期存在，且 11/5—11/11 每次开机都有记录。
- `C:\Windows\System32\AMD` 目录的时间戳为 **2025-08-15**，说明 `AMD\ANR\AMDNoiseSuppression.exe` 缺失已久，不是昨晚造成的。

### 3. 登录时实际会执行、且会失败的三条

```
HKCU\...\Run  AMDNoiseSuppression = "C:\Windows\system32\AMD\ANR\AMDNoiseSuppression.exe"   → 文件不存在
HKLM\...\Wow6432Node\Run  TeamsMachineInstaller = C:\Program Files\Teams Installer\Teams.exe --checkInstall ...  → 文件不存在，且路径未加引号
HKCU\...\Run  Docker Desktop = C:\Program Files\Docker\Docker\Docker Desktop.exe            → 路径含空格、未加引号
```

第 3 条是最典型的“找不到文件”来源：Windows 按空格切分后去执行 `C:\Program`，该文件不存在，于是弹窗提示找不到文件（多半就是提示 `C:\Program`）。

---

## 三、已执行的修复（可回滚）

| 步骤 | 操作 | 结果 |
| :--- | :--- | :--- |
| 1 | 导出备份 `HKCU\...\Run` 与 `HKLM\...\Wow6432Node\Run` | `backup\2026-11-11-boot-fix\HKCU-Run.reg`、`HKLM-Wow6432-Run.reg` |
| 2 | 删除 `HKCU\...\Run\AMDNoiseSuppression` | 完成（含 `StartupApproved\Run` 残留状态） |
| 3 | 删除 `HKLM\...\Wow6432Node\Run\TeamsMachineInstaller` | 完成 |
| 4 | 为 `HKCU\...\Run\Docker Desktop` 补引号 | `"C:\Program Files\Docker\Docker\Docker Desktop.exe"` |
| 5 | 为服务 `GBTECService` 的 `ImagePath` 补引号（原值同样含空格未加引号，属隐患） | `"C:\Program Files (x86)\GIGABYTE\GBTECService\OLEDDisplayService.exe"` |
| 6 | 全量复核 HKCU/HKLM 的 Run、RunOnce + 启动文件夹 + 自启服务 + 计划任务（共 122 项） | ✅ 全部指向存在的文件、无未加引号的含空格路径 |

### 回滚方法

```powershell
# 恢复原来的两个启动项键（双击也可）
reg import "C:\Users\Administrator\Desktop\owen-work\backup\2026-11-11-boot-fix\HKCU-Run.reg"
reg import "C:\Users\Administrator\Desktop\owen-work\backup\2026-11-11-boot-fix\HKLM-Wow6432-Run.reg"
```

---

## 四、另一项独立问题：luafv 驱动被阻止加载（与本次弹窗无关）

```
sc query luafv  →  STOPPED, WIN32_EXIT_CODE 1275 (This driver has been blocked from loading)
文件：C:\Windows\System32\drivers\luafv.sys 存在，签名有效（Microsoft Windows，10.0.22621.1）
历史：2026-09-23 起每次开机都失败
```

- 原因：系统里有 WDAC 代码完整性策略（`C:\Windows\System32\CodeIntegrity\CiPolicies\Active\*.cip`）或易受攻击驱动阻止列表把 `luafv` 拦下；**不是文件被删**（文件在、签名有效）。
- 影响：仅“UAC 文件虚拟化”不可用（老程序往受保护目录写文件不再被静默重定向）。不影响开机、不影响日常使用。
- 建议：先 `sfc /scannow`，再 `DISM /Online /Cleanup-Image /RestoreHealth`；如仍在，检查是否装了第三方驱动/安全软件下发的阻止策略。**不建议**关闭“易受攻击的驱动程序阻止列表”。

---

## 五、下次如果还弹窗

请把弹窗**原文**（或截图）发我，尤其是引号里的路径。最快的定位方法：

```powershell
# 列出所有会被登录执行的项，并标出目标不存在的
powershell -NoProfile -ExecutionPolicy Bypass -File .\winsweep\scripts\startup_audit.ps1
```

---

## 六、给 WinSweep 的改进建议（待确认后实施）

1. **启动项体检**：新增一页，扫描 Run/RunOnce、启动文件夹、计划任务、自动服务，标出“目标不存在/路径未加引号/已被禁用”，一键修复（补引号、改指向、或删除并备份）。
2. **删除保护**：执行清理前，把“被启动项/服务引用的文件”加入保护名单，永不删除（当前已通过目录范围天然规避）。
3. **清理白名单细化**：`%TEMP%` 下按“最后访问时间 > 7 天”才清理，进一步避免打断正在运行的程序。
