# 验证记录

版本：0.2.0 · 2026-09-06

验证规则、本地存储通路和交互，不宣称真实任务准确率或自动监控已完成。

## 本地结果

| 检查 | 结果 |
| --- | --- |
| TypeScript 语义规则与会话合并 | 31/31 PASS |
| Python hook 隐私、并发与错误边界 | 15/15 PASS |
| Python 评估契约、锁、原子写入、CLI 与 UTF-8 | 15/15 PASS |
| Rust 报告与快照的有界/精确读取 | 20/20 PASS |
| Playwright 语义状态、依据、澄清、过期、复制、绑定、静音、主题与键盘 | 8/8 PASS |
| TypeScript + Vite 生产构建 | PASS |
| Plugin / skill 官方脚本结构验证 | PASS |
| git diff --check | PASS |

本地共 89 项测试通过，全部使用虚构数据或临时目录。31 项规则测试不构成真实提醒准确率证明。

视觉检查：1440×1000 浅色/深色、390×844 窄屏，以及 382×690 浏览器悬浮窗组件。检查无横向溢出、主操作可达、详情可滚动；浏览器组件截图不证明系统窗口行为。[预览](assets/design-preview.png) · [深色](assets/design-preview-dark.png) · [依据](assets/semantic-evidence.png) · [悬浮窗组件](assets/orb-surface.png)。

上一版 Windows CI 在并发 os.replace 时出现 WinError 5。本版加入限定 Windows 错误码的 150ms 有界重试，保留失败清理，未放宽隐私或 JSON 完整性检查。跨平台结果以对应代码提交的 [GitHub Checks](https://github.com/Arashi3516/codex-context-orb/actions/workflows/checks.yml) 为准。

## 尚未完成

尚未完成：真实 Codex 安装/trust、身份与 skill 写入到原生显示完整链；自动后台评估、系统通知、前台跟随；真实标注任务误报/漏报；Windows 真机与多显示器；签名公证和安装升级；官方目录审核。

原生 UI 自动化受本机 macOS 系统权限限制，浏览器组件 QA 不能替代原生真机验收。

正常 Stop 也会使评估退出当前判断；用户可回顾历史依据。自动化前需要更稳定的输入/压缩失效水位，以改善该保守策略的可用性。
