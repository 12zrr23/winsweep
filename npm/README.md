# WinSweep

> **Windows 深度清理、系统优化、磁盘分析与安全迁移工具**
>
> 纯 Rust 实现 · 单文件可执行（约 4.8 MB）· 自带本地 Web GUI · 零外部运行时

WinSweep 是一个面向 Windows 10/11 的系统维护工具，灵感来自 macOS 上的 ，
但**从界面到实现全部原创重写**，并针对 Windows 的实际痛点（C 盘爆满、残留启动项、开发者缓存、软件卸载残留）做了强化。


## 2. 功能一览

| # | 模块 | 说明 | CLI |
| :- | :--- | :--- | :--- |
| 1 | 状态监控 | CPU / 内存 / 磁盘 / 网络 / Top 进程 + **健康评分** | `winsweep status` |
| 2 | 深度清理 | 6 类缓存：用户 Temp、系统 Temp、Windows 更新缓存、Edge / Chrome 缓存、缩略图 | `winsweep clean` |
| 3 | 构建产物瘦身 | `node_modules` / `target` / `.venv` / `__pycache__` / `dist`·`build` / `bin`·`obj` | `winsweep purge` |
| 4 | 安装包清理 | 扫描 Downloads / Desktop 中 >5 MB 的 `.exe/.msi/.msix/.iso/.zip` 安装镜像 | `winsweep installer` |
| 5 | 磁盘大文件分析 | 小顶堆算法，找出目录内 Top N 大文件 | `winsweep analyze <path>` |
| 6 | 系统优化 | 刷新 DNS 缓存、ARP 缓存、资源管理器图标缓存 | `winsweep optimize` |
| 7 | 软件卸载分析 | 读注册表列出已安装应用（按体积排序、可搜索），**可直接启动卸载程序** | `winsweep uninstall -q <kw>` |
| 8 | 文件迁移 | 大文件夹搬到别的盘：复制 → 校验 → 目录联接，可撤销 | `winsweep migrate <src> --to D:\Store` |
| 9 | 图形界面 | 本地 Web GUI，8 个页面，5 套主题可切换 | `winsweep gui` |

### 2.1 深度清理的分类

| 分类 id | 名称 | 典型路径 | 需要管理员 |
| :--- | :--- | :--- | :--- |
| `user_temp` | 用户临时文件 | `%LOCALAPPDATA%\Temp` | 否 |
| `sys_temp` | 系统临时文件 | `%WINDIR%\Temp` | 是 |
| `windows_update` | Windows 更新缓存 | `%WINDIR%\SoftwareDistribution\Download` | 是 |
| `edge_cache` | Edge 缓存 | `...\Microsoft\Edge\User Data\Default\Cache` | 否 |
| `chrome_cache` | Chrome 缓存 | `...\Google\Chrome\User Data\Default\Cache` | 否 |
| `thumbnails` | 缩略图数据库 | `...\Microsoft\Windows\Explorer\thumbcache_*.db` | 否 |

> 扫描只返回「分类 + 数量 + 体积」的摘要（约 1 KB），执行时才按勾选真正删除；避免一次传输上万条路径导致浏览器卡死。

### 2.2 构建产物的识别规则（防误杀）

| 目录名 | 认定为产物必须有 | 类型名 |
| :--- | :--- | :--- |
| `node_modules` | 父目录存在 `package.json` | Node.js Modules |
| `target` | 父目录存在 `Cargo.toml` / `pom.xml` / `build.sbt` | Build Target |
| `.venv` / `venv` | 目录内存在 `pyvenv.cfg` | Python Virtualenv |
| `__pycache__` | —（字节码缓存本身安全） | Python Bytecode Cache |
| `dist` / `build` | 父目录存在 `package.json` / `pyproject.toml` / `setup.py` / `CMakeLists.txt` / `Makefile` | Build Output |
| `bin` / `obj` | 父目录有 `*.csproj/*.fsproj/*.vbproj/*.sln`，或子目录含 `Debug`/`Release`/`net*` | .NET Build Output |

> 这条规则来自一次真实事故：早期版本把所有名为 `bin` 的目录当成 .NET 产物，差点删掉 `ffmpeg\bin`、`Go\bin`、`Git\bin`。现在必须**父目录指纹**匹配才认。

### 2.3 安装包清理的判定

- 扩展名 `msi / iso / msix` → 直接认定；
- 扩展名 `exe / zip` → 文件名需含 `setup` / `install` / `update` / `patch` / `init`，或以 `-x64.exe`、`_win64.exe`、`-windows-x86_64.exe` 结尾；
- 体积必须 > 5 MB；扫描深度 2 层；默认扫 `Downloads` + `Desktop`。

---


---

## 4. 快速开始

### 4.1 通过 npm 安装（普通用户推荐）

#### 前置要求

| 项 | 要求 | 检查方法 |
| :--- | :--- | :--- |
| 操作系统 | Windows 10 / 11（x64） | — |
| Node.js | ≥ 18（仅安装阶段用，**运行本身不依赖 Node**） | `node --version` |

> 没有 Node.js？去 https://nodejs.org 下载 LTS 版安装（一路下一步即可），或在 PowerShell 里执行 `winget install OpenJS.NodeJS.LTS`。

#### 安装

打开 **PowerShell** 或 **CMD**（Win 键 → 输入 `powershell` → 回车），执行：

```bash
npm install -g winsweep
```

安装过程（约 10–30 秒，取决于网速）：

1. npm 下载安装器包（仅 4.4 kB）；
2. `postinstall` 脚本自动从 GitHub Release 下载 **winsweep-win32-x64.exe**（约 4.8 MB）；
3. 自动校验 **SHA256**（与 Release 里的校验文件比对，不符则报错重来）；
4. 完成，`winsweep` 命令全局可用。

看到这样的输出就是成功了：

```text
added 1 package in 8s
[winsweep] 正在下载二进制：https://github.com/12zrr23/winsweep/releases/download/v0.1.2/winsweep-win32-x64.exe
[winsweep] sha256 校验通过
[winsweep] 安装完成（4.79 MB）→ C:\Users\你\AppData\Roaming\npm\node_modules\winsweep\bin\winsweep.exe
[winsweep] 运行：winsweep --help    或    winsweep gui
```

#### 验证安装

```bash
winsweep --version      # 应输出：winsweep 0.1.2
winsweep status         # 打印系统健康分和硬件状态
```

#### 第一次使用（30 秒上手）

```bash
# 1) 看看能清多少（只预览，不删除任何东西）
winsweep clean --dry-run

# 2) 打开图形界面（自动弹出浏览器）
winsweep gui
```

`winsweep gui` 会启动本地服务并自动打开 `http://127.0.0.1:41234`。
界面上每个危险操作都有二次确认；关闭 PowerShell 窗口（或 Ctrl+C）即退出服务。

#### 升级与卸载

```bash
npm update -g winsweep      # 升级到最新版（重新下载新二进制）
npm uninstall -g winsweep   # 卸载（会移除命令与下载的 exe）
```

> 卸载不会删除你的配置和日志（`%LOCALAPPDATA%\winsweep\`），需要的话手动删。

#### 常见安装问题（国内网络 / 杀软告警）

**① 下载慢或超时** —— GitHub 直连不稳时，换镜像源：

```bash
WINSWEEP_MIRROR=https://ghproxy.net/ npm install -g winsweep
```

**② 完全离线 / 内网环境** —— 先手动下载 exe，再指向它安装：

```bash
# 浏览器打开 https://github.com/12zrr23/winsweep/releases 下载 winsweep-win32-x64.exe
WINSWEEP_BINARY=D:\downloads\winsweep-win32-x64.exe npm install -g winsweep
```

**③ 杀毒软件 / SmartScreen 告警** —— exe 未做代码签名，属正常告警：

- Windows Defender：告警页 →「更多信息」→「仍要运行」；
- 想彻底验证：比对 SHA256（安装日志里有，Release 页也公示）；
- 不放心就改用「从源码构建」（见 4.3），编译产物自己生成。

**④ `npm install` 报 ENEEDAUTH / 权限错误** —— 不要用管理员终端装全局包（反而会坏权限）。如果已经装坏了：

```bash
npm uninstall -g winsweep
npm cache clean --force
npm install -g winsweep
```

**⑤ `winsweep` 命令找不到** —— 重开一个 PowerShell 窗口（PATH 刷新），或运行 `npm config get prefix` 确认该目录在 PATH 里。

#### npm 包的工作原理（可选阅读）

```
npm i -g winsweep
  └─ postinstall: node install.js
       ├─ 已有 exe 且校验通过？ → 跳过（幂等）
       ├─ WINSWEEP_BINARY 指定本地文件？ → 直接复制
       └─ 下载 https://github.com/12zrr23/winsweep/releases/download/v<版本>/winsweep-win32-x64.exe
            └─ sha256 校验（Release 里的 .sha256 文件）→ 失败即删除重报错
  └─ bin/winsweep.js：把命令行参数原样透传给 exe，Ctrl+C 信号也会转发
```

环境变量一览：

| 变量 | 作用 | 示例 |
| :--- | :--- | :--- |
| `WINSWEEP_MIRROR` | 下载地址前缀（镜像） | `https://ghproxy.net/` |
| `WINSWEEP_BINARY` | 本地 exe 路径（离线安装） | `D:\dl\winsweep.exe` |
| `WINSWEEP_SKIP_DOWNLOAD` | 跳过下载（先装壳后补） | `1` |
| `WINSWEEP_SHA256` | 手动指定校验值 | `abc123...` |
| `WINSWEEP_SKIP_VERIFY` | 跳过校验（不推荐） | `1` |

### 4.2 直接运行（已编译 exe）

```powershell
# 启动 GUI（会自动打开浏览器）
.\winsweep\target\release\winsweep.exe gui

# 或双击
.\winsweep\run-gui.bat
```

### 4.3 从源码构建

```powershell
cd winsweep
cargo build --release
# 产物：target\release\winsweep.exe（单文件，约 4.8 MB）
```

- 工具链：`x86_64-pc-windows-gnu` + w64devkit（本项目使用），或 MSVC 工具链均可；
- 无外部运行时依赖，拷到别的机器可直接跑。

### 4.4 三条最常用命令

```powershell
winsweep status                  # 先看机器状态与健康分
winsweep clean --dry-run         # 看看能清多少（不会删）
winsweep purge --dry-run         # 看看构建产物能省多少（不会删）
```

---


---

## 安全机制（摘要）

- 删除前先扫描，默认 --dry-run 只预览；
- 系统关键目录（System32、WinSxS、Windows Defender 等）在黑名单内，拒绝操作；
- 三级删除策略（回收站优先），最终以「文件真消失」为成功判定；
- 所有删除写审计日志 %LOCALAPPDATA%\winsweep\operations.log；
- 文件迁移采用「复制 → 校验 → 目录联接」，校验不通过不动源数据。

---

## 常见问题

## 13. 已知限制与排障

| 现象 | 原因 | 处理 |
| :--- | :--- | :--- |
| 自动化环境里删除报「拒绝访问」 | 在受限沙箱（低完整性令牌）的进程树中启动时，Windows 会拒绝写盘 | 双击启动 / 普通终端 / 计划任务 / WMI 创建进程即可，正常使用不受影响 |
| 预检提示「目标目录不可写」 | 目标盘根目录通常只允许管理员写 | 选择普通文件夹（如 `D:\Store`）或以管理员运行 |
| 迁移后目标盘是 FAT/exFAT | 目录联接只支持 NTFS | 自动回退符号链接（需开发者模式或管理员） |
| 某些文件没清掉，显示「跳过」 | 文件被进程占用 | 关闭相关程序后重试；不会强删 |
| 开机弹「找不到文件」 | 多半是残留启动项目标不存在，或路径含空格未加引号 | 跑 `scripts/startup_audit.ps1`；案例见 `docs/2026-11-11-boot-issue-report.md` |
| 开机事件里有驱动 `blocked from loading` | 系统驱动阻止列表 / WDAC 策略拦截，**不是文件被删** | `sfc /scannow` + `DISM /Online /Cleanup-Image /RestoreHealth`；不要关闭阻止列表 |
| GUI 打不开 | 端口被占用 | 会自动从 41234 递增到 41243，看控制台输出的实际地址 |

---


---

完整文档（含 GUI 截图、HTTP API、主题系统、项目结构）：**https://github.com/12zrr23/winsweep**

## License

MIT