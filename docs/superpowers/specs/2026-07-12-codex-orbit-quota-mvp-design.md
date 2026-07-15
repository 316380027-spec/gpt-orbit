# Codex Orbit 额度核心 MVP 设计规范

> 日期：2026-07-12  
> 状态：已完成交互式设计确认，待书面审阅  
> 目标平台：Windows 11  
> 产品范围：真实额度读取、5 小时/周额度悬停切换、倒计时、断线缓存、透明桌面窗口和基础托盘

## 1. 产品目标

Codex Orbit MVP 是一个 Windows 桌面额度小组件。它通过 Codex 官方本地 `app-server` 获取账户额度，不截图、不爬网页、不读取对话正文。

小组件默认显示 5 小时额度。鼠标稳定移入后自动切换为周额度，鼠标移出后恢复 5 小时额度。用户无需打开 Codex Usage 页面即可看到剩余额度与重置时间。

## 2. 成功标准

- 真实 ChatGPT/Codex 账户能够完成官方登录并读取额度。
- 同时识别 5 小时额度和周额度；周额度不存在时安全降级。
- 默认展示 5 小时额度。
- 鼠标移入 150ms 后展示周额度，移出 200ms 后恢复 5 小时额度。
- 快速反复移入、移出时不闪烁，不发生旧定时器覆盖新状态。
- 展示剩余百分比，即 `100 - usedPercent`，结果限制在 0–100。
- 百分比和重置时间与 Codex Usage 页面基本一致，重置时间误差不超过 1 分钟。
- 额度变更通知到达后 1 秒内更新界面。
- App Server 或网络断开后保留最后一次有效数据，恢复后自动刷新。
- 连续运行 24 小时无崩溃、无重复 App Server 子进程。
- 空闲 CPU 目标低于 1%，内存目标低于 150MB。
- 不读取对话正文，不上传凭证，不在日志中记录敏感数据。

## 3. 范围

### 3.1 MVP 包含

- Tauri 2 Windows 桌面应用。
- 透明、无边框、可拖动的毛玻璃额度卡片。
- Rust 管理的 `codex app-server` 子进程。
- 官方 ChatGPT 登录状态检测与登录流程。
- `account/rateLimits/read` 完整读取。
- `account/rateLimits/updated` 增量通知。
- 5 小时与周额度分类和展示。
- 本地重置倒计时。
- 最后一次有效额度缓存与陈旧状态提示。
- 5 分钟补偿刷新、系统唤醒刷新、手动刷新。
- 基础系统托盘。
- 窗口位置、置顶状态和开机启动设置。
- 键盘查看周额度和减少动态效果支持。

### 3.2 MVP 不包含

- 当前或最近活跃模型识别。
- Sol、Terra、Luna 星体映射。
- Three.js、3D 星球、复杂粒子和像素主题。
- 额度阈值系统通知和声音提醒。
- 主题市场、主题导入和账户云同步。
- macOS 支持。
- 预测剩余消息数量。
- 鼠标穿透开启时的全局鼠标命中检测。

## 4. 技术路线

采用 Tauri 2 + Rust + React + TypeScript。

- Rust 负责 App Server 进程、JSON-RPC、官方登录、额度读取、通知监听、重连、缓存、窗口和托盘。
- React 负责额度卡片、进度环、悬停状态机和可访问性。
- Zustand 保存前端额度快照、连接状态、采集时间与显示模式。
- Vitest 和 Testing Library 覆盖 TypeScript 与 React 行为。
- Rust 自带测试框架覆盖进程、RPC、缓存和日志过滤。

选择 Tauri 的原因是小组件需要透明窗口、系统托盘和较低的空闲资源占用。Electron 开发更快但资源目标较难保证；WinUI 3 的 Windows 集成更深，但开发成本更高并限制后续跨平台扩展。

## 5. 架构与边界

```text
Codex app-server 子进程
        |
        | stdio JSONL / JSON-RPC
        v
Rust AppServerSupervisor
        |
        +--> RpcClient
        +--> AccountSession
        +--> RateLimitRepository
        +--> RateLimitCache
        |
        | Tauri event / command
        v
React rateLimitStore
        |
        +--> QuotaWidget
        +--> QuotaRing
        +--> ResetCountdown
        +--> ConnectionIndicator
```

### 5.1 AppServerSupervisor

职责：启动一个 `codex app-server` 子进程，检测退出，执行指数退避重启，并在应用退出时清理子进程。

约束：任意时刻最多存在一个由小组件管理的 App Server 子进程。重启延迟依次为 1、2、4、8 秒，之后每次最多等待 30 秒。成功稳定连接后重置退避计数。

### 5.2 RpcClient

职责：完成初始化握手，生成递增请求 ID，将响应关联到等待中的请求，并把无 ID 消息分发为通知。

约束：进程断开时，所有等待中的请求必须以明确连接错误结束，不能永久挂起。

### 5.3 AccountSession

职责：调用 `account/read` 检测登录状态。已登录时进入额度读取；未登录时发起官方 ChatGPT 登录流程，登录完成后重新读取账户与额度。

约束：不直接读取浏览器 Cookie，不要求用户复制 Access Token，不自行保存密码。

### 5.4 RateLimitRepository

职责：调用 `account/rateLimits/read`，监听 `account/rateLimits/updated`，合并稀疏通知，规范化窗口并向前端发布快照。

约束：React 不接收 App Server 原始响应，也不接触认证凭证。

### 5.5 RateLimitCache

职责：只保存展示必需的非敏感额度字段和采集时间。应用启动时可立即恢复缓存，但在实时读取成功前必须标记为历史数据。

### 5.6 React 展示层

职责：消费统一额度快照，实现悬停状态机、倒计时、加载/离线/历史状态和键盘交互。

约束：前端不负责额度网络轮询；每秒只在本地更新倒计时。

## 6. 数据模型

```ts
type QuotaWindowKind = 'fiveHour' | 'weekly' | 'other';

interface QuotaWindow {
  kind: QuotaWindowKind;
  usedPercent: number;
  remainingPercent: number;
  windowDurationMins: number;
  resetsAt: number | null;
}

type RateLimitSource = 'read' | 'updated' | 'cache';

interface RateLimitState {
  fiveHour: QuotaWindow | null;
  weekly: QuotaWindow | null;
  other: QuotaWindow[];
  planType: string | null;
  reachedType: string | null;
  fetchedAt: number;
  source: RateLimitSource;
  stale: boolean;
}

type ConnectionStatus =
  | 'starting'
  | 'loginRequired'
  | 'refreshing'
  | 'live'
  | 'offline';
```

`resetsAt` 与 `fetchedAt` 均为 Unix 秒。前端显示时转换为 Windows 当前本地时区。

## 7. 额度窗口分类

数据适配器读取顶层 `rateLimits` 及 `rateLimitsByLimitId`，只把可验证的窗口映射到界面。

分类顺序：

1. `windowDurationMins` 在 240–360 分钟之间时分类为 `fiveHour`。
2. `windowDurationMins` 在 9360–10800 分钟之间时分类为 `weekly`。
3. 无法按时长识别时，顶层 `primary` 可作为 `fiveHour` 回退，顶层 `secondary` 可作为 `weekly` 回退。
4. 其他窗口进入 `other`，不冒充 5 小时或周额度。
5. 同类别有多个候选时优先 `limitId === "codex"`，其次优先顶层 `rateLimits`，再选择具有有效 `resetsAt` 的候选。

字段校验：

- `usedPercent` 必须是有限数字；无效窗口不进入展示状态。
- 有效百分比限制在 0–100。
- `remainingPercent = 100 - usedPercent`。
- `windowDurationMins` 必须为大于 0 的有限数字。
- `resetsAt` 缺失或无效时保存为 `null`。

## 8. 数据流

1. 应用启动并读取本地缓存。
2. 有缓存时立即展示，但设置 `source = "cache"` 和 `stale = true`。
3. Rust 启动 `codex app-server` 并完成初始化握手。
4. 调用 `account/read`。
5. 已登录时调用 `account/rateLimits/read`；未登录时进入官方 ChatGPT 登录流程。
6. 完整响应经规范化后写入缓存并推送给前端，设置 `source = "read"` 和 `stale = false`。
7. `account/rateLimits/updated` 到达后与现有快照合并，再规范化、缓存并推送，设置 `source = "updated"`。
8. 通知没有携带的可空字段保留旧值，不能因缺失而清空周额度、套餐或 credits 元数据。
9. 正常连接时每 5 分钟调用一次 `account/rateLimits/read` 作为补偿刷新。
10. 窗口从隐藏变为可见、系统从睡眠恢复、用户点击托盘刷新时立即读取。

## 9. 界面设计

小组件为固定尺寸毛玻璃卡片，首版不使用 3D 引擎。

- 顶部：窗口标签“5 小时”或“本周”。
- 中部：圆形额度进度环与剩余百分比。
- 底部：相对重置倒计时，例如“2 小时 18 分后重置”。
- 右下角：连接状态圆点。
- 详情辅助文本：本地绝对重置时间和最后更新时间。

状态圆点语义：

- 蓝色：正在启动或刷新。
- 绿色：实时数据。
- 黄色：展示历史数据或实时读取失败。
- 灰色：离线且没有可用数据。

`rateLimitReachedType` 非空或剩余百分比为 0 时，进度环进入耗尽状态并突出重置倒计时。

## 10. 悬停状态机

初始显示模式为 `fiveHour`。

```text
fiveHour
  -- pointerenter 保持 150ms --> weekly
weekly
  -- pointerleave 保持 200ms --> fiveHour
任意状态
  -- weekly 变为 null --> fiveHour
  -- Esc --> fiveHour
```

具体规则：

- `pointerenter` 启动 150ms 切换定时器，并取消尚未执行的恢复定时器。
- `pointerleave` 启动 200ms 恢复定时器，并取消尚未执行的切换定时器。
- 定时器触发时再次检查指针状态和周额度是否存在。
- 组件卸载时清理两个定时器。
- 数据更新不改变当前悬停状态；如果仍处于悬停，则展示新的周额度。
- 周额度缺失时不切换，详情辅助文本显示“当前账户未返回周额度”。
- 标题、百分比、进度环和重置时间绑定到同一个 `QuotaWindow`，作为整体切换。
- 默认使用约 200ms 淡入淡出；系统设置 `prefers-reduced-motion: reduce` 时取消过渡。
- 组件取得键盘焦点后，按 `W` 查看周额度，按 `Esc` 恢复 5 小时。
- 可访问名称包含当前窗口、剩余百分比与重置时间。

## 11. 窗口与托盘

窗口默认行为：

- 透明、无系统边框、可拖动。
- 默认始终置顶，用户可从托盘关闭。
- 默认关闭鼠标穿透，保证悬停切换可用。
- 保存窗口位置、尺寸、显示器标识和缩放信息。
- 显示器不可用时将窗口恢复到主显示器可见区域。
- 窗口隐藏或系统锁屏时暂停视觉动画；恢复时立即刷新额度。

鼠标穿透为可选设置。用户开启后，界面和托盘明确提示“鼠标穿透开启时，悬停切换不可用”。MVP 不监听 Windows 全局鼠标位置。

托盘菜单：

- 显示/隐藏小组件。
- 立即刷新。
- 临时查看 5 小时额度。
- 临时查看周额度；周额度不存在时禁用。
- 始终置顶开关。
- 鼠标穿透开关。
- 开机启动开关。
- 退出。

## 12. 错误处理

### 12.1 App Server 不存在

显示“未找到 Codex CLI”，提供诊断信息，但不自动下载或执行不受信任的安装脚本。保留本地缓存并标记为历史数据。

### 12.2 未登录

显示登录引导并启动官方 ChatGPT 登录流程。登录未完成前不反复弹出浏览器。

### 12.3 子进程退出

终止所有等待中的 RPC 请求，设置连接状态为离线，并由唯一 supervisor 按退避策略重启。

### 12.4 网络失败

保留最后一次有效额度。状态圆点变黄，并显示最后更新时间。网络恢复或下一次补偿读取成功后恢复实时状态。

### 12.5 重置时间已过

如果 `resetsAt <= 当前时间` 且没有新快照，倒计时显示“等待刷新”，立即触发一次读取，不显示负时间。

### 12.6 坏数据

丢弃无法验证的窗口并记录无敏感信息的诊断事件。如果旧快照仍有效则继续展示旧快照；没有有效快照时显示数据不可用。

## 13. 隐私与日志

- 不读取浏览器 Cookie。
- 不读取或记录对话正文、提示词和工作区文件。
- 不要求用户复制密码或 Access Token。
- 不向第三方服务器上传账户数据或额度数据。
- 本地缓存只包含规范化额度字段、套餐类型、限制状态和采集时间。
- 日志只记录 App Server 方法、请求 ID、Codex 版本、生命周期事件和错误类别。
- 日志不得包含 token、邮箱、登录 URL 中的敏感参数或完整原始响应。

## 14. 测试策略

所有行为采用红—绿—重构的 TDD 顺序。

### 14.1 Rust 单元测试

- 递增请求 ID 与乱序响应正确关联。
- 无 ID JSON-RPC 消息作为通知分发。
- 进程断开使等待请求返回连接错误。
- 退避延迟为 1、2、4、8、30 秒上限。
- 多次退出不会生成多个子进程。
- 登录状态驱动正确的额度读取流程。
- 缓存不包含敏感字段。
- 日志过滤 token、邮箱和完整响应。

### 14.2 TypeScript 单元测试

- 300 分钟 `primary` 分类为 5 小时。
- 10080 分钟 `secondary` 分类为周额度。
- 字段顺序变化时仍按时长分类。
- 只有 5 小时窗口时 `weekly = null`。
- `usedPercent` 的 -1、101 被限制到 0、100。
- `NaN`、字符串和 null 百分比被拒绝。
- 稀疏通知不清空旧的周额度与套餐字段。
- 缓存快照始终以 `stale = true` 恢复。

### 14.3 React 组件测试

- 初始显示 5 小时额度。
- 移入不足 150ms 不切换。
- 移入达到 150ms 切换周额度。
- 移出不足 200ms 保持周额度，达到后恢复。
- 快速 enter/leave 不留下会延迟执行的旧定时器。
- 悬停时收到新快照仍展示新的周额度。
- 周额度变为 null 时立即恢复 5 小时。
- 组件卸载后没有定时器更新。
- 减少动态效果时没有过渡动画。
- `W` 和 `Esc` 键盘操作正确。

### 14.4 模拟 App Server 集成测试

- 已登录并返回完整双窗口。
- 未登录、完成官方登录后返回额度。
- 完整响应后收到稀疏通知。
- 子进程退出后自动重启并重新读取。
- 请求超时后继续展示缓存。
- 坏 JSON 行不会使整个应用退出。

### 14.5 Windows 人工验收

- Windows 11 单屏和多屏。
- 100%、125%、150% DPI。
- 窗口位置保存与不可用显示器恢复。
- 始终置顶开关。
- 鼠标穿透开关及悬停不可用提示。
- 睡眠、锁屏、网络断开与恢复。
- 托盘显示、隐藏、刷新和退出。
- 与 Codex Usage 页面比较百分比和重置时间。
- 24 小时稳定性与资源占用记录。

## 15. 交付物

- 可运行的 Tauri 2 Windows 项目。
- Rust App Server 管理与额度数据层。
- React 毛玻璃额度小组件。
- 自动化单元测试和模拟 App Server 集成测试。
- Windows 人工验收记录。
- 未签名的内部测试安装包。

代码签名、自动更新和公开发布不属于本 MVP。
