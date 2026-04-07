# plc-gen Workflow

本文回答三个问题：

1. 什么时候走 scaffold 项目
2. 什么时候修单文件 `.plc`
3. 什么时候保留或建立多文件 `.bundle.toml` + fragments

## 1. 先判断交付形态

出现以下任一情况时，优先交付 scaffold 项目：
- 新 machine 或新 station
- 用户明确要“项目”“脚手架”“完整工程”“交付包”
- 需要 scenario、`project-check`、`no-board-gate`、ST 导出或板级构建
- 用户缺少 repo 细节，需要一套可直接运行的项目目录

出现以下情况时，优先修单文件 `.plc`：
- 用户已经给出一个现有 `.plc`
- 需求只涉及该 `.plc` 的局部修复、验证或报错解释
- 当前 source boundary 很稳定，没有拆分 `topology`、`constraints`、`tasks` 的必要

出现以下情况时，优先保留或建立多文件 `.bundle.toml` + fragments：
- 项目已经使用 `.bundle.toml`
- 需求天然按 `topology`、`constraints`、`tasks` 分工或分阶段维护
- 希望把 DSL 源按语义块拆分，再由 loader 组装进入编译链
- 需要在多文件边界上稳定映射诊断与协作修改

## 2. 新项目默认路径

对一个全新项目，默认顺序是：

1. scaffold 项目
2. 确认 `plc/main.system.md`
3. 确认 DSL source shape
4. 生成或修复 DSL source entry
5. 调整 `scenarios/nominal/normal.yaml`
6. 运行 `project-check`
7. 需要单步排查时，再拆看 `scenario-validate` / `sequence-lint` / `scenario-doctor` / `no-board-gate`
8. 需要 ST 时再运行 `gen-st`
9. 需要板级交付时再运行 `build-rp2040` 或 `release-bundle`

对 scaffold 默认布局：
- system contract 入口是 `plc/main.system.md`
- DSL source entry 默认是 `plc/main.plc`

对 bundle 布局：
- DSL source entry 是 `<name>.bundle.toml`
- 语义片段通常按 `topology`、`constraints`、`tasks` 分到 fragments

`new` 之后通常已经有 `scenarios/nominal/normal.yaml`，只有在 scenario 缺失或用户明确要求重建 skeleton 时，再优先考虑 `scenario-init`。

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

例如：

```bash
cargo run --release --bin rust_plc -- project-check out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --out-dir out/my_plc_project/out/project_check/normal --output human
```

## 4. 单文件路径

对现有单文件 `.plc`，默认顺序是：

1. 修复 DSL 或 semantic 问题
2. 如果用户有配套 scenario，优先跑 `project-check`
3. 如果只是定点排查，再按需跑 `scenario-validate`、`sequence-lint`、`scenario-doctor`、`no-board-gate`
4. 如有需要，再导出 ST 或继续做项目级 gate

## 5. 多文件 bundle 路径

对现有 `.bundle.toml` + fragments，默认顺序是：

1. 确认 bundle entry 与 fragment 布局
2. 按语义块修复 `topology`、`constraints`、`tasks` 对应 fragments
3. 保持 bundle source boundary 稳定
4. 用 bundle entry 跑 `project-check` 或对应子命令
5. 如有需要，再导出 ST、运行仿真或继续项目级 gate

## 6. 何时额外走 repo 回归

如果当前工作发生在 RustPLC 源码仓内，并且修改的是示例、技能模板、编译语义或 example-backed 行为，优先补这些回归入口：

- `cargo test --test examples_integration`
- `cargo test --test runtime_bridge_us006`
- `scripts/concurrent_runtime_verification_gate.sh`

不要把 scenario CLI 结果当成源码仓语义回归的唯一依据。

## 7. 只问真正改变结构的阻塞项

以下属于会改变 DSL source shape 或结构的关键问题：
- task 划分
- 采用单文件还是 bundle
- 哪些等待是 manual wait，哪些必须带 timeout
- fault 或 warning 分流怎么走
- 关键 actuator 或 sensor 是否存在
- mode 或 supervisor 结构
- 共享资源与互锁边界

以下内容通常可以先保守默认：
- 占位 I/O 名称
- 中性的 device 名称
- 初始 timeout 数值
- nominal scenario 的起始 timing

## 8. 验证门槛

`plc-gen` 交付不是“生成完就结束”。

项目级最小推荐验证：
- `project-check`

如果 `project-check` 不适用或用户只要求局部排查，可拆开：
- `scenario-validate`
- `sequence-lint`
- `scenario-doctor`
- `no-board-gate`

如果请求是“优化现有 PLC”，也不要跳过验证。无论是普通生成、bundle 重组还是 optimization candidate，最终都必须经过现有 semantic / verification / runtime 路径。
