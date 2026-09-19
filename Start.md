# AgentTidy v0.1 产品设计与技术方案（完善版）

> 文档状态：Draft for Implementation  
> 版本：v0.1  
> 核心原则：Local-first、Read-first、Trash-first、Fail-closed

## 1. 项目定位

### 1.1 产品名称

**AgentTidy**

Slogan：

> Keep your AI agents tidy.

产品定位：

> **Local-first AI Agent Session & Storage Manager**

AgentTidy 是一个开源、本地优先的 AI Agent 数据管理工具，用于发现、理解、浏览和安全整理用户电脑中由 AI Agent 产生的本地数据。

AgentTidy 不是传统磁盘垃圾清理器。它首先帮助用户回答：

1. 哪些 Agent 在本机保存了数据？
2. 数据保存在哪里、占用了多少空间？
3. 这些数据属于哪些 Session、项目和资源？
4. 哪些资源能够被可靠识别？
5. 哪些资源可以低风险处理，哪些必须人工检查，哪些禁止处理？

AgentTidy 的首要价值不是“多删文件”，而是：

> 让用户看清 AI Agent 在本机留下的数据，并在可验证、可解释、可恢复的前提下处理已经完成生命周期的数据。

### 1.2 首期目标用户

- 高频使用多个 AI 编程 Agent 的开发者
- 本机积累大量会话、日志、缓存和工作区数据的用户
- 关注会话隐私、磁盘占用和数据生命周期的高级用户
- 需要排查不同 Agent 数据位置和空间占用的工具开发者

### 1.3 v0.1 要验证的核心假设

1. 用户确实无法方便地了解多个 Agent 的本地数据占用。
2. Session、Agent、项目三个维度比普通目录视图更有价值。
3. 用户愿意使用本地开源工具查看 Agent 数据。
4. 至少存在一类可明确识别、可独立处理、能释放显著空间的资源。
5. 透明的风险说明和可恢复操作能够建立足够的删除信任。

v0.1 不以清理量最大为成功标准，而以“识别准确、解释清楚、执行安全”为成功标准。

---

## 2. 产品边界与隐私承诺

### 2.1 Local-first

v0.1：

- 不要求账号
- 不依赖服务器
- 不上传 Session、路径或扫描结果
- 不提供云同步
- 所有分析默认在本机完成
- 默认仅读取结构化元数据和文件系统信息

如果某个 Provider 只能通过读取会话正文推断标题或项目，AgentTidy 必须：

- 默认不启用正文解析；或
- 在产品中明确说明并取得用户授权；
- 即使解析，也只在本机进行，不持久化正文副本。

因此，“不分析内容”的准确表述是：

> AgentTidy 默认不读取或解析会话正文；需要内容推断的增强能力必须单独授权，且内容永不上传。

### 2.2 默认只读

新发现的 Provider、未知版本、未知 Schema、权限异常或无法确认所有权的数据，默认进入只读模式。

只有同时满足以下条件，Provider 才能开放清理能力：

- 数据结构已经识别
- 资源边界已经验证
- 清理操作具有明确语义
- 执行前置条件可检查
- 已有真实脱敏 Fixture 和集成测试
- 清理后经过对应 Agent 的兼容性验证

### 2.3 不触碰用户项目源码

AgentTidy 默认不清理：

- 用户 Git 仓库中的源代码
- Agent 生成但已位于用户项目中的文件
- 无法确认由 Agent 独占管理的文件
- 指向 Agent 数据目录之外的符号链接目标

Generated Files 在 v0.1 只展示，不进入默认 CleanupPlan。

---

## 3. 产品原则

### 3.1 Understand Before Tidy

```text
Discover → Scan → Normalize → Analyze → Review → Validate → Tidy
```

禁止：

```text
Scan → Delete
```

### 3.2 Never Act on What We Do Not Understand

无法明确识别、无法确定所有权、无法验证依赖关系或不支持当前版本的数据：

```text
Unknown → Blocked
```

### 3.3 Archive Is Intent, Not Permission

Archive 是强生命周期信号，但不是删除授权。

```text
Archived
+ Inactive
+ Known Cleanup Unit
+ Exclusive Ownership
+ Preconditions Valid
→ Low-risk Candidate
```

任何一项不成立，都不能标记为低风险。

### 3.4 Trash First

文件型清理默认移动到系统废纸篓。v0.1 不提供自动永久删除。

数据库记录不能伪装成“移动到废纸篓”。如果无法通过可靠方式备份和恢复，v0.1 不修改第三方数据库。

### 3.5 Fail Closed

发生下列情况时自动停止或跳过，而不是继续猜测：

- Schema 不匹配
- 文件或数据库在扫描后发生变化
- Agent 正在写入相关数据
- 权限不足
- 资源依赖不完整
- 大小或指纹校验失败
- Provider 版本未知

### 3.6 Explain Every Recommendation

每个清理候选必须展示：

- 将处理什么
- 为什么被推荐
- 预计可释放空间
- 风险等级
- 数据所属 Agent 和项目
- 操作方式
- 是否可恢复
- 哪些前置条件将在执行时重新验证

---

## 4. v0.1 范围

### 4.1 支持对象

P0 Provider：

- WorkBuddy
- Claude Code
- Codex

“支持”按能力分别声明，不承诺三个 Provider 同时具有写入能力。

例如：

| Provider | Detect | Storage | Sessions | Lifecycle | Cleanup |
|---|---:|---:|---:|---:|---:|
| WorkBuddy | 支持 | 支持 | 支持 | 视研究结果 | Beta |
| Claude Code | 支持 | 支持 | 支持 | Inactive | 只读或有限清理 |
| Codex | 支持 | 支持 | 支持 | 视可用元数据 | 只读或有限清理 |

最终能力矩阵必须由 Phase 0 调研结果决定，不能提前假定。

### 4.2 平台边界

v0.1 首发正式支持：

- **macOS**
- **Windows**

Linux 不属于 v0.1 正式支持范围。最低系统版本由 Phase 0 根据 Tauri、系统回收站 API 和真实用户分布确定，并写入发布支持矩阵。

macOS 和 Windows 必须使用同一套领域模型、CleanupPlan 和安全规则，但允许基础设施层分别实现：

| 能力 | macOS | Windows |
|---|---|---|
| 数据路径 | Home/Application Support 等 | `%APPDATA%`、`%LOCALAPPDATA%`、用户目录等 |
| 回收站 | 系统 Trash API | Windows Recycle Bin / Shell API |
| 路径规则 | POSIX、默认可能大小写不敏感 | Drive/UNC、大小写不敏感、长路径、保留名 |
| 链接边界 | Symlink | Symlink、Junction、Reparse Point |
| 文件占用 | 可删除已打开文件的 Unix 语义 | 被占用文件通常无法移动或删除 |
| 进程检测 | macOS 实现 | Windows 实现 |

平台差异只能存在于 Infrastructure 和 Provider 路径发现逻辑中，不能渗透到 Cleanup Policy 或 React 页面。

首发不要求两个平台支持完全相同的可写能力。某个 Provider 可以在 macOS 开放 Cleanup、在 Windows 暂时 Read-only，但 UI 必须准确展示平台能力差异，不能把“可扫描”宣传为“可安全清理”。

### 4.3 v0.1 功能

- 自动发现已使用的 Agent 和安装实例
- 展示每个 Agent 的数据位置和空间占用
- 统一浏览可可靠识别的 Session
- 按 Agent、项目、生命周期、时间、大小筛选
- 展示资源构成和空间口径
- 生成 CleanupPlan
- 执行前重新验证计划
- 对白名单 Cleanup Unit 执行 Move to Trash
- 展示执行结果、跳过原因和实际释放空间
- 导出不包含会话正文的诊断报告

### 4.4 v0.1 明确不做

- 云同步、账号系统、遥测默认上传
- 自动永久删除
- 定时自动清理
- 修改未经验证的第三方 SQLite 数据库
- 清理用户项目源代码和生成文件
- Session Resume
- Universal Agent Memory
- 动态插件加载与插件市场
- 跨设备同步
- 复杂内容分析和 AI 摘要
- “一键全选所有可疑数据”

---

## 5. 分阶段交付策略

### 5.1 v0.1-alpha：三 Provider 只读分析

- Detect
- Storage
- Session List
- Lifecycle Detection
- Resource Inventory
- Diagnostic Report
- 不开放 Session 删除
- 只允许清理经过白名单验证的独立日志、缓存或临时目录

### 5.2 v0.1-beta：一个 Provider 的完整闭环

选择数据结构最清晰、用户价值最高的 Provider，实现：

- Cleanup Unit 识别
- Dependency/Ownership 验证
- CleanupPlan
- Plan Revalidation
- Trash Execution
- Recovery Test
- Agent Compatibility Test

另外两个 Provider 保持只读。

### 5.3 v0.1：逐 Provider 开放清理能力

Provider 通过安全门槛后单独开放 Cleanup。不存在“为了同时支持三个 Provider 而降低安全标准”。

---

## 6. 信息架构与主要页面

### 6.1 Overview

展示：

- 已检测 Agent
- 数据总逻辑大小
- 实际占用空间（可获得时）
- 已确认可回收空间
- 待人工检查空间
- 扫描时间和扫描完整性
- Provider 当前能力状态

“Potentially Reclaimable”必须拆分为：

```text
Confirmed reclaimable
Review required
Unknown / blocked
```

### 6.2 Sessions

统一字段：

- Agent
- Session 标题或稳定标识
- Project
- Lifecycle
- Last Activity
- Exclusive Size
- Shared Size
- Capability Status

Session Detail 显示关联资源，但 v0.1 不展示完整聊天内容。

### 6.3 Review & Tidy

每个 Cleanup Item 展示：

- Cleanup Unit 名称
- 包含的资源
- 推荐理由
- 风险等级
- 预计独占可回收空间
- 操作方式
- 是否可恢复
- 扫描后是否发生变化

默认选择规则：

- `low-risk` 可以默认选中，但必须允许用户取消
- `review-required` 默认不选中
- `blocked` 不可选择

### 6.4 Settings / Diagnostics

v0.1 可采用轻量抽屉而非独立主页面：

- Provider 数据路径
- 排除目录
- 内容解析授权
- 扫描错误
- Provider 版本与 Schema 识别结果
- 导出诊断报告

---

## 7. 风险模型

避免使用绝对的 `safe`。

```ts
type RiskLevel =
  | 'low-risk'
  | 'review-required'
  | 'blocked'
```

### 7.1 Low Risk

必须同时满足：

- 已知资源类型
- 已知 Provider 版本和数据结构
- 已形成原子 Cleanup Unit
- 资源为 Agent 独占或可证明独立
- 无未知依赖
- 当前不活跃
- 执行动作可恢复或 Provider 提供可靠恢复语义
- 执行前置条件可重新校验

### 7.2 Review Required

例如：

- 长期未使用但没有 Archive 语义的 CLI Session
- 项目目录已不存在，但 Session 仍完整
- 可识别但价值无法由系统判断的数据
- 可清理但恢复成本较高的数据

此类资源必须由用户逐项或按明确分组主动选择。

### 7.3 Blocked

例如：

- Active/Recent Session
- Unknown Resource
- Shared Ownership
- Unsupported Schema
- Agent 正在写入
- 扫描后已变化
- 位于用户项目目录
- 符号链接越界
- 数据库修改没有可靠事务和恢复方案

Blocked 资源不得进入可执行计划。

---

## 8. 空间计算模型

“占用空间”和“可释放空间”不是同一概念。

```ts
interface SizeInfo {
  logicalBytes: number
  allocatedBytes?: number
  exclusiveBytes?: number
  sharedBytes?: number
  reclaimableBytes?: number
  confidence: 'exact' | 'estimated' | 'unknown'
}
```

定义：

- `logicalBytes`：文件逻辑长度总和
- `allocatedBytes`：实际磁盘块占用，可获得时展示
- `exclusiveBytes`：仅属于当前资源或 Cleanup Unit 的空间
- `sharedBytes`：被多个资源引用或无法独占归属的空间
- `reclaimableBytes`：执行该计划后预计实际可释放的空间

规则：

- 嵌套目录只能由一个统计层级计入总量
- 硬链接按 inode 去重
- 符号链接只统计链接本身，不跟随到外部目标
- Shared Size 不计入单个 Session 的可回收空间
- 不能精确计算时必须标记 Estimated，不展示伪精确数值
- 执行完成后重新测量，区分 Estimated 和 Recovered

---

## 9. 核心领域模型

### 9.1 Agent Installation

```ts
interface AgentInstallation {
  id: string
  provider: ProviderId
  platform: 'macos' | 'windows'
  version?: string
  dataRoots: string[]
  status: 'available' | 'permission-required' | 'unsupported'
}
```

### 9.2 Capability

Capability 不能只用 boolean：

```ts
type CapabilityStatus =
  | 'supported'
  | 'read-only'
  | 'unsupported'
  | 'degraded'
  | 'permission-required'

interface AgentCapabilities {
  sessions: CapabilityStatus
  projects: CapabilityStatus
  archive: CapabilityStatus
  restoreArchive: CapabilityStatus
  cache: CapabilityStatus
  logs: CapabilityStatus
  cleanup: CapabilityStatus
}
```

### 9.3 Session

```ts
interface Session {
  id: string
  provider: ProviderId
  installationId: string
  title?: string
  project?: ProjectRef
  createdAt?: Date
  updatedAt?: Date
  lifecycle: SessionLifecycle
  size: SizeInfo
  resourceRefs: ResourceRef[]
  metadata: Record<string, unknown>
}

type SessionLifecycle =
  | { state: 'active' }
  | { state: 'archived'; archivedAt?: Date }
  | { state: 'inactive' }
  | { state: 'unknown' }
```

没有 Archive 概念的 Agent 不能使用 `unarchived`。

### 9.4 Resource

```ts
type ResourceType =
  | 'session'
  | 'workspace'
  | 'project-metadata'
  | 'subagent'
  | 'generated-file'
  | 'cache'
  | 'log'
  | 'checkpoint'
  | 'database'
  | 'unknown'

type Ownership = 'exclusive' | 'shared' | 'unknown'

interface Resource {
  id: string
  provider: ProviderId
  installationId: string
  type: ResourceType
  locator: ResourceLocator
  ownership: Ownership
  managedBy: 'agent' | 'agenttidy' | 'user'
  size: SizeInfo
  project?: ProjectRef
  createdAt?: Date
  updatedAt?: Date
  dependencies: ResourceRef[]
  metadata: Record<string, unknown>
}
```

### 9.5 Cleanup Unit

CleanupPlan 不直接处理任意 Resource，而只处理经过 Provider 证明可作为一个整体操作的 Cleanup Unit。

```ts
interface CleanupUnit {
  id: string
  provider: ProviderId
  installationId: string
  kind: 'file-tree' | 'file-set' | 'provider-operation'
  resources: ResourceRef[]
  ownership: 'exclusive'
  estimatedReclaimableBytes: number
  fingerprint: ResourceFingerprint
  preconditions: CleanupPrecondition[]
  operation: CleanupOperation
}
```

共享或所有权未知的资源不能形成可执行 Cleanup Unit。

---

## 10. Provider Adapter

Provider 负责理解特定 Agent 的数据结构，包括“如何正确执行本 Provider 的操作”；但不负责决定用户是否应该清理。

```ts
interface AgentProviderAdapter {
  readonly id: ProviderId
  readonly displayName: string

  detect(): Promise<AgentInstallation[]>

  inspect(
    installation: AgentInstallation
  ): Promise<ProviderInspection>

  capabilities(
    inspection: ProviderInspection
  ): Promise<AgentCapabilities>

  scan(
    installation: AgentInstallation,
    options: ScanOptions
  ): Promise<AgentSnapshot>

  buildCleanupUnits(
    snapshot: AgentSnapshot
  ): Promise<CleanupUnit[]>

  validateCleanupUnit(
    unit: CleanupUnit
  ): Promise<ValidationResult>
}
```

职责边界：

```text
Provider
→ 描述事实、资源关系、操作方法和前置条件

Cleanup Policy
→ 判断风险、推荐与否、解释原因

Cleanup Executor
→ 重新验证并执行计划
```

Provider 不应包含“超过 30 天就推荐删除”这类产品策略；但必须包含“删除这个资源需要整体移动哪些文件”这类结构知识。

### 10.1 Provider Inspection

扫描前先识别：

- Agent 版本
- 数据根目录
- Schema 版本
- 数据库 journal 模式
- 权限
- 是否正在运行
- 是否存在未知结构

如果 Inspection 不通过，能力自动降级为只读或 unsupported。

---

## 11. Cleanup Policy

```ts
interface CleanupPolicy {
  evaluate(
    unit: CleanupUnit,
    context: CleanupContext
  ): CleanupDecision
}

interface CleanupDecision {
  risk: RiskLevel
  recommended: boolean
  reasons: CleanupReason[]
  blockers: CleanupBlocker[]
}
```

### 11.1 Archive-aware Policy

Archive 是加权信号，不是单独授权。

示例：

```text
Archived + inactive > 30d + exclusive + recoverable
→ Low Risk

Archived + recent
→ Review Required

Archived + shared/unknown dependencies
→ Blocked

Active
→ Blocked
```

### 11.2 Retention Policy

没有 Archive 的 Session：

```text
Inactive > 90d
→ Review Required

Inactive > 90d + missing project
→ 仍然是 Review Required

Recent / Active / Unknown
→ Blocked
```

v0.1 不应仅因时间久远把 CLI Session 判定为低风险。

### 11.3 Resource-specific Policy

日志、缓存、Checkpoint 不能用同一规则判断。Policy 应按资源语义组合，但保持 Provider 无关；如果某个语义只适用于一个 Provider，应通过 Provider 提供的标准化属性表达。

---

## 12. CleanupPlan 与执行协议

```ts
interface CleanupPlan {
  id: string
  scanId: string
  createdAt: Date
  expiresAt?: Date
  items: CleanupItem[]
  estimatedReclaimableBytes: number
  riskSummary: RiskSummary
}

interface CleanupItem {
  unit: CleanupUnitRef
  fingerprint: ResourceFingerprint
  action: 'trash' | 'provider-operation'
  risk: RiskLevel
  reasons: CleanupReason[]
  preconditions: CleanupPrecondition[]
}
```

### 12.1 执行流程

```text
Load Plan
→ Validate plan version
→ Re-inspect Provider
→ Revalidate each Cleanup Unit
→ Acquire operation guard
→ Execute item
→ Verify result
→ Record item result
→ Recalculate recovered space
```

### 12.2 计划失效

以下变化导致 Item 自动跳过：

- 文件路径、inode、大小、mtime 或内容指纹变化
- Session 重新活跃
- Agent 开始运行且操作不允许并发
- Schema 或 Agent 版本变化
- 资源依赖变化
- Cleanup Unit 不再独占

UI 必须显示“计划已过期，请重新扫描”，不能静默执行。

### 12.3 执行幂等性

- 已移动的资源再次执行时不得报成新的成功
- 部分失败不影响结果报告的准确性
- 每个 Item 独立记录 `removed/skipped/failed`
- 崩溃恢复后能够识别已完成项
- v0.1 不承诺跨多个第三方数据源的全局事务

### 12.4 数据库资源

数据库清理只有两种允许方式：

1. Provider 官方或稳定接口；
2. 经版本识别、事务、备份、完整性检查和恢复测试验证的 Provider Operation。

否则：

```text
Database-owned Session → Read-only / Blocked
```

不能把数据库行级删除包装成 Trash 操作。

---

## 13. 总体技术架构

```text
┌─────────────────────────────┐
│ Presentation                │
│ Desktop GUI / CLI           │
└──────────────┬──────────────┘
               │
┌──────────────▼──────────────┐
│ Application API             │
│ Detect / Scan / List        │
│ Plan / Validate / Execute   │
└──────────────┬──────────────┘
               │
┌──────────────▼──────────────┐
│ Core                        │
│ Models / Policy / Planning  │
│ Risk / Size Accounting      │
└──────────────┬──────────────┘
               │
     ┌─────────┴─────────┐
     │                   │
┌────▼─────────┐  ┌──────▼────────┐
│ Providers    │  │ Infrastructure │
│ WB/Claude/   │  │ FS/SQLite/     │
│ Codex        │  │ Trash/Process  │
└──────────────┘  └───────────────┘
```

v0.1 不需要同时维护语义重叠的 Public API Layer 和 Application Layer。Application API 是 GUI 与 CLI 共用的稳定入口。

### 13.1 技术栈

Desktop：

- Tauri 2
- React
- TypeScript
- Tailwind CSS
- shadcn/ui

Core：

- Rust
- serde
- SQLite 只读访问优先
- 平台原生 Trash 实现或经过验证的库

Rust 负责：

- 文件系统访问
- 安全路径规范化
- SQLite 只读读取
- 空间计算
- Provider 扫描
- CleanupPlan 验证和执行
- 操作日志

平台基础设施必须通过统一接口暴露，并使用 macOS/Windows 分离实现：

```ts
interface PlatformServices {
  paths: PlatformPathService
  trash: PlatformTrashService
  processes: PlatformProcessService
  filesystem: PlatformFileSystem
}
```

业务层不得通过字符串判断操作系统后自行拼接路径或调用系统命令。

前端负责：

- 数据展示
- 搜索和筛选
- 风险说明
- 用户选择与确认
- 进度和结果展示

---

## 14. 推荐仓库结构

```text
agenttidy/
├── apps/
│   ├── desktop/
│   └── cli/
├── crates/
│   ├── core/
│   ├── application/
│   ├── infrastructure/
│   ├── provider-api/
│   └── test-support/
├── providers/
│   ├── workbuddy/
│   ├── claude-code/
│   └── codex/
├── fixtures/
├── docs/
│   ├── providers/
│   ├── safety/
│   └── adr/
├── README.md
└── LICENSE
```

静态 Provider Registry 即可，不实现动态插件系统。

---

## 15. CLI v0.1

```bash
agenttidy doctor
```

显示 Provider、路径、版本、权限、Schema 和能力状态。

```bash
agenttidy scan
```

扫描并显示空间摘要。

```bash
agenttidy sessions
```

列出可识别 Session。

```bash
agenttidy clean --dry-run
```

生成但不执行 CleanupPlan。

```bash
agenttidy clean --plan <plan-id>
```

交互确认、重新验证并执行指定计划。

CLI 是核心验证和诊断界面，不是独立实现另一套业务逻辑。

---

## 16. 安全要求

### 16.1 路径安全

- 所有路径在执行前 canonicalize
- 不允许 Cleanup Unit 越出 Provider 数据根目录
- 不跟随指向数据根目录之外的 Symlink、Junction 或 Reparse Point
- 拒绝空路径、根目录、用户主目录和过宽目录
- 删除目标必须来自扫描快照，不能直接接受任意用户输入路径
- Windows 路径比较必须处理盘符、大小写、UNC、`\\?\` 长路径前缀和保留设备名
- macOS 路径比较必须考虑卷的大小写敏感性和路径规范化
- 不能依赖字符串前缀判断目录包含关系，必须使用规范化后的路径组件和文件身份信息

### 16.2 活跃数据保护

进程检测只是一个信号，还必须结合：

- 最近写入时间
- 文件锁或数据库状态
- 扫描前后指纹
- Provider 特定活跃标志

无法确认时 Blocked。

Windows 上若文件被其他进程占用，执行器必须将该 Item 标记为 `skipped`，不得使用强制终止进程、延迟到重启时删除或绕过文件锁的方式继续执行。

### 16.3 操作记录

本地记录：

- Plan ID
- 操作时间
- Provider
- Cleanup Unit ID
- 原路径和 Trash 结果标识
- Estimated/Reclaimed Bytes
- Skipped/Failed 原因

日志不得包含会话正文、敏感 Token 或无必要的完整内容。

### 16.4 恢复

文件型资源依赖系统废纸篓恢复。产品必须明确：

- 哪些操作可恢复
- 恢复由系统完成还是 AgentTidy 完成
- 恢复后 Agent 是否需要重启或重新扫描

v0.1 若不能可靠实现应用内恢复，不应展示虚假的 Undo 按钮。

---

## 17. Provider 调研要求

每个 Provider 必须输出：

```text
docs/providers/<provider>.md
```

至少包含：

- 安装与数据位置
- 版本发现方式
- Session 标识与存储结构
- Archive/Lifecycle 语义
- Project Mapping
- 资源依赖图
- 缓存、日志、Checkpoint
- 数据库与 Schema
- 运行时写入行为
- 可清理原子单元
- 已知风险
- 能力降级规则
- 验证过的版本矩阵

调研结果必须来自真实样本，不依赖猜测或单一开发者机器。

---

## 18. 测试策略

### 18.1 Fixture Tests

每个 Provider 保存脱敏 Fixture：

- JSON / JSONL
- SQLite
- 目录树
- 嵌套资源
- 符号链接
- 损坏或未知版本样本

### 18.2 Contract Tests

所有 Provider 必须通过统一契约：

- 稳定 ID
- 路径不越界
- Unknown 默认 Blocked
- Shared 资源不形成 Cleanup Unit
- Session 与 Resource 引用完整
- 大小统计不重复

### 18.3 Safety Tests

- 扫描后文件变化
- Agent 执行中启动
- 权限突然丢失
- 磁盘空间不足
- Trash 部分失败
- 进程崩溃和重启
- 重复执行同一 Plan
- Symlink 攻击
- Windows Junction/Reparse Point 越界
- Windows UNC、长路径、保留名和大小写路径碰撞
- Windows 文件被 Agent 或杀毒软件占用
- macOS 大小写敏感与不敏感卷
- 数据库锁定和 Schema 改变

### 18.4 Compatibility Tests

清理前后都要启动对应 Agent 并验证：

- Agent 正常启动
- 未选择的 Session 仍可见
- 现存 Session 可正常打开
- 数据库完整性检查通过
- 被清理资源不会留下破损索引

上述兼容性测试必须分别在 macOS 和 Windows 原生环境运行。不能以 Wine、容器或仅路径模拟替代 Windows 文件系统与回收站测试。

### 18.5 Golden Tests

对 Snapshot、Session 列表、CleanupPlan 和诊断报告使用 Golden Files，及时发现 Provider 解析行为变化。

---

## 19. 开发顺序

### Phase 0：Product & Provider Investigation

- 访谈 10–20 位目标用户
- 收集脱敏空间分布和数据结构样本
- 明确用户主要痛点和实际可回收空间
- 完成三个 Provider 调研文档
- 选择 v0.1 正式支持平台
- 确定 macOS 与 Windows 的最低支持版本和 Provider 路径矩阵
- 选择首个开放清理能力的 Provider

退出条件：数据结构、风险和用户价值足够明确。

### Phase 1：Read-only Core

- AgentInstallation
- Session
- Resource
- CapabilityStatus
- SizeInfo
- Snapshot
- Provider Registry

### Phase 2：Infrastructure

- Safe Filesystem
- Read-only SQLite
- JSON/JSONL
- Disk Usage
- Process Signals
- Trash

### Phase 3：三个 Read-only Provider

- Detect
- Inspect
- Scan
- Sessions
- Resources
- Diagnostics

### Phase 4：CLI Validation

- doctor
- scan
- sessions
- Golden/Fixture Test

### Phase 5：GUI Read-only Experience

- Overview
- Sessions
- Diagnostics

### Phase 6：Cleanup Model

- Ownership/Dependency
- CleanupUnit
- CleanupPolicy
- CleanupPlan
- Revalidation
- Execution Log

### Phase 7：首个 Provider Cleanup Beta

- 只开放通过测试的资源类型
- 完成 Trash/Recovery/Compatibility Tests
- 小范围真实用户 Beta

### Phase 8：逐 Provider 扩展

每个 Provider 独立通过安全门槛后开放 Cleanup。

---

## 20. v0.1 Done Definition

### 产品

- 用户能看见三个 Provider 是否存在、位于哪里、占用多少空间
- 用户能理解空间统计口径和置信度
- 用户能按 Agent、项目、生命周期、时间和大小浏览 Session
- 每个清理建议都有可解释理由和风险等级
- 不读取正文时产品承诺与实际行为一致
- macOS 和 Windows 用户都能完成 Detect、Scan、Sessions 和 Diagnostics 主流程

### Provider

- 三个 Provider 均完成真实样本验证的只读扫描
- 未知版本自动降级，不尝试猜测解析
- 至少一个 Provider 完成端到端 Cleanup Beta
- 能力矩阵准确反映 read-only/degraded/unsupported 状态
- 每个 Provider 分别声明 macOS 与 Windows 的能力，不以一个平台的验证结果推导另一个平台

### Cleanup

- 只对 Exclusive Cleanup Unit 执行操作
- CleanupPlan 在执行前重新验证
- 资源变化、Agent 活跃或 Schema 变化时安全跳过
- 默认只使用 Trash 或经过验证的 Provider Operation
- 不修改未经验证的数据库
- 不触碰用户项目源码
- 至少一个 Provider 在 macOS 和 Windows 上均完成端到端 Cleanup Beta；若因 Provider 自身平台限制无法满足，必须在发布范围中明确降级，不得静默缺失

### 空间

- 嵌套目录和硬链接不会重复计入
- Shared Size 不计入独占可回收空间
- 估算值明确标记
- 执行后展示实际 Recovered Space

### 工程质量

- Provider Fixture、Contract、Safety 和 Compatibility Tests 通过
- GUI 和 CLI 共用同一 Application API
- 无 Provider 特例泄漏到 React 页面
- 无 Cleanup 策略硬编码进 Provider
- 操作日志不包含正文和凭证
- macOS 与 Windows CI/原生集成测试均通过

---

## 21. 成功指标

v0.1 不建议只看“清理了多少 GB”。建议同时观察：

- Detection 成功率
- Session 识别准确率
- Unknown/Blocked 比例
- 空间估算与实际释放误差
- CleanupPlan 执行成功率
- 因计划失效而安全跳过的比例
- 清理后 Agent 兼容性故障数
- 用户从扫描到 Review 的转化率
- 用户是否能正确理解风险等级
- 30 天内再次打开产品的原因：查找 Session 或清理空间

安全底线指标：

```text
已知误删用户项目文件 = 0
已知破坏第三方 Agent 数据库 = 0
Blocked 资源被执行 = 0
```

---

## 22. 架构红线

1. UI 不直接访问 Agent 文件或数据库。
2. Provider 不决定“多久以后应该删除”。
3. Policy 不猜测 Provider 的物理数据结构。
4. 任意 Resource 不能直接进入执行器，必须先形成 Cleanup Unit。
5. Archive 不能单独推导为低风险。
6. Unknown、Shared、Unsupported 默认 Blocked。
7. 扫描结果不能不经校验直接执行。
8. 不把数据库行级删除描述为 Move to Trash。
9. 不为了首发支持数量降低 Provider 安全门槛。
10. 不提前建设动态插件系统、云端或复杂事件架构。
11. 不在业务层散落 macOS/Windows 条件分支，平台差异必须收敛在 Infrastructure 和 Provider 路径发现层。

---

## 23. 一句话验收标准

> macOS 和 Windows 用户可以在一个本地 GUI 中可靠地看见 WorkBuddy、Claude Code 和 Codex 的本机数据与 Session；理解哪些空间能够被准确归属；审查由系统解释清楚的 CleanupPlan；并只对经过当前平台验证、独占、未发生变化且可恢复的 Cleanup Unit 执行安全整理。

这就是 AgentTidy v0.1。
