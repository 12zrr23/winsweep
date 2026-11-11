# winsweep

> Windows 深度清理、系统优化、磁盘分析与安全文件迁移 —— 纯 Rust 单文件程序 + 本地 Web GUI。

```bash
npm install -g winsweep
```

安装时自动从 GitHub Release 下载对应 Windows 二进制（约 4.8 MB），无需编译、无外部运行时。

## 用法

```bash
winsweep status          # 系统状态与健康分
winsweep clean --dry-run # 预览可清理项（不删除）
winsweep clean           # 执行清理
winsweep purge --dry-run # 预览可瘦身的构建产物
winsweep installer --dry-run
winsweep analyze "D:\\Projects"
winsweep optimize
winsweep uninstall -q edge
winsweep migrate "C:\\Users\\me\\Downloads" --to "D:\\Store" --dry-run
winsweep gui             # 启动本地 Web GUI（默认 http://127.0.0.1:41234）
```

所有命令都支持 `--json`，便于脚本集成：

```bash
winsweep --json status
```

## 环境要求

| 项 | 要求 |
| :--- | :--- |
| 系统 | Windows 10 / 11（x64） |
| Node.js | ≥ 18（仅用于启动器，运行本身不依赖 Node） |
| 权限 | 清理系统级缓存、迁移到盘根目录时需要管理员 |

## 安装失败 / 国内网络慢

`postinstall` 从 GitHub 下载，网络受限时可改用下列任一方式：

```bash
# 1) 走镜像（示例：ghproxy）
WINSWEEP_MIRROR=https://ghproxy.net/ npm install -g winsweep

# 2) 手动下载后用本地文件安装
#    https://github.com/12zrr23/winsweep/releases
WINSWEEP_BINARY=D:\downloads\winsweep.exe npm install -g winsweep

# 3) 先跳过下载，稍后补
WINSWEEP_SKIP_DOWNLOAD=1 npm install -g winsweep
npm rebuild winsweep
```

## 安全说明

- 删除前先扫描，默认 `--dry-run` 只预览；
- 系统关键目录（`System32`、`WinSxS`、Windows Defender 等）在黑名单内，拒绝操作；
- 所有删除写入审计日志 `%LOCALAPPDATA%\winsweep\operations.log`；
- 文件迁移采用「复制 → 校验 → 目录联接」，校验不通过不动源数据。

完整文档与源码：**https://github.com/12zrr23/winsweep**

## License

MIT
