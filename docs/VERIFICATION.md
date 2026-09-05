# 0.1.0 验证记录

日期：2026-09-06（Asia/Shanghai）。范围：框架、设计原型、插件开发包。所有浏览器场景使用虚构数据。

## 已验证

| 检查 | 结果 | 实际覆盖 |
| --- | --- | --- |
| `npm run check` | 8 单元测试通过；TypeScript 与 Vite 构建通过 | 阈值、未知/过期/异常数据、固定会话身份、交接模板 |
| `python3 plugins/codex-context-orb/scripts/test_hooks.py` | 13 测试通过 | 元数据白名单、字段校验、有限读取、无正文持久化 |
| `cargo test --manifest-path src-tauri/Cargo.toml telemetry::tests` | macOS 8 测试通过 | 文件身份、大小/数量边界、未知指标、Unicode、符号链接与目录错误 |
| `PLAYWRIGHT_CHANNEL=chrome npm run test:ui` | 5 流程通过 | 四种状态、A/B 固定、后台更新、会话隔离暂停、模板编辑/复制、拖动/右键、键盘/弹层焦点、主题/窄屏 |
| `npm run tauri -- build --debug --bundles app` | macOS Apple Silicon 编译及开发应用打包通过 | 生成本地 `Context Orb.app`，未签名分发、未公证 |
| 插件与 skill 结构验证 | 通过 | manifest 与 SKILL.md 通过创建工具的校验；不代表官方上架 |

浏览器验证在本机安装的 Chrome 中执行；CI 使用 Playwright Chromium。桌面 viewport 为 1440 × 1000，窄屏为 390 × 844。初始页、完整面板、深色交接状态分别检查，未发现横向溢出、按钮遮挡或未处理的页面异常。截图在动画结束后采集。

- [浅色预览](assets/design-preview.png)
- [深色交接状态](assets/dark-handoff.png)
- [窄屏首页](assets/mobile-preview.png)
- [窄屏完整面板](assets/mobile-panel.png)

独立复核修正了目录探测失败被误报为空数据的问题；会话名称现在保留 ID 首尾，完整 ID 可访问。原生拖动与按会话暂停的逻辑也经过审查。源码审查不能替代原生窗口操作验收。

## 尚未验证或实现

- macOS 独立窗口的 UI 操作验收受系统屏幕录制/辅助功能权限阻挡。已编译打包，但不宣称已验证透明度、置顶、拖动、恢复、剪贴板和多显示器行为。
- Windows 真机运行、安装卸载、DPI、休眠恢复及系统签名尚未验收。仓库提供对应 CI 与手动开发打包工作流；工作流存在本身不等于检查通过。
- 未在用户 Codex 中安装/信任 hooks，未修改其全局插件设置。端到端 hook 安装仍需实测。
- Hook 快照只有生命周期元数据。真实上下文用量、语义杂乱判断、自动跟随当前选中会话、系统通知均未接入。
- 有界目录扫描可能遗漏固定会话；缺失时显示未知。后续需要按固定 ID 直接读取、扫描覆盖提示、保留期限与清理策略。
- 官方 Plugin 目录提交、审核以及正式 Releases 安装包仍是后续交付目标。

本记录说明本地已取得的证据。远端提交和 CI 状态以 GitHub 对应提交及 Actions 结果为准。
