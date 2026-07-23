# DualSlot Shuttle Press Cell

这是一个 station 级 RustPLC structured-fragments 工程。权威系统契约位于 `plc/main.system.md`，可编译入口为 `rustplc.bundle.toml`。

源侧工艺调度意图位于 `process_model/process_operation_model.toml`。验证工件统一写入 `out/`，不得作为 authored source 回写。

本工程实现有限两件验收周期：两件 `insert_part` 从 `raw_infeed` 装载到双槽 `shuttle_tray`，往返压装后卸载到 `good_outfeed` 并完成。

## 自测

从仓库根目录运行：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/run_delivery_project_corpus.ps1
```

项目内权威机器结果位于 `runs/20260723-172826/result.json`。当前 harness execution 为 `pass`，acceptance 与 delivery 均为 `blocked`；仓库级 runner 会把“fixture 可验证”与“工程可交付”分开汇总。

已验证范围包括 compile、四类 verification、state-proof、六个 scenario 的 validate/doctor、nominal 两件周期、startup self-check、illegal-start、七个 intent milestone 和统一 `project-check` 中的 `intent_alignment=aligned`。

当前产品缺口包括：transform-carrier 的 process-model `OP-003`、`sim-plc` 缺少 axis/cylinder fault injection、`trace-doctor` 无法加载 bundle、phase-2.v1 intent schema 无法表达 external runtime reinitialization、runtime 无法注入并保留启动前残件的精确 token 位置，以及 PLC 内部状态码尚无目标 HMI 传输/寄存器绑定。持续 front-door、残件人工协助入口和 PLC 内部 status/fault code producer 已进入可执行 DSL 与场景回归。
