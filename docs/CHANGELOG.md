# 项目进度 Changelog

本文件记录 AgentTidy 的开发阶段进度，帮助贡献者了解当前工作重点与
已知阻塞。它不是发布日志：面向用户的版本变更仍维护在仓库根目录的
[`CHANGELOG.md`](../CHANGELOG.md)。阶段定义以 [`Start.md`](../Start.md#19-开发顺序)
为准。

## 当前概览

| 阶段 | 状态 | 说明 |
| --- | --- | --- |
| Phase 0：Product & Provider Investigation | 已完成 | 三个首批 Provider 的 macOS / Windows 样本调研与脱敏 fixtures 已具备。 |
| Phase 1：Read-only Core | 已完成 | 只读领域模型、能力模型、快照和静态 Provider Registry 已实现。 |
| Phase 2：Infrastructure | 已完成 | 安全文件系统、只读 SQLite、JSONL、空间统计、进程信号与回收站封装已实现。 |
| Phase 3：三个 Read-only Provider | 已完成 | Codex、Claude Code（CLI/Desktop）和 WorkBuddy 均已具备安全降级的只读扫描。 |
| Phase 4：CLI Validation | 已完成（只读初版） | CLI 已提供版本化 JSON 契约、文本诊断与真实扫描；清理及候选预览保留给未来 GUI。 |
| Phase 5：GUI Read-only Experience | 未开始 | 待 Application API 和 CLI 验证路径稳定后接入。 |
| Phase 6：Cleanup Model | 未开始 | 继续保持只读；尚未开放清理模型或执行能力。 |
| Phase 7：首个 Provider Cleanup Beta | 未开始 | 依赖 Phase 6 的安全模型与跨平台验证。 |
| Phase 8：逐 Provider 扩展 | 未开始 | 按 Provider 分别通过安全门槛后推进。 |

## 已完成里程碑

### Phase 0：Product & Provider Investigation

- 完成 WorkBuddy、Claude Code 和 Codex 的真实样本调研文档，以及 Windows
  路径矩阵和 Claude Desktop 的补充调研。
- 添加按 Provider 划分的脱敏目录结构与会话 JSONL fixtures。
- 明确 v0.1 的首批范围为 macOS 与 Windows 上的三个 Provider；默认工作区
  只报告、不清理。

### Phase 1：Read-only Core

- 实现安装信息、Session、Resource、大小置信度、能力状态、扫描快照和
  静态 Provider Registry 的只读领域模型。
- 实现 `AgentProviderAdapter` 只读契约；清理相关接口有意延后至 Phase 6。
- 采用闭集 Provider 标识与缺失能力默认 Unsupported 的 fail-closed 规则。

### Phase 2：Infrastructure

- 实现路径边界与保留名称检查、禁止跟随链接的目录遍历、硬链接去重与
  分配空间统计。
- 实现逐行 JSONL 解析、只读 SQLite（含 WAL / 临时副本回退）、进程存在性
  信号和系统回收站封装。
- 平台特定实现限定在 infrastructure 层，符合架构边界。

## 当前工作与已知问题

### Phase 3：三个 Read-only Provider

- Codex 已接入 `AgentProviderAdapter`：发现共享的 CLI / Desktop 状态根，检查
  读取权限和运行信号，并从 rollout JSONL 的首条 `session_meta` 生成只读 Session
  与 transcript Resource；不会读取对话正文。
- Codex 以只读 `state_5.sqlite.threads` 映射补齐标题、时间、工作目录和归档
  状态；索引不可读、完整性检查失败或归档位置不一致时均记录诊断，冲突 Session
  降级为 Unknown，绝不写入数据库。
- 无法识别的 rollout 默认只报告问题，只有在 `include_unknown` 时才以 Unknown
  资源展示。下一步是补齐 Provider fixture / contract 覆盖，并以相同安全边界
  实现 WorkBuddy 与 Claude Desktop 安装。
- Claude Code CLI 已接入 `AgentProviderAdapter`：仅扫描已验证的
  `~/.claude/projects/<cwd-slug>/<session-id>.jsonl` 布局；主 transcript 与同名
  side directory 作为同一 Session 资源计量，嵌套 subagent 不会被独立解析或重复
  计数。解析仅保留会话元数据，不保留消息正文。
- Claude Code Windows Desktop 的 VM 安装、凭证与镜像副本尚未接入，维持不扫描、
  不触碰的边界，直至其镜像关系与 capability 规则获得单独的契约测试。
- WorkBuddy 已接入 `AgentProviderAdapter`：扫描已验证的 transcript、同名
  tool-results directory、`.meta.json` 和 `.file-rollback.ndjson` 文件集；数据库
  只用于健康与 journal mode 检查。因尚未冻结 `sessions` 表契约，WorkBuddy Session
  的生命周期保守标为 Unknown。
- Claude Code Desktop 在 Windows 上发现为 `claude-code:desktop`，仅扫描
  `local-agent-mode-sessions` 中嵌入的 `.claude/projects` transcript 镜像；不读取
  VM 镜像、凭证、审计日志或用户输出目录。

## 下一步

### Phase 4：CLI Validation（只读初版）

- 建立 Application API 的静态 Provider Registry，统一调度 Detect / Inspect / Scan。
- 将 CLI 的 `doctor`、`scan`、`sessions` 占位命令接到该 API：显示安装、能力、
  Session/Resource/空间汇总与诊断；`clean` 明确拒绝，保持只读。
- `doctor`、`scan` 与 `sessions` 均支持带
  `schema_version`、`command`、`mode: "read-only"` 的 JSON 输出契约
  `agenttidy.cli.v1`；文本输出则面向人工诊断。
- Application 层以无副作用的纯函数应用工作空间候选门槛，并以单元测试锁定：精确
  单 Session + 文件系统安全事实才能预览为 eligible；WorkBuddy 在自动化引用契约
  未验证前始终 blocked。该能力只供未来 GUI 的候选选择与确认流程使用，不暴露给 CLI。
- 已在开发机验证真实扫描结果。后续可按真实 provider fixtures 增加端到端 CLI
  golden 基线，但不阻塞这版只读 CLI 的交付。
- 默认工作空间与常见 AppData/缓存路径的只读统计将作为本阶段后续工作；它们与
  Session 占用分开显示，默认仅报告。
- Codex `Documents/Codex` 与 WorkBuddy `WorkBuddy` 默认工作空间已纳入扫描快照和
  CLI 汇总，作为 `Workspace / User / report-only` 资源独立统计；不会归入 Session
  或可回收空间。

### Phase 6：Workspace Cleanup Policy

- 已确定默认工作空间的未来清理门槛：精确单会话归属、无 Git、无其他 Session /
  自动化引用、无未知或变化项，并要求“计划预览确认 + 执行前确认”两次独立确认。
  详细安全规则见 `docs/safety/workspace-cleanup.md`；当前不开放清理。

### 验证前置问题

- macOS 无 GUI / Finder 服务时，回收站功能会以 `TrashError::Platform` 失败；
  单元测试会明确跳过原生回收站断言，生产调用仍保持 fail-closed。其余 Rust
  测试与 Clippy 在当前环境通过。
- 前端构建在当前受限会话未返回有效退出状态，尚需在标准 CI 或本地桌面环境
  确认 `pnpm build:desktop` 的结果。

## 维护约定

- 完成或范围变化时，更新对应 Phase 的状态、关键产出和“当前工作”内容。
- 仅当阶段验收条件已满足时标记为“已完成”；已知阻塞和跨平台差异必须保留。
- 发布面向用户的功能或修复时，同时更新根目录 `CHANGELOG.md`。
