# Gpt Orbit

![非官方个人项目](https://img.shields.io/badge/非官方-个人项目-6d5dfc)

![Gpt Orbit Weekly 折叠状态](docs/acceptance/screenshots/weekly-collapsed.png)

## 下载

[前往最新版本下载页](https://github.com/316380027-spec/gpt-orbit/releases/latest)

首发 `v0.1.0` 只提供一个 Windows x64 安装包：`Gpt Orbit Weekly_0.1.0_x64-setup.exe`。请只从上述 GitHub Releases 页面下载。

## 功能

- 在轻量桌面悬浮组件中查看 Codex 每周额度。
- 显示本周剩余额度、倒计时和额度重置次数。
- 支持悬停展开、拖动定位、置顶、托盘显示与位置恢复。
- 断开数据源时保留最近一次已知结果，并明确标为过期状态。

## 系统要求

- Windows 11 x64。
- 已安装并登录官方 Codex 应用；Gpt Orbit 不提供独立的 OpenAI/Codex 账号。

这是非官方个人项目，与 OpenAI 无隶属、认可或赞助关系。

## 安装

1. 从[最新版本下载页](https://github.com/316380027-spec/gpt-orbit/releases/latest)下载 `Gpt Orbit Weekly_0.1.0_x64-setup.exe`。
2. 运行当前用户 NSIS 安装程序；如 Windows 显示来源提示，请核对文件名及 Release 页面中的 SHA-256。
3. 保持官方 Codex 登录有效，然后启动 Gpt Orbit。

应用使用独立的安装目录和位置设置，不会修改 Codex 的界面设置。

## 隐私与数据来源

Gpt Orbit 从本机 Codex `app-server` 读取并归一化额度状态；周额度版还会使用本机 Codex 登录材料向对应的 OpenAI HTTPS 端点读取重置次数。项目不设置中转服务器。

应用不会把 token、邮箱或完整的 `app-server` 响应写入日志或缓存。缓存只保存界面所需的归一化额度、重置次数、时间戳和过期状态。提交问题或截图前，也请移除登录 URL、token、账号标识和完整响应内容。

## 开发

需要 Node.js 22、Rust 与 Tauri 的 Windows 构建依赖。

```powershell
npm.cmd install
npm.cmd test -- --configLoader runner --run
npm.cmd run typecheck
npm.cmd run build
npm.cmd run build:weekly
npm.cmd run rust:test
```

发布前可运行公开树检查：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release/check-public-tree.ps1
```

## 已验证范围

当前版本只声明以下已验证场景：

- 前端 109 项测试与 Rust 149 项单元/集成测试通过，Weekly 构建通过。
- 当前用户安装包具有独立应用标识，并能保存窗口位置。
- 实时值、明确的零值、断线后过期回退、异常响应拒绝与恢复流程已通过本地回环集成测试。
- 仓库中的折叠截图来自已安装的 Gpt Orbit Weekly；安装包字节数与 SHA-256 记录在 [`docs/acceptance/release-manifest.md`](docs/acceptance/release-manifest.md)。

## 尚未验证的场景

- 浅色壁纸下的可读性。
- 实体混合 DPI 显示器组合。
- 卸载后对 Codex 凭据无影响的人工检查。
- 已安装周额度版展开状态的公开截图。
- Windows 11 x64 以外的平台与架构。

详情见 [`docs/acceptance/gpt-orbit-windows-matrix.md`](docs/acceptance/gpt-orbit-windows-matrix.md)。

## 许可证

本项目采用 [MIT License](LICENSE)。
