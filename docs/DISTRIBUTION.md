# 分发与安装路径

版本：0.3.0 · 2026-09-06

项目有两个独立交付物：桌面悬浮球应用与可选 Codex 插件。公开 GitHub 仓库让用户取得源码；插件目录让 Codex 发现技能和 hooks。安装插件本身不会自动安装或启动桌面应用。

公开仓库为 [Arashi3516/codex-context-orb](https://github.com/Arashi3516/codex-context-orb)。0.3.0 交付源码、任务账本与文件检查和插件开发包；尚无经过签名、验收的 Releases 安装包。

## 当前：从源码运行

从上述地址克隆后进入仓库根目录。使用 Node.js 22.12 或更新版本：

```sh
npm ci
npm run dev
```

开发预览仅监听 `127.0.0.1:1427`。浏览器中的演示场景不读取真实会话，不代表实际前台跟随或自动语义诊断已经可用。

本机桌面开发还需要 Rust 工具链与对应平台的 Tauri 构建依赖：

```sh
npm run desktop:dev
```

`npm run desktop:build` 是桌面构建入口。能在某台机器构建不代表生成了经过签名、公证或两个平台验收的安装包。0.3.0 的发布说明必须列出实际完成的平台与验证结果；没有产出的二进制不能出现在下载承诺中。

## 可选插件：仓库内的独立包

插件根目录：`plugins/codex-context-orb`。它有 `.codex-plugin/plugin.json`、聚焦的 `context-health` skill、`hooks/hooks.json` 和可运行 Python 脚本，没有占位 MCP 服务或虚构登录方式。Python 3.9+ 是首期 hook 的单独运行依赖；macOS 命令为 `python3`，Windows 命令为 `py -3`。

仓库已包含 `.agents/plugins/marketplace.json`，目录名为 `personal`。在克隆后的仓库根目录，用支持 `plugin` 命令的 Codex CLI 执行：

```sh
codex plugin marketplace add . --json
codex plugin add codex-context-orb@personal --json
codex plugin list --json --marketplace personal
```

第一条注册本地仓库，第二条安装并启用插件，第三条核对实际来源和版本。这些显式命令会更新本机 Codex 的插件配置；仅克隆仓库或运行网页预览不会安装插件。[官方插件打包与安装方式](https://developers.openai.com/plugins/build/plugins)

安装、启用和 hook trust 是独立状态。Codex 会跳过未信任的非托管 hooks；当前定义更新后可能再次需要其内置 review 流程。只有在目标客户端实际产生匹配快照后，才能报告“元数据接入成功”。不要将已安装状态当作已经采集成功。[官方 Hooks](https://learn.chatgpt.com/docs/hooks)

在客户端通过 `/hooks` 审阅此插件的六个异步命令。它们只调用本插件的 `emit_hook_event.py`，读取当前事件的允许字段并写入 Orb 自己的目录。按客户端显示的信任流程启用后，用新任务加载插件技能，再主动请求 `$context-health`。新任务是插件定义的加载边界，不是上下文质量或重启收益建议。

本地独立验证，无需安装插件：

```sh
python3 plugins/codex-context-orb/scripts/test_hooks.py
python3 plugins/codex-context-orb/scripts/test_assessments.py
python3 plugins/codex-context-orb/scripts/test_evidence_review.py
python3 plugins/codex-context-orb/scripts/test_doctor.py
python3 plugins/codex-context-orb/scripts/inspect_events.py
```

Windows 将 `python3` 替换成 `py -3`。适配器只写自己的数据根。默认 `~/.codex-context-orb`；需要隔离开发环境时，可给 hook 与桌面进程同时设置绝对路径的 `ORB_DATA_DIR`。用户如需移除本地快照，只删除已确认属于 Orb 的数据根；卸载插件无需清除或修改 Codex 会话。

## 核对接入

在已核实精确会话 ID 后运行：

```sh
python3 plugins/codex-context-orb/scripts/doctor.py --session-id verified-session-id
cargo run --manifest-path src-tauri/Cargo.toml --locked -- --inspect-session verified-session-id
```

Python doctor 只读该 ID 的三个快照，分别报告缺失、无效或可读结果；在当前 Codex 运行环境里可省略 ID，使用 `CODEX_THREAD_ID`。显式参数与环境不一致会标记为不匹配。原生诊断调用应用相同的 Rust 读取函数，并额外检查该 ID 的有界历史。两者输出实际数据根和收据 ID，不输出任务摘要或文件内容，不创建目录。退出码 0 只表示诊断完成，各层状态仍须分别检查。

两端的 `session_id`、数据根和 `report_id` 必须匹配。另行记录采集工作区、源码提交与客户端版本；v2 收据中的相对路径本身没有绑定工作区身份。诊断不执行模型、触发 hook 或操控界面，不能证明宿主 dispatch、原生 IPC 或实际显示。`inspect_events.py` 是旧版元数据工具，不能作为 v0.3 的整体自检。

最后在桌面手动固定完整 ID，确认来源为本地文件检查、采集时点和实际检查结果。没有自然产生的目标 hook 或尚未亲测界面时，相应环节继续保留未验证。

## 后续：GitHub Releases 安装包

目标分发矩阵：

| 平台 | 目标架构 | 计划形式 | 发布前必须完成 |
| --- | --- | --- | --- |
| Windows | x64 | 平台安装程序 | 实机运行、WebView2/运行依赖、安装卸载、更新、代码签名 |
| macOS | Apple Silicon | arm64 应用与磁盘镜像 | 实机运行、Developer ID 签名、公证、安装与更新 |
| macOS | Intel | x64 应用与磁盘镜像 | 独立构建与实机验收、签名和公证 |

这些是计划目标，不是 0.3.0 的下载清单。首个正式二进制版本应附可复核的源代码版本、校验和、已测 Codex 版本、已知限制和数据迁移说明。自动更新、自动启动、Python 捆绑或替代运行时都需要实现后才写入产品说明。

## 后续：官方插件目录

本仓库为非官方项目。拥有合法 manifest、公开源码或本地 marketplace 不等于已进入 OpenAI 官方目录；目录展示也不能说明桌面伴随程序已由 OpenAI 验证。

按当前官方流程，公开提交需要具备提交权限与经验证的开发者或企业身份，准备准确的功能、支持、隐私、条款和测试材料；提交后经过审核，再由开发者发布。可先评估 skills-only 路径，独立桌面 app 与本地 hook 的分发适配性仍需核实，不能承诺目录会替用户安装原生伴随程序。[官方提交与审核流程](https://developers.openai.com/plugins/deploy/submission)

本版本不发起官方上架申请。待功能和实际平台验证完成后，再以真实发布者身份、明确的数据行为和可重复的测试案例提交。审核资格、审核结果、签名证书和商店政策均不得由源码成功构建推定。
