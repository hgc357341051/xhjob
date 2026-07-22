# Rust 扩展全量代码审查 Spec

## Why

xhjob Rust 扩展（ext-php-rs 0.15）经多轮迭代已积累 ~12000 行代码（27 个 `.rs` 文件），覆盖 daemon、executor、ipc、pool、scheduler、store、task 等模块。在功能持续扩张（两种池模式、chord、chain、group、持久化、多服务、限流等）之后，需要一次系统性的全量代码审查，沉淀优点清单、识别缺点与风险、并列出"值得保留的好功能"与"需要完善的短板"，为后续优化与对外发布提供依据。

## What Changes

本次为**只读审查 + 报告产出**，不修改任何 Rust 源码：

- 按 7 大维度审查所有 Rust 源文件：架构设计、并发模型、错误处理、资源管理、安全性、可观测性、API 设计
- 产出审查报告 `/workspace/AUDIT_REPORT.md`，包含：
  - **优点清单**（值得保留的好功能与设计决策）
  - **缺点清单**（代码异味、潜在 bug、风险点）
  - **需要完善的功能**（未实现、stub、TODO、改进建议）
  - **按模块的审查结论**（daemon / executor / ipc / pool / scheduler / store / task / lib）
  - **优先级排序的改进建议**（P0/P1/P2）
- 审查结论以**代码位置引用**（文件:行号）形式呈现，便于追溯

## Impact

- Affected specs: 无（纯审查任务，不修改任何现有 spec 的实现）
- Affected code: 只读审查 `/workspace/src/**/*.rs`（27 文件，~12000 行），产出 `/workspace/AUDIT_REPORT.md`
- 不修改：Rust 源码、PHP 集成包、测试脚本、配置文件、已编译的 `.so`

## ADDED Requirements

### Requirement: 全量代码审查覆盖

审查 SHALL 覆盖 `/workspace/src/` 下全部 27 个 Rust 源文件，按以下 7 大维度逐项评估：

1. **架构设计**：模块边界、职责划分、依赖方向、抽象层次
2. **并发模型**：async/thread 两种池的正确性、锁竞争、死锁风险、Send/Sync 约束
3. **错误处理**：Result/Option 使用、错误传播链、panic 安全、unwrap/expect 使用
4. **资源管理**：文件句柄、socket、子进程、SQLite 连接的释放与泄漏
5. **安全性**：命令注入、路径遍历、反序列化、权限、credential 泄漏
6. **可观测性**：tracing 使用、事件流、指标、日志级别合理性
7. **API 设计**：PHP 扩展函数签名、JSON 契约、向后兼容、错误字符串约定

#### Scenario: 审查报告完整覆盖所有模块

- **WHEN** 审查完成
- **THEN** AUDIT_REPORT.md 中对 daemon / executor / ipc / pool / scheduler / store / task / lib 每个模块都有独立审查结论
- **AND** 每条结论引用具体文件:行号

### Requirement: 优点与缺点分类产出

审查报告 SHALL 将发现分为三类：

- **优点（Strengths）**：值得保留的好功能、设计决策、工程实践
- **缺点（Weaknesses）**：代码异味、潜在 bug、风险点、不一致之处
- **待完善（Improvements）**：未实现功能、stub、TODO、可改进的增强项

#### Scenario: 每条发现可追溯

- **WHEN** 报告列出一条优点或缺点
- **THEN** 该条目包含：所在文件:行号、现象描述、影响评估
- **AND** 缺点类条目额外包含：建议修复方向

### Requirement: 改进建议优先级排序

审查报告 SHALL 为所有缺点与待完善项标注优先级：

- **P0**：安全漏洞、数据丢失风险、潜在 panic、死锁
- **P1**：功能缺陷、错误处理不当、资源泄漏、不一致行为
- **P2**：代码异味、可读性、命名、注释、文档

#### Scenario: 优先级合理

- **WHEN** 审查发现安全问题（如命令注入、路径遍历）
- **THEN** 标注为 P0
- **WHEN** 发现资源泄漏或错误处理缺陷
- **THEN** 标注为 P1

## MODIFIED Requirements

无（本次为只读审查，不修改任何现有需求）。

## REMOVED Requirements

无。
