# Codex Orbit Windows MVP 实施计划

> 日期：2026-07-12  
> 状态：待执行  
> 范围：Windows + Tauri 2 技术验证版，优先完成真实额度读取、5 小时/周额度悬停切换和透明桌面小组件。

## 1. 目标与成功标准

交付一个可运行的 Windows 桌面小组件：默认显示 Codex 5 小时额度；鼠标移入组件后自动切换为周额度；鼠标移出后恢复 5 小时额度。额度来自 Codex 官方本地 `app-server`，不截图、不爬网页、不读取对话正文。

首版成功标准：

- 登录后能读取 `account/rateLimits/read`。
- 能同时解析 5 小时窗口与周窗口。
- UI 展示“剩余百分比”，即 `100 - usedPercent`，并限制在 0–100。
- 默认卡片展示 5 小时额度和对应重置倒计时。
- 鼠标稳定移入 150 ms 后，卡片平滑切换为周额度。
- 鼠标移出 200 ms 后，卡片自动恢复 5 小时额度。
- 快速反复掠过时不闪烁、不出现旧定时器覆盖新状态。
- 周额度缺失时，悬停不切换，并显示“当前套餐未返回周额度”。
- 额度事件到达后 1 秒内更新；事件缺失时定时补偿刷新。
- 断网或 App Server 暂时不可用时保留最后一次有效数据并标记“数据可能已过期”。
- 连续运行 24 小时不崩溃，空闲 CPU 目标低于 1%。

## 2. 已验证的技术事实

- Codex App Server 支持 `account/rateLimits/read` 和 `account/rateLimits/updated`。
- 返回窗口包含 `usedPercent`、`windowDurationMins` 和 `resetsAt`。
- 当前常见数据中，`primary` 是 300 分钟窗口，`secondary` 是 10080 分钟窗口；实现必须优先按窗口时长分类，并保留字段位置作为兼容回退，不能把 300/10080 当作永远不变的业务常量。
- `resetsAt` 是 Unix 秒时间戳，展示时转换为用户本地时区。
- App Server 通知可能是稀疏更新；缺失的可空账户字段不能错误清空已有值。
- 额度百分比是账户配额快照，不是本地线程 token 数量。

参考实现与问题记录：

- [Codex App Server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)
- [Codex App Server 接口文档](https://github.com/openai/codex/blob/main/codex-rs/docs/codex_mcp_interface.md)
- [5 小时与周额度数据形状记录](https://github.com/openai/codex/issues/24080)
- [历史快照可能被误当成实时周额度的问题](https://github.com/openai/codex/issues/23190)
- [Tauri 透明区域鼠标穿透讨论](https://github.com/tauri-apps/tauri/issues/13070)

## 3. MVP 架构

```text
Codex app-server 子进程
  -> JSON-RPC 连接与认证
  -> 额度数据适配层
  -> 应用状态（5h、weekly、连接状态、采集时间）
  -> React 卡片 / 进度环 / 倒计时
  -> Tauri 透明窗口与托盘
```

建议技术栈：

- Tauri 2 + Rust：进程生命周期、JSON-RPC、窗口、托盘和本地设置。
- React + TypeScript：小组件界面与交互。
- Zustand：轻量状态管理。
- Vitest + Testing Library：数据适配器、状态机和组件测试。
- Rust 单元测试：JSON-RPC 消息关联、进程重启和数据转换。
- 首版使用 CSS/Canvas 2D 动效；Three.js 和复杂粒子放到额度链路稳定之后。

## 4. 核心数据模型

前端不直接消费 App Server 原始结构，统一转换为：

```ts
type QuotaWindowKind = 'fiveHour' | 'weekly' | 'other';

interface QuotaWindow {
  kind: QuotaWindowKind;
  usedPercent: number;
  remainingPercent: number;
  windowDurationMins: number;
  resetsAt: number | null;
}

interface RateLimitState {
  fiveHour: QuotaWindow | null;
  weekly: QuotaWindow | null;
  other: QuotaWindow[];
  planType: string | null;
  reachedType: string | null;
  fetchedAt: number;
  source: 'read' | 'updated' | 'cache';
  stale: boolean;
}
```

窗口分类顺序：

1. 先检查 `windowDurationMins`：接近 300 分钟归类为 5 小时，接近 10080 分钟归类为周额度。
2. 无法按时长识别时，再将 `primary` 作为 5 小时、`secondary` 作为周额度的兼容回退。
3. 无法可靠识别的窗口进入 `other`，不冒充周额度。
4. 同一类别出现多个候选时，优先选择 `limitId === 'codex'`，其次选择最新有效快照；记录诊断日志但不记录凭证。

## 5. 悬停切换交互规格

### 5.1 状态机

```text
FIVE_HOUR
  -- pointer enter + 150ms --> WEEKLY
WEEKLY
  -- pointer leave + 200ms --> FIVE_HOUR
任意状态
  -- weekly 缺失 --> FIVE_HOUR
  -- 设置面板打开 --> 锁定当前视图，关闭面板后恢复 FIVE_HOUR
```

### 5.2 展示规则

- 默认标题：“5 小时额度”。
- 悬停标题：“周额度”。
- 数字展示剩余百分比，而不是 `usedPercent`。
- 切换时百分比、进度环、标题和重置时间必须来自同一个窗口，禁止不同窗口字段交叉。
- 动画时长 180–240 ms，使用淡出/淡入或轻微翻转；尊重系统“减少动态效果”设置。
- 切换后在角落显示轻量标签“5h”或“7d”，防止用户只看到数字却不知道当前窗口。
- 周额度缺失时保留 5 小时视图，可在详情区显示缺失原因，不弹阻断提示。
- 键盘可访问：组件获得焦点后，按 `W` 临时查看周额度，按 `Esc` 恢复；屏幕阅读器播报当前额度窗口。

### 5.3 防抖与竞态

- `pointerenter` 与 `pointerleave` 各自只保留一个定时器。
- 每次新事件先取消相反方向的未完成定时器。
- 组件卸载时清理全部定时器。
- 数据更新不重置用户当前悬停状态，只替换当前窗口数据。
- UI 不自行每秒重新请求额度；每秒只本地更新倒计时。

### 5.4 鼠标穿透冲突

Tauri 开启 `setIgnoreCursorEvents(true)` 后，WebView 无法收到标准悬停事件。MVP 采用以下产品规则：

- 默认关闭鼠标穿透，确保悬停切换可用。
- 用户主动开启鼠标穿透后，界面明确提示“悬停切换暂停”；仍可从托盘查看周额度。
- V0.5 再评估 Rust 全局鼠标位置 + 组件命中区域的方案，只在鼠标进入可视区域时临时关闭穿透。该方案必须先验证多显示器、DPI 缩放、窗口拖动及空闲 CPU 占用，验证通过后才默认启用。

## 6. 分阶段实施任务

### 阶段 A：项目骨架与验证夹具

建议文件：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/capabilities/default.json`
- `src/main.tsx`
- `src/App.tsx`
- `src/test/fixtures/rate-limits.json`

任务：

1. 初始化 Tauri 2 + React + TypeScript。
2. 固定 Node、Rust 和 Codex CLI 最低支持版本。
3. 加入格式化、类型检查、前端测试和 Rust 测试命令。
4. 建立额度响应夹具：完整双窗口、仅 5 小时、空值、越界百分比、稀疏通知、未知窗口。
5. 先用夹具渲染卡片，避免 UI 开发被真实登录流程阻塞。

完成条件：开发模式窗口可启动，所有夹具能被测试加载，基础检查全绿。

### 阶段 B：App Server 连接与认证

建议文件：

- `src-tauri/src/app_server/process.rs`
- `src-tauri/src/app_server/rpc.rs`
- `src-tauri/src/app_server/messages.rs`
- `src-tauri/src/app_server/mod.rs`
- `src-tauri/src/commands/rate_limits.rs`

任务：

1. 由 Tauri 后端启动并监管 `codex app-server`，使用 stdio JSONL 通信。
2. 完成初始化握手，并用递增请求 ID 关联响应。
3. 调用 `account/read` 判断登录状态；未登录时使用官方 ChatGPT 登录流程。
4. 登录完成后调用 `account/rateLimits/read`。
5. 持续读取 `account/rateLimits/updated` 通知。
6. 子进程退出时按 1s、2s、4s、8s、30s 上限退避重启；应用退出时清理子进程。
7. 日志只记录方法、请求 ID、错误类别和版本，不记录 token、邮箱或完整响应。

完成条件：真实账户能返回一份额度快照；断开并重启 App Server 后自动恢复。

### 阶段 C：额度适配层与缓存

建议文件：

- `src/lib/rateLimits/types.ts`
- `src/lib/rateLimits/normalize.ts`
- `src/lib/rateLimits/classify.ts`
- `src/stores/rateLimitStore.ts`
- `src-tauri/src/cache.rs`

任务：

1. 解析主额度对象及 `rateLimitsByLimitId`。
2. 对百分比做有限数校验和 0–100 截断。
3. 计算 `remainingPercent = 100 - usedPercent`。
4. 按第 4 节规则识别 5 小时和周额度。
5. 合并稀疏通知：通知未携带的字段保留上次值。
6. 每次成功读取写入本地缓存，并保存 `fetchedAt`；缓存只包含展示需要的非敏感字段。
7. 启动时可以立即展示缓存，但必须标记 stale，直到实时读取成功。
8. 监听事件为主，每 5 分钟补偿读取一次；窗口显示、托盘刷新、系统唤醒时立即读取。

完成条件：所有夹具单元测试通过；历史缓存绝不会被标为实时数据。

### 阶段 D：5 小时/周额度悬停组件

建议文件：

- `src/components/QuotaWidget/QuotaWidget.tsx`
- `src/components/QuotaWidget/useQuotaViewMode.ts`
- `src/components/QuotaWidget/QuotaRing.tsx`
- `src/components/QuotaWidget/ResetCountdown.tsx`
- `src/components/QuotaWidget/QuotaWidget.css`
- `src/components/QuotaWidget/QuotaWidget.test.tsx`

任务：

1. 先写 `useQuotaViewMode` 的假时钟测试：移入延迟、移出延迟、快速掠过、卸载清理、周额度缺失。
2. 实现默认 5 小时、悬停周额度状态机。
3. 统一切换标题、百分比、进度环和重置时间。
4. 加入本地时区重置时间与动态倒计时。
5. 加入加载、未登录、离线、陈旧数据和额度耗尽状态。
6. 加入 `prefers-reduced-motion` 和键盘操作。

完成条件：组件测试覆盖全部状态；人工连续快速移入移出 30 次无闪烁或错误窗口。

### 阶段 E：Windows 桌面窗口与托盘

建议文件：

- `src-tauri/tauri.conf.json`
- `src-tauri/src/window.rs`
- `src-tauri/src/tray.rs`
- `src/components/Settings/Settings.tsx`

任务：

1. 配置透明、无边框、可拖动、可选始终置顶窗口。
2. 保存窗口位置、尺寸、显示器和缩放；显示器断开后把窗口恢复到可见区域。
3. 托盘提供显示/隐藏、立即刷新、查看 5 小时、查看周额度、始终置顶、开机启动和退出。
4. 默认关闭鼠标穿透；开启时按第 5.4 节提示交互限制。
5. 窗口隐藏或锁屏时暂停动画，恢复时先刷新额度。

完成条件：多显示器和 100%/125%/150% DPI 下位置正确；重启后恢复；托盘可以可靠退出进程。

### 阶段 F：视觉主题与额度联动

建议文件：

- `src/themes/types.ts`
- `src/themes/glass.ts`
- `src/themes/minimal.ts`
- `src/components/Orb/Orb.tsx`

任务：

1. 先完成毛玻璃与极简圆环两套低成本主题。
2. 用 CSS 变量将剩余额度映射到亮度、颜色和警告光环。
3. 额度重置时仅播放一次恢复动画，避免重复通知触发。
4. 低于 30%、15%、5% 时分级提示；每个窗口、每个重置周期只提醒一次。
5. 宇宙 3D、像素主题和模型星体映射在额度 MVP 稳定后进入 V0.5。

完成条件：主题切换不影响额度状态；空闲性能达到目标。

### 阶段 G：端到端验收与发布准备

任务：

1. 用模拟 App Server 覆盖登录、双窗口、稀疏通知、断线、重启和坏数据。
2. 用真实账户对比 Codex Usage 页面，记录百分比、重置时间和采集时间。
3. 验证 Windows 冷启动、睡眠恢复、网络切换、Codex 升级和账户退出。
4. 跑 24 小时稳定性测试，记录 CPU、内存、子进程数量和重连次数。
5. 生成未签名内部测试包；签名与自动更新在功能验收后单独处理。

完成条件：第 1 节成功标准全部满足，已知差异有清楚的用户提示和日志证据。

## 7. 测试清单

### 数据测试

- `primary=300`、`secondary=10080` 正确分类。
- 只有 `primary` 时周额度为 null。
- 字段顺序变化时仍按时长分类。
- `usedPercent` 为 -1、101、NaN、null 时安全处理。
- `resetsAt` 为过去时间或缺失时显示“等待刷新”，不显示负倒计时。
- 稀疏通知不清空 `planType` 和已有周额度。
- 缓存加载后为 stale，实时读取成功后解除 stale。

### 悬停测试

- 初始一定是 5 小时额度。
- 移入不足 150 ms 不切换。
- 移入达到 150 ms 切换周额度。
- 移出不足 200 ms 保持周额度，达到后恢复。
- 150 ms 内连续 enter/leave 不残留定时器。
- 悬停期间收到新数据时仍显示周额度的新值。
- 周额度消失时立即、安全地回到 5 小时额度。
- 减少动态效果开启时不播放翻转动画。

### 桌面测试

- Windows 11，单屏与多屏。
- 100%、125%、150% DPI。
- 始终置顶开/关。
- 鼠标穿透开/关及对应提示。
- 睡眠、锁屏、网络断开与恢复。
- App Server 崩溃后只有一个新子进程，不产生僵尸进程。

## 8. 范围边界

MVP 不承诺：

- 预测还能发送多少条消息。
- 读取或展示用户对话正文。
- 准确识别官方桌面应用全局唯一的“当前模型”。
- 在鼠标穿透开启时仍提供标准 WebView 悬停。
- 首版同时完成三套复杂动画主题、主题市场和 macOS。

## 9. 推荐执行顺序与里程碑

- 里程碑 1（技术闭环）：阶段 A–C。目标是拿到真实 5 小时与周额度并可靠缓存。
- 里程碑 2（用户核心价值）：阶段 D。目标是完成默认 5 小时、悬停周额度的稳定交互。
- 里程碑 3（桌面产品化）：阶段 E。目标是透明窗口、托盘、位置恢复和开机启动。
- 里程碑 4（视觉与发布）：阶段 F–G。目标是主题、性能、稳定性和内部安装包。

每个里程碑结束都应产出一段可运行演示，并通过对应自动化测试后再进入下一阶段。
