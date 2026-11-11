#!/usr/bin/env node
/**
 * winsweep CLI 启动器：把参数原样透传给真正的 winsweep.exe。
 * 这样 `npm i -g winsweep` 之后即可 `winsweep status` / `winsweep gui`。
 */
"use strict";

const path = require("path");
const fs = require("fs");
const { spawn } = require("child_process");

const exe = path.join(__dirname, "..", "bin", "winsweep.exe");

if (!fs.existsSync(exe)) {
  console.error(
    [
      "",
      "[winsweep] 找不到已下载的程序：",
      "  " + exe,
      "",
      "通常是安装时跳过了下载。请任选其一修复：",
      "  npm rebuild winsweep",
      "  node node_modules/winsweep/install.js",
      "  WINSWEEP_BINARY=D:\\path\\winsweep.exe npm rebuild winsweep",
      "",
      "或直接从 Release 下载：",
      "  https://github.com/12zrr23/winsweep/releases",
      "",
    ].join("\n")
  );
  process.exit(1);
}

const child = spawn(exe, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: false,
});

for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => {
    try { child.kill(sig); } catch {}
  });
}

child.on("error", (err) => {
  console.error(`[winsweep] 启动失败：${err.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code === null ? 1 : code);
  }
});
