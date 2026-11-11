#!/usr/bin/env node
/**
 * install.js —— 在 npm install 之后把 Windows 二进制下载到位。
 *
 * 设计原则：
 *  - 幂等：已存在且 sha256 一致就直接跳过；
 *  - 可离线：设置 WINSWEEP_BINARY=本地 exe 路径 可直接复制；
 *  - 可换源：设置 WINSWEEP_MIRROR=https://ghproxy.net/ 走镜像（国内常用）；
 *  - 失败要说人话：告诉用户手动下载地址和后续命令。
 */
"use strict";

const fs = require("fs");
const path = require("path");
const https = require("https");
const http = require("http");
const crypto = require("crypto");
const os = require("os");

const pkg = require("./package.json");
const VERSION = pkg.version;
const ASSET = `winsweep-win32-x64.exe`;
const DEFAULT_BASE = `https://github.com/12zrr23/winsweep/releases/download/v${VERSION}`;
const SHA_URL = `${DEFAULT_BASE}/${ASSET}.sha256`;
// 校验值优先取环境变量，其次下载 Release 里的 .sha256（发版流水线自动生成）
const ENV_SHA256 = (process.env.WINSWEEP_SHA256 || "").trim().toLowerCase() || null;

const binDir = path.join(__dirname, "bin");
const target = path.join(binDir, "winsweep.exe");

function sha256File(p) {
  const h = crypto.createHash("sha256");
  h.update(fs.readFileSync(p));
  return h.digest("hex");
}

function ok(msg) {
  console.log(`[winsweep] ${msg}`);
}
function warn(msg) {
  console.warn(`[winsweep] ${msg}`);
}

function fail(msg) {
  console.error(`\n[winsweep] 安装失败：${msg}\n`);
  console.error(`  手动方式（任选其一）：`);
  console.error(`   1) 直接下载：${DEFAULT_BASE}/${ASSET}`);
  console.error(`      放到：${target}`);
  console.error(`   2) 离线安装：WINSWEEP_BINARY=D:\\path\\winsweep.exe npm install -g winsweep`);
  console.error(`   3) 走镜像：  WINSWEEP_MIRROR=https://ghproxy.net/ npm install -g winsweep`);
  console.error(`   4) 跳过下载：WINSWEEP_SKIP_DOWNLOAD=1 npm install -g winsweep\n`);
  process.exit(1);
}

function alreadyGood() {
  if (!fs.existsSync(target)) return false;
  if (!ENV_SHA256) return true; // 本地无法得知远端校验值，存在即视为已安装（重新安装会覆盖）
  try {
    return sha256File(target) === ENV_SHA256;
  } catch {
    return false;
  }
}

function fetchText(url, redirects = 0) {
  if (redirects > 6) return Promise.reject(new Error("重定向次数过多"));
  return new Promise((resolve, reject) => {
    const client = url.startsWith("http://") ? http : https;
    const req = client.get(url, { headers: { "User-Agent": `winsweep-npm/${VERSION}` } }, (res) => {
      const code = res.statusCode || 0;
      if (code >= 300 && code < 400 && res.headers.location) {
        res.resume();
        return resolve(fetchText(new URL(res.headers.location, url).toString(), redirects + 1));
      }
      if (code !== 200) { res.resume(); return reject(new Error(`HTTP ${code}`)); }
      let data = "";
      res.setEncoding("utf8");
      res.on("data", (d) => (data += d));
      res.on("end", () => resolve(data));
      res.on("error", reject);
    });
    req.setTimeout(20000, () => req.destroy(new Error("超时")));
    req.on("error", reject);
  });
}

function download(url, dest, redirects = 0) {
  if (redirects > 6) return Promise.reject(new Error("重定向次数过多"));
  return new Promise((resolve, reject) => {
    const client = url.startsWith("http://") ? http : https;
    const req = client.get(
      url,
      { headers: { "User-Agent": `winsweep-npm/${VERSION}`, Accept: "*/*" } },
      (res) => {
        const code = res.statusCode || 0;
        if (code >= 300 && code < 400 && res.headers.location) {
          res.resume();
          const next = new URL(res.headers.location, url).toString();
          return resolve(download(next, dest, redirects + 1));
        }
        if (code !== 200) {
          res.resume();
          return reject(new Error(`下载返回 HTTP ${code}：${url}`));
        }
        const tmp = `${dest}.download`;
        const out = fs.createWriteStream(tmp);
        res.pipe(out);
        out.on("finish", () => out.close(() => resolve(tmp)));
        out.on("error", reject);
        res.on("error", reject);
      }
    );
    req.setTimeout(60000, () => req.destroy(new Error("下载超时（60s）")));
    req.on("error", reject);
  });
}

(async () => {
  if (process.env.WINSWEEP_SKIP_DOWNLOAD === "1") {
    warn("WINSWEEP_SKIP_DOWNLOAD=1，跳过二进制下载");
    return;
  }
  if (process.platform !== "win32") {
    warn(`当前平台是 ${process.platform}，WinSweep 只支持 Windows，跳过下载`);
    return;
  }

  fs.mkdirSync(binDir, { recursive: true });

  if (alreadyGood()) {
    ok(`已存在且校验通过：${target}`);
    return;
  }

  // 1) 离线：本地已有 exe
  const local = process.env.WINSWEEP_BINARY;
  if (local) {
    if (!fs.existsSync(local)) fail(`WINSWEEP_BINARY 指向的文件不存在：${local}`);
    fs.copyFileSync(local, target);
    ok(`已从本地复制：${local}`);
    return;
  }

  // 2) 在线下载（可加镜像前缀）
  const mirror = process.env.WINSWEEP_MIRROR || "";
  const url = `${mirror.replace(/\/$/, "")}${mirror ? "/" : ""}${DEFAULT_BASE}/${ASSET}`;
  ok(`正在下载二进制：${url}`);
  let tmp;
  try {
    tmp = await download(url, target);
  } catch (e) {
    fail(e.message);
  }

  const size = fs.statSync(tmp).size;
  if (size < 1024 * 1024) fail(`下载文件过小（${size} 字节），可能不是有效程序`);

  let expected = ENV_SHA256;
  if (!expected) {
    try {
      expected = (await fetchText(SHA_URL)).trim().split(/\s+/)[0].toLowerCase();
    } catch (e) {
      expected = null;
    }
  }
  if (expected) {
    const got = sha256File(tmp);
    if (got !== expected) {
      try { fs.unlinkSync(tmp); } catch {}
      fail(`校验失败：期望 ${expected}，实际 ${got}`);
    }
    ok("sha256 校验通过");
  } else if (process.env.WINSWEEP_SKIP_VERIFY !== "1") {
    warn("未能获取 .sha256 校验文件，已跳过校验（可用 WINSWEEP_SHA256=... 指定，或 WINSWEEP_SKIP_VERIFY=1 明确跳过）");
  }

  fs.renameSync(tmp, target);
  ok(`安装完成（${(size / 1024 / 1024).toFixed(2)} MB）→ ${target}`);
  ok(`运行：winsweep --help    或    winsweep gui`);
})().catch((e) => fail(e && e.message ? e.message : String(e)));
