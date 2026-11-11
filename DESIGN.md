# DESIGN.md — WinSweep 主题系统

> 本文件是 WinSweep 视觉层的唯一事实来源。改 UI 前先读这里。
> 架构：**一套组件 + 多套主题令牌**。组件只引用语义变量，切换主题只换一组变量，不改结构。

---

## 3. 状态看板的布局规则

看板是"信息密度最高"的一屏，三条规则固定下来：

1. **一行三张指标卡**：等高（grid 自动拉伸），数值 32px 等宽数字，右上角胶囊显示百分比/核心数。
2. **下排双栏等高**：`align-items: stretch`，两卡高度由较高者决定，避免"一高一矮"的错位。
   - 左：**磁盘空间利用** —— 顶部汇总（总占用率大数字 + 满宽进度条）→ 两个小指标（可用空间 / 可清理估算）→ 分区列表（内滚，`max-height: 46vh`）→ 底部汇总条。
   - 右：**高负载活跃进程** —— 列表内滚 → 底部标注"按 CPU 占用排序 / 前 N 名"。
3. **卡片内部三段式**：标题 → 内容（可滚动区 `flex:1; min-height:0`）→ 底部脚注（`margin-top:auto` + 1px 上边线）。这样内容多时卡片内滚、内容少时脚注贴底，页面不会出现大片空洞。

长列表一律**卡片内滚动**，不要让整页被几条数据撑长。

## 4. 主题清单

| # | 主题 | data-theme | 明暗 | 语言 | 适合场景 |
| :- | :--- | :--- | :--- | :--- | :--- |
| 1 | 暗黑学院风 | `academia` | 深色 | 衬线（EB Garamond）+ 深棕/墨绿/暗金 | 夜间长时间使用、想要"书房"气质 |
| 2 | 柔和现代 | `soft` | 浅色 | Varela Round / Nunito Sans + 靛蓝 | 日常默认，柔和易读 |
| 3 | Windows 原生 | `fluent` | 浅色 | Segoe UI Variable + 系统蓝 #0F6CBD | 与 Windows 11 融为一体 |
| 4 | 极简瑞士 | `swiss` | 浅色 | Inter + 黑白红、直角、无阴影 | 想要克制、编辑排版感 |
| 5 | 深色 OLED | `oled` | 深色 | Inter + 纯黑底 + 薄荷绿/琥珀 | 夜间、OLED 省电、开发者气质 |

切换入口：侧栏左下角的**下拉菜单**（GitHub 式交互），不是常驻列表。

### 主题下拉的交互规格

| 项 | 约定 |
| :--- | :--- |
| 触发器 | 显示当前主题色点（主色 + 强调色描边）+ 名称 + 上折角；`aria-haspopup="menu"`、`aria-expanded` |
| 弹出方向 | 向上弹出（`bottom: calc(100% + 8px)`），与触发器同宽，`role="menu"` |
| 选项 | `role="menuitemradio"` + `aria-checked`；内容 = 三色板 + 名称 + 深色/浅色 + 勾选 |
| 打开 | 点击触发器，或聚焦时 Enter / Space / ↑ / ↓ |
| 移动 | ↑ ↓ 循环、Home / End 跳首尾（roving tabindex） |
| 选择 | Enter / 点击 → 立即切换并关闭，焦点回到触发器 |
| 关闭 | Esc、Tab、点击外部；关闭后 `aria-expanded="false"` |
| 记忆 | 写入 `localStorage["winsweep-theme"]`，刷新保持 |
| 动效 | 140ms 位移 + 轻微缩放，`prefers-reduced-motion` 下静止 |

## 2. 两套文字色，别混用

同一个主题里，"页面底色上的字"和"纸面/卡片上的字"往往不是同一种颜色（例如暗黑学院：页面是深棕，卡片是奶白）。

| 令牌 | 用在哪 |
| :--- | :--- |
| `--page-ink` / `--page-ink-soft` / `--page-ink-mute` | 页面底色上：页头标题与说明、侧栏外的一切裸文 |
| `--ink` / `--ink-soft` / `--ink-mute` | 卡片内部：列表行、指标数值、说明文字 |

组件里 `.card` 会重置 `color: var(--ink)`，`.page-head` 用 `--page-ink`。**新增组件时先判断它落在哪种底色上。**

## 3. 令牌分组（新增主题照抄这 8 组）

1. **底色**：`--bg`、`--bg-image`
2. **面**：`--surface`、`--surface-2`、`--surface-3`
3. **文字**：`--page-ink*`、`--ink*`、`--sel-bg`、`--sel-ink`
4. **线**：`--line`、`--line-strong`、`--head-rule`、`--section-rule`
5. **动作色**：`--primary*`、`--accent*`、`--danger*`、`--ok*`、`--warn*`
6. **形状与深度**：`--r-card`、`--r-ctrl`、`--r-tag`、`--r-pill`、`--bw`、`--bw-row`、`--shadow-card(-hover)`、`--shadow-btn(-hover)`、`--card-hover-tint`
7. **行与按钮的形态**：`--row-bg`、`--row-border`、`--row-sep`、`--row-radius`、`--row-hover-bg`、`--row-hover-shadow`、`--btn-bg`、`--btn-ink`、`--btn-border`、`--btn-hover-*`、`--btn-radius`、`--btn-pad`、`--btn-transform`、`--btn-tracking`、`--btn-fs`
8. **排版与节奏**：`--font-display`、`--font-body`、`--fs-body`、`--lh-body`、`--fs-h2`、`--track-h2`、`--fs-num`、`--track-num`、`--fs-rowname/desc/value`、`--fs-label`、`--track-label`、`--label-transform`、`--dur`、`--dur-card`、`--ease`、`--focus-ring`、`--focus-outline`、`--is-dark`
9. **侧栏**：`--sidebar-bg`、`--sidebar-image`、`--sidebar-border`、`--sidebar-ink`、`--sidebar-muted`、`--nav-*`、`--brand-*`

## 4. 各主题的形态差异（都在令牌里，不在组件里）

| 主题 | 卡片 | 列表行 | 按钮 | 标签 | 过渡 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 暗黑学院 | 2px 圆角、1px 暗金、内阴影纸面光晕 | 无底、烫金细线分隔 | 深棕皮革、大写、字距 .15em、16/40 内距 | 2px 直角、描边 | 700 / 1000ms ease-in-out |
| 柔和现代 | 16px、发丝线、柔影 | 白底描边小卡 12px | 10px 圆角、常规大小写 | 胶囊 | 200ms |
| Windows 原生 | 8px、极浅阴影 | 6px 小白卡 | 6px、系统按钮感 | 4px 圆角 | 150ms |
| 极简瑞士 | 直角、1px 黑边、无阴影 | 无底、1px 灰线分隔 | 直角、大写黑边（hover 反白） | 直角 | 180ms |
| 深色 OLED | 12px、1px 白 10% 边、无阴影 | 8px 深灰卡 | 8px、描边（hover 转薄荷边） | 6px | 160ms |

## 5. 新增一套主题（三步）

1. CSS 复制一个 `:root[data-theme="your-id"] { … }` 块，填满第 3 节列出的令牌（缺一个就会出现继承错色）。
2. JS 的 `THEMES` 数组加一项：`{ id:'your-id', label:'显示名', swatch:['主色','辅色','底色'], dark:true|false }`。
3. 重跑 `impeccable detect`，把报告里的对比度问题改掉（它按静态 CSS 推算配色）。

## 6. 无障碍底线（每套主题都要过）

- 文字对比 ≥4.5:1（大号数字 ≥3:1）；深色主题单独校验，别复用浅色值
- 非文本（描边、勾选框、进度条）≥3:1
- 焦点可见：`--focus-outline` + `--focus-ring` 组合，覆盖按钮/导航/主题项/输入
- 键盘：导航与主题项可 Tab 可达，Enter/Space 触发
- 状态不只靠颜色：标签带文字
- `prefers-reduced-motion: reduce` 下全部动效静止
- 响应式：≥1180 三列 / 960–1180 两列 / <960 单列（侧栏 → 顶栏）

## 7. 禁止

- ❌ 在组件里写死颜色/圆角/阴影（必须走令牌，否则换主题会漏）
- ❌ 卡片里再套卡片；同页出现两套按钮形态
- ❌ Emoji 当图标（统一内联 SVG，stroke 1.6–2）
- ❌ 用 `--ink` 去写页面底色上的文字（暗黑学院会直接看不见）
