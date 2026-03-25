# plc-gen Workflow

当调用方需要面向产品的 RustPLC 生成流程，而不是仓库内部导览时，使用本文件。

## 1. Pick the Launch Mode

先确定 launcher 前缀：

- 已安装 binary 模式：`rust_plc`
- source workspace 模式：`cargo run --release --bin rust_plc --`

不要使用 `cargo run --release -- ...`。
这个 workspace 有多个 binary。

## 2. Pick the Delivery Shape

当用户有以下需求时，使用 scaffold 项目：

- 新 machine 或 station
- 面向客户交付的项目
- 端到端验证
- scenario validation 或 no-board gate
- 需要明确的目录与文件指导

只有在请求非常收敛时，才走单文件流程：

- 修复单个 `.plc`
- 验证单个 `.plc`
- 解释单个 compiler 或 validation 失败

## 3. Recommended Generation Path

对于新项目：

1. 先 scaffold 项目
2. 直接把 scaffold 自带的 `scenarios/nominal/normal.yaml` 作为 nominal scenario 起点
3. 确认或编写 `plc/main.system.md`
4. 生成或修复 `plc/main.plc`
5. 调整 `scenarios/nominal/normal.yaml`
6. 运行 `scenario-validate`
7. 运行 `scenario-doctor`
8. 如果是项目级交付，再运行 `no-board-gate`
9. 只有在要求 ST 输出时，才运行 `gen-st`

对于现有 PLC：

1. 先看需求或当前失败现象
2. 修复 `main.plc`
3. 用 `scenario-validate` 验证
4. 用 `scenario-doctor` 诊断
5. 如有需要再导出 ST

不要在 `new` 之后立刻推荐 `scenario-init`。
scaffold 已经自带 `scenarios/nominal/normal.yaml`。
只有当 scenario 文件缺失，或调用方希望从独立 `.plc` 重新生成 scenario skeleton 时，才使用 `scenario-init`。

## 4. Blocking Questions Only

把以下事项视为真正的阻塞项：

- start mode
- cycle mode
- 关键 actuator 与 sensor 是否存在
- 某个 wait 是 indefinite 还是 timed
- timeout 与 fault routing expectation
- 独立工作是否应拆成独立 task

除非用户明确在意，否则以下内容采用保守默认值：

- placeholder I/O name
- 中性的 device 名称
- 初始 timeout 值
- nominal scenario 的 timing 值

## 5. Concurrency and Blocking

按当前产品语义建模 RustPLC：

- task 可以并发运行
- blocking step 只阻塞自己的 task
- `wait`、`delay`、`timeout` 与 motion wait 默认都是 blocking
- 如果一个 station 等待时另一个 station 仍应继续运行，就拆成独立 task

不要因为描述方便，就把独立工作压扁成单一串行 task。

## 6. Completion Rule

只有在真实 RustPLC 工具链通过，或已经明确识别出精确 contract 缺口时，生成才算完成。
