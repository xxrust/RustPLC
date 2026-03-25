---
name: plc-gen
description: "基于已确认的系统描述或等价工业控制需求，生成可通过验证的 RustPLC DSL（`.plc`）。当用户需要 RustPLC 程序、需要在 scaffold 项目中填充 `main.plc`、需要验证或修复现有 `.plc`，或在不了解仓库细节时需要精确的 RustPLC scaffold 与 CLI 命令时使用。"
---

# plc-gen

生成能够通过真实流水线的 RustPLC DSL。

只有当产出的 `.plc` 通过当前 RustPLC 工具链验证时，这个 skill 才算完成。

保持本文件精简。
按需加载对应 reference 文件：

- `references/workflow.md`
  用于端到端生成与验证流程。
- `references/commands.md`
  用于精确 CLI 命令与 launcher 选择。
- `references/project-layout.md`
  用于 scaffold 后指导用户先改哪些文件。
- `references/output-contract.md`
  用于约束最终交付格式与结果状态。
- `references/troubleshooting.md`
  用于命令发现失败或环境不清晰时排障。

## Source of Truth

遵循以下项目规则：
- `AGENTS.md`
- `docs/architecture/signal-direction.md`

不要发明第二套语义模型。
生成的代码必须匹配现有 parser、semantic gate、runtime bridge 与 verification 链路。

## Input Contract

优先输入：
- 已确认的 `.system.md`

如果用户没有提供 `.system.md`，只在剩余歧义很小的情况下才内部补一个最小 system model。
如果歧义会影响 safety、task 划分或 fault handling，只问最少的阻塞问题。

## Default Workflow

1. 先读取已确认的系统意图。
2. 只要请求不止是单文件，就优先走 scaffold 项目。
3. 构建 topology、constraints、tasks 与 failure path。
4. 默认采用保守的 task 与 timeout 设计。
5. 用真实 RustPLC 工具链验证。
6. 反复修复，直到程序通过，或明确存在真实 contract 缺口。

## Scaffold Rule

当用户需要完整项目，或要求端到端验证时，先使用 scaffold：

```bash
cargo run --release --bin rust_plc -- new my_plc_project
```

然后将产物写入：
- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`

如果不走 scaffold，生成的 `.plc` 要与配套 `.system.md` 放在一起。

将 RustPLC 视为两种 launcher 模式：

- 已安装 binary 模式：`rust_plc ...`
- source workspace 模式：`cargo run --release --bin rust_plc -- ...`

不要给出不带 `--bin rust_plc` 的 `cargo run --release -- ...`。
这个 workspace 有多个 binary，短写法不可靠。

## Generation Rules

始终强制：
- 每个 device 都有 `purpose`
- topology 使用显式 `relation { from, to, via }`
- task 语义遵循 concurrent-task 与 blocking-step 规则
- 人工等待使用 `allow_indefinite_wait: true`
- 非人工等待必须有明确 timeout route
- failure route 必须是具体 task，不能只是含糊注释

除非工艺明显需要其他结构，否则优先使用这一类 task 骨架：
- `ready`
- `cycle`
- 一个或多个 `fail_*` task

## Concurrency and Blocking

默认把以下语句视为 blocking：
- `wait`
- `delay`
- `timeout`
- `axis.move_relative`
- `axis.move_absolute`
- 对外部反馈的等待

如果某个 action 必须在 axis move 完成后发生，就把它拆到后续 step。

如果独立工位可以在另一个工位等待时继续推进，就建模成独立 task，不要把所有逻辑压扁到单个 `cycle` task。

## Device and Constraint Heuristics

优先：
- `plc_main: plc { ports: [...] }`
- 在真实场景下，为 cylinder 配套 `_ext` / `_ret` feedback
- 用显式 `requires` 表达依赖约束
- `conflicts_with` 仅用于真正的状态共存冲突

不要用 `conflicts_with` 编码纯执行顺序。

## Analog、PID、Axis Rules

对于 analog signal：
- 使用 `analog_input` / `analog_output`
- 始终声明 `range` 与 `unit`
- 当范围判断更安全时，避免精确 `==` threshold

对于 PID：
- `pv` 与 `out` 命名要和真实 analog device 名称一致

对于 axis motion：
- 优先 `axis.move_relative` / `axis.move_absolute`
- 必须包含 `timeout`
- 必须包含 `on_reject`
- 必须包含 `on_motion_fault`
- 必须包含 `on_safety_fault`

## Validation Loop

不要依赖顶层 `--help`。
直接给出准确的 subcommand 语法。

只要环境允许，就用真实 toolchain 验证生成结果。

如果调用方有已安装 binary，优先：

```bash
rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

如果你在 source workspace 中，优先：

```bash
cargo run --release --bin rust_plc -- scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

同时运行：

```bash
cargo run --release --bin rust_plc -- scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human
```

当请求是项目级交付且 scenario 已准备好时，补跑 `no-board-gate`。

## Fixture Discipline

这个 skill 的 fixture 回归位于：
- `.codex/skills/plc-gen/fixtures/valid/*.plc`

当 skill 规则发生实质变化时：
- 更新或新增一个有代表性的 fixture
- 运行 `cargo test --test plc_gen_skill_fixtures`

## Output Style

默认输出顺序：
1. 简短结果
2. 生成的 `.plc`
3. assumptions
4. validation 状态

除非用户明确要求展开，否则解释保持简短。
