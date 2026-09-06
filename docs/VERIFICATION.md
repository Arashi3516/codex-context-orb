# 验证记录

版本：0.3.0 · 2026-09-06

验证实际文件检查、状态边界、存储通路与 UI，不宣称真实会话语义准确率或自动监控已完成。

## 本地结果

| 检查 | 结果 |
| --- | --- |
| TypeScript 报告规则、简报、渲染边界与会话合并 | 40/40 PASS |
| Python hook 隐私、并发与错误边界 | 15/15 PASS |
| Python 旧版评估兼容通路 | 15/15 PASS |
| Python v2 同字节检查、拒绝伪造结果、UTF-8、锁、历史与 CLI | 22/22 PASS |
| Python doctor 精确身份、独立分层、隐私、只读与 CLI | 13/13 PASS |
| Rust v1/v2/hook 有界读取、精确身份、契约、哈希与诊断 | 32/32 PASS |
| Playwright 状态、来源、作废、历史、复制、绑定、静音与键盘 | 10/10 PASS |
| TypeScript + Vite 生产构建 | PASS |
| macOS 本地开发包（debug .app，未签名公证验收） | PASS |
| Plugin / skill 官方脚本结构验证 | PASS |
| git diff --check | PASS |

本地累计 147 项测试通过，使用虚构数据和临时目录；本轮新增 15 项诊断检查，并重跑受影响的 Python/Rust 检查与生产构建。Python、Rust、TypeScript 共用一份合成 v2 收据；这验证了契约互通，不证明真实客户端接入。

独立演练按技能流程构造需求变更：旧条件文件检查 PASS → 新条件声明替代旧条件后 FAIL → 修改文件并重新采集后 PASS；无法用字面条件判断的交互要求始终 UNKNOWN。逐次读回核对会话与报告 ID，三份历史单独保留，其他会话读不到该报告。演练仅使用隔离的临时数据根。

复核修复了简报遗漏有效事实/决策/进展和假设、旧观察回流当前待办，以及总览未直接展示未核验前提的问题。已作废条目不会重新成为有效检查；失败/未知状态不会显示通过卡。

视觉检查：1440×1000 深浅主题、390×844 窄屏及 382×690 浏览器悬浮窗组件。检查无横向溢出、操作可达、长详情可滚动。[预览](assets/design-preview.png) · [深色](assets/design-preview-dark.png) · [来源](assets/semantic-evidence.png) · [账本](assets/task-ledger.png) · [悬浮窗组件](assets/orb-surface.png)。

## 本机运行时试点

2026-09-06 在 macOS Codex Desktop `26.901.41123 (7942)` 上，使用其内置 CLI `0.153.3` 执行安装和状态查询：

| 环节 | 实测结果 |
| --- | --- |
| 仓库 marketplace | 已注册本地源码根；包名和路径匹配 |
| 插件安装 | `codex-context-orb@personal`，`installed=true`、`enabled=true`；版本 `0.3.0+codex.20260906025931` |
| 安装内容 | doctor、收集器、技能和 hook 定义与源码逐字节一致 |
| 当前身份 | 从当前运行环境取得完整 `CODEX_THREAD_ID`；当前 turn ID 缺失，保存为 null |
| 真实采集 | 明确选择本仓库两个文件；2 个字面检查 PASS，3 个人工检查 UNKNOWN |
| 三端读取 | 收集器回读、已安装插件 doctor、原生程序的实际读取函数均返回相同的会话与报告 ID；历史包含同一收据 |
| 宿主 hook | 目标快照 ABSENT；安装结果不证明 trust 或自然 dispatch |
| 原生 IPC / 界面 | 未验收；诊断命令未启动或操作 WebView |

原生开发 `.app` 已重新构建。真实收据与运行绑定记录保留在本机 Orb 数据根，未提交任何会话 ID、需求摘要或本机验收文件。采集结果只对应当时选定的文件条件，不能证明整个上下文健康。精确 ID、实际数据根、工作区、源码版本与报告 ID 应一起核对，操作步骤见[接入说明](DISTRIBUTION.md#核对接入)。

## 边界

新版收据永远对应采集时点，不再使用 v0.2 的 20 分钟或后续 Stop 失效规则。之后的活动只提示复查。来源声明不是独立宿主取证，字面检查不是行为测试，报告哈希不是防篡改的真实性证明。

本机结果不替代跨平台执行；对应提交的 macOS/Windows 框架检查与 Linux 浏览器检查以 [GitHub Checks](https://github.com/Arashi3516/codex-context-orb/actions/workflows/checks.yml) 为准。Windows 分享冲突重试与 reparse 处理有测试，但不声明能抵抗恶意并发目录替换的完整沙箱保证。

Windows 首轮 CI 暴露了两处兼容问题：Git 自动换行改变字节夹具，以及 CPython 3.12 路径查询与句柄查询的 ctime 语义不同。现对共享夹具固定 LF；同 API 的读取前后仍完整比较，跨 API 只比较文件身份、大小与修改时间。新增反例在旧实现下复现失败，修复后通过，ctime-only 变化、内容修改和文件替换仍会被拒绝。依据：[CPython 路径查询](https://github.com/python/cpython/blob/v3.12.10/Modules/posixmodule.c#L2139-L2149)、[句柄查询](https://github.com/python/cpython/blob/v3.12.10/Python/fileutils.c#L1109-L1127)、[Windows 文件时间定义](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_basic_info)。最终平台结论仍以对应提交的 CI 为准。

尚未完成：客户端新任务技能加载、hook trust/自然 dispatch、原生 IPC 与显示完整链；自动语义诊断与 C/C+S/fresh+S 对照校准；后台模型复查、系统通知、前台自动跟随；Windows 真机、多显示器、签名公证、安装升级和官方目录审核。

原生 UI 自动化受本机 macOS 系统权限限制，浏览器组件 QA 不能替代原生真机验收。
