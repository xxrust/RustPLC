# plc-gen Workflow

本文件回答三个问题：

1. 什么时候走 scaffold 项目
2. 什么时候只修单个 `.plc`
3. 生成后必须经过哪些验证

## 1. 先判断交付形态

出现以下任一情况时，优先 scaffold：

- 新 machine / 新 station
- 用户明确要“项目”“脚手架”“完整工程”“交付包”
- 需要 scenario、no-board gate、ST 导出、RP2040 构建
- 用户没有 repo 细节，需要一步步说明先改什么文件

只有在以下场景才优先单文件：

- 修复现有 `.plc`
- 验证现有 `.plc`
- 解释某个编译或 verification 报错

## 2. 项目级默认路径

对一个全新项目，默认顺序是：

1. scaffold 项目
2. 确认 `plc/main.system.md`
3. 生成或修复 `plc/main.plc`
4. 调整 `scenarios/nominal/normal.yaml`
5. 运行 `scenario-validate`
6. 运行 `scenario-doctor`
7. 项目级请求再运行 `no-board-gate`
8. 需要 ST 时再运行 `gen-st`
9. 需要板级交付时再运行 `build-rp2040` 或 `release-bundle`

不要在 `new` 之后立刻推荐 `scenario-init`。
scaffold 已经生成 `scenarios/nominal/normal.yaml`。

## 3. 先判断运行环境

### 已安装 `rust_plc`

这种情况下，用户可以直接：

1. `rust_plc new my_plc_project`
2. `cd my_plc_project`
3. 继续运行 `rust_plc scenario-validate ...`

### 仍在 RustPLC 源码仓中

这种情况下：

- `cargo run --release --bin rust_plc -- ...` 必须从 RustPLC 仓库根目录执行
- scaffold 项目路径要写成仓库根目录可解析的路径
- 不要让用户 `cd` 进 scaffold 目录后再运行 `cargo run ...`

也就是说，source workspace 模式下应写成：

```bash
cargo run --release --bin rust_plc -- scenario-validate out/my_plc_project/plc/main.plc --scenario out/my_plc_project/scenarios/nominal/normal.yaml --output human
```

## 4. 单文件默认路径

对一个现有 `.plc`，默认顺序是：

1. 修复 DSL 与语义问题
2. 运行 `scenario-validate`
3. 运行 `scenario-doctor`
4. 如有需要，导出 ST 或继续做项目级 gate

## 5. 阻塞问题只问关键项

以下属于真正会改变代码结构的阻塞项：

- start mode
- cycle mode
- task 划分
- 哪些等待是 manual wait，哪些必须 timeout
- 关键 actuator / sensor 是否存在
- fault route 应该怎么走

以下内容通常可以先保守默认：

- 占位 I/O 名称
- 中性的 device 名称
- 初始 timeout 数值
- nominal scenario 的起始 timing

## 6. 验证门槛

`plc-gen` 交付不是“生成完就结束”。

最少验证：

- `scenario-validate`
- `scenario-doctor`

项目级推荐验证：

- `scenario-validate`
- `scenario-doctor`
- `no-board-gate`

如果请求是“优化现有 PLC”，也不要跳过验证。
无论是普通生成还是 optimization candidate，最终都必须经过既有语义与 verification 链路。
