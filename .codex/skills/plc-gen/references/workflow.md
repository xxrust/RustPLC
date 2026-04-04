# plc-gen Workflow

本文件回答三个问题：

1. 什么时候走 scaffold 项目
2. 什么时候只修单个 `.plc`
3. 生成后默认走哪条验证链

## 1. 先判断交付形态

出现以下任一情况时，优先 scaffold：
- 新 machine / 新 station
- 用户明确要“项目”“脚手架”“完整工程”“交付包”
- 需要 scenario、`project-check`、`no-board-gate`、ST 导出或板级构建
- 用户没有 repo 细节，需要一套可运行的项目目录

只有以下场景，才优先只修单文件：
- 修复现有 `.plc`
- 验证现有 `.plc`
- 解释某个编译、semantic、verification 或 scenario 报错

## 2. 新项目默认路径

对一个全新项目，默认顺序是：

1. scaffold 项目
2. 确认 `plc/main.system.md`
3. 生成或修复 `plc/main.plc`
4. 调整 `scenarios/nominal/normal.yaml`
5. 运行 `project-check`
6. 需要单步排查时，再拆看 `scenario-validate` / `sequence-lint` / `scenario-doctor` / `no-board-gate`
7. 需要 ST 时再运行 `gen-st`
8. 需要板级交付时再运行 `build-rp2040` 或 `release-bundle`

不要在 `new` 之后立刻推荐 `scenario-init`。脚手架已经生成 `scenarios/nominal/normal.yaml`。

## 3. 先判断运行环境

### 已安装 `rust_plc`

这种情况下，用户可以直接：

1. `rust_plc new my_plc_project`
2. `cd my_plc_project`
3. 在项目目录内继续运行 `rust_plc project-check ...`

### 仍在 RustPLC 源码仓库内

这种情况下：

- `cargo run --release --bin rust_plc -- ...` 必须在 RustPLC 仓库根目录执行
- scaffold 项目路径要写成仓库根目录可解析的路径
- 不要让用户 `cd` 进 scaffold 目录后再跑 `cargo run ...`

也就是说，source workspace 模式下应写成：

```bash
cargo run --release --bin rust_plc -- project-check out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --out-dir out/my_plc_project/out/project_check/normal --output human
```

## 4. 单文件默认路径

对一个现有 `.plc`，默认顺序是：

1. 修复 DSL / semantic 问题
2. 如果用户有配套 scenario，优先跑 `project-check`
3. 如果只是定点排查，再按需跑 `scenario-validate`、`sequence-lint`、`scenario-doctor`、`no-board-gate`
4. 如有需要，再导出 ST 或继续做项目级 gate

## 5. 何时额外走 repo 回归

如果当前工作发生在 RustPLC 源码仓内，并且修改的是示例、技能模板、编译语义或 example-backed 行为，优先补充这些回归入口：

- `cargo test --test examples_integration`
- `cargo test --test runtime_bridge_us006`
- `scripts/concurrent_runtime_verification_gate.sh`

不要把 scenario CLI 结果当成源码仓语义回归的唯一凭据。

## 6. 只问真正改变结构的阻塞项

以下属于会改变 `.plc` 结构的关键问题：
- task 划分
- 哪些等待是 manual wait，哪些必须带 timeout
- fault / warning 分流怎么走
- 关键 actuator / sensor 是否存在
- mode / supervisor 结构
- 共享资源与互锁边界

以下内容通常可以先保守默认：
- 占位 I/O 名称
- 中性的 device 名称
- 初始 timeout 数值
- nominal scenario 的起始 timing

## 7. 验证门槛

`plc-gen` 交付不是“生成完就结束”。

项目级最小推荐验证：
- `project-check`

如果 `project-check` 不适用或用户只要求局部排查，可拆开：
- `scenario-validate`
- `sequence-lint`
- `scenario-doctor`
- `no-board-gate`

如果请求是“优化现有 PLC”，也不要跳过验证。无论是普通生成还是 optimization candidate，最终都必须经过现有 semantic / verification / runtime 路径。
