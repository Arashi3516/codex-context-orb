# Context Orb

**专注，留一点余量。** 一个面向 Codex 的本地悬浮球，目标是在适当的时机提醒你整理进展、开始新会话。

> **v0.1.0 · 框架与交互预览。** 目前提供可运行的设计原型、Tauri 桌面壳、显式会话固定和 Codex Hooks 元数据接入。尚未实现自动跟随 Codex 当前选中会话、真实上下文用量接入、语义杂乱判断或系统通知。浏览器预览中的所有会话和用量均为模拟数据。

![Context Orb 交互预览](docs/assets/design-preview.png)

独立开源项目，与 OpenAI 无隶属关系。项目以 Windows + macOS 桌面应用为主，可选 Codex 插件作为接入与交接辅助。官方目录上架与签名安装包是后续独立验收目标。

## 立即体验

需要 **Node.js 22.12+**。

```sh
git clone https://github.com/Arashi3516/codex-context-orb.git
cd codex-context-orb
npm ci
npm run dev
```

打开 [http://127.0.0.1:1427](http://127.0.0.1:1427)。也可以在已克隆目录执行 `node scripts/start-preview.mjs`，由脚本安装锁定依赖并启动预览。Windows 与 macOS 使用同一组命令。

可操作内容：

- 四种状态：余量充足、留意余量、建议交接、数据未知。
- 点击悬浮球展开/收起；拖动定位；Escape 收起；键盘操作。
- 手动固定 A 会话，模拟 B 会话后台更新，检查不会误切换。
- 深浅主题、暂停/恢复当前会话的提醒展示。
- 编辑并复制交接**模板**。模板不假装是 AI 已完成的会话总结。

## 桌面框架

安装 [Tauri 平台前置依赖](https://v2.tauri.app/start/prerequisites/) 与 Rust 后运行：

```sh
npm ci
npm run desktop:dev
```

先关闭已占用 1427 端口的独立预览服务。桌面版是实际置顶窗口，默认状态为未知。它只读取自有数据目录里的有限 hook 元数据；不会读取 Codex 对话正文、账号或凭证。

```sh
npm run desktop:build
```

此命令用于开发打包。当前仓库未把未签名构建标为可正式分发的成品。GitHub Actions 的 **Package development preview** 可手动构建 Mac / Windows 预览产物；系统签名、Windows 真机测试、升级与卸载验收见 [分发方案](docs/DISTRIBUTION.md)。

## Codex 插件

[plugins/codex-context-orb](plugins/codex-context-orb/README.md) 包含：

- 合法的 `.codex-plugin/plugin.json` 和聚焦的 context-health skill。
- 异步生命周期 hooks 及有限元数据适配器。
- 有界只读 inspector 和机器可读快照契约。

插件源代码已提供，**不代表已安装、已上架，或能单独提供系统悬浮窗**。本阶段开发接入需要 Python 3；正式桌面安装包将收敛运行时依赖。不要把本地插件包直接等同于官方目录审核通过。

## 设计与架构

| 文档 | 内容 |
| --- | --- |
| [UI / UX](docs/UIUX.md) | 悬浮球、面板、状态、交互、可访问性与提醒原则 |
| [架构](docs/ARCHITECTURE.md) | 桌面端、接入、判定的边界及真实指标要求 |
| [分发](docs/DISTRIBUTION.md) | GitHub 安装与 Codex 插件上架路线 |
| [路线图](docs/ROADMAP.md) | 阶段交付与验收门槛 |
| [验证记录](docs/VERIFICATION.md) | 本版本实际完成与尚未完成的验证 |

```text
src/                      React 悬浮球、设计预览、核心规则
src/lib/                  显式身份绑定与可解释容量判定
src-tauri/                Tauri 2 本地窗口与有界元数据读取
plugins/codex-context-orb/ 可选 Codex 插件
docs/                     框架方案、UIUX、分发与验收
```

## 开发检查

```sh
npm run check
python3 plugins/codex-context-orb/scripts/test_hooks.py
cargo test --manifest-path src-tauri/Cargo.toml
```

Windows 使用 `py -3` 代替 `python3`。浏览器流程测试使用 `npm run test:ui`，需先安装 Playwright Chromium（`npx playwright install chromium`）。

## 不变的原则

**不猜。** 缺失、异常、过期用量显示未知；累计计费 tokens 不能冒充当前上下文。

**不串。** 当前固定会话与后台活跃会话明确区分；自动前台绑定未验证前不宣传。

**不打断。** 不自动停止、压缩、分叉或创建 Codex 会话。

**不上传。** 当前版本不调用外部 AI，也不上传对话；示例、截图与测试不含真实会话数据。

## License

[MIT](LICENSE).
