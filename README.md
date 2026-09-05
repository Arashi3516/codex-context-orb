# Context Orb

**专注，让思路清楚。** 面向 Codex 的本地悬浮球：多次压缩后，当旧信息与未解决的冲突开始干扰下一步，建议整理有效信息并新开会话。

> **v0.2.0 · 语义评估框架与交互预览。** 已实现可解释规则、证据面板、澄清后复查演示、干净交接模板，以及 Codex skill → 本地报告 → 桌面读取通路。真实评估目前由用户主动发起。自动后台评估、系统通知和前台会话自动跟随尚未实现；规则尚未用真实任务校准。

![Context Orb 交互预览](docs/assets/design-preview.png)

独立开源项目，与 OpenAI 无隶属关系。目标平台为 Windows 和 macOS，采用 Tauri 应用与可选 Codex 插件。

## 判断什么

| 观察 | 处理 |
| --- | --- |
| 会话很长、压缩很多次，但目标和下一步一致 | 可以继续 |
| 可核对的约束遗漏或冲突仍影响下一步 | 先澄清，再复查 |
| 多次压缩后，纠正过的问题复现，并有不同执行偏差印证 | 建议整理干净交接 |
| 身份不匹配、报告过期、后续活动或证据不足 | 等待评估 |

容量、轮数和文件大小不参与健康判定，没有“信息熵百分比”。[规则与反例](docs/SEMANTIC-REVIEW.md)。

## 立即体验

需要 Node.js 22.12+，Windows 与 macOS 使用同一组命令：

```sh
git clone https://github.com/Arashi3516/codex-context-orb.git
cd codex-context-orb
npm ci
npm run dev
```

打开[本地预览](http://127.0.0.1:1427)。也可执行 `node scripts/start-preview.mjs` 安装锁定依赖并启动。

浏览器中的会话、压缩次数和证据均为虚构。可切换状态、查看依据、模拟澄清、固定会话、暂停提醒展示、编辑并复制交接模板。支持深浅主题、拖动和键盘操作。

## 桌面与插件

安装 [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)与 Rust，关闭占用 1427 端口的独立预览，再运行 `npm run desktop:dev`。

原生窗口默认未绑定、等待评估。启用[仓库插件](plugins/codex-context-orb/README.md)后，在目标 Codex 会话中请求 `$context-health`。身份可验证时，skill 将最小化评估写入本机，桌面读取显示。复制评估指令本身不会运行评估。

Hook 仅保存生命周期元数据；手动评估另存目标、下一步、问题摘要与证据位置。这些摘要可能包含项目私密信息，只应保存在本机。悬浮球不读取 Codex 原始对话文件或凭证，不新增外部 AI 请求；评估由用户现有 Codex 模型完成。

`npm run desktop:build` 用于开发打包。尚无签名、正式验收的安装包；插件源码不代表已安装或官方上架，也不能单独提供系统悬浮窗。

## 文档与检查

[语义评估](docs/SEMANTIC-REVIEW.md) · [UI / UX](docs/UIUX.md) · [架构](docs/ARCHITECTURE.md) · [分发](docs/DISTRIBUTION.md) · [路线图](docs/ROADMAP.md) · [验证记录](docs/VERIFICATION.md)

```sh
npm run check
python3 plugins/codex-context-orb/scripts/test_hooks.py
python3 plugins/codex-context-orb/scripts/test_assessments.py
cargo test --manifest-path src-tauri/Cargo.toml --locked
npx playwright install chromium
npm run test:ui
```

Windows 使用 `py -3` 代替 `python3`。测试仅用虚构数据和临时目录。

始终保留未知、明确手动固定，不自动创建、压缩、中断或关闭会话。代码测试、运行时接入、规则准确率、平台分发与插件审核分别验收。

[MIT License](LICENSE).
