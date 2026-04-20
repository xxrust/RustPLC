# Source Shape Selection

这个工件只回答一个问题：

> 面对确认版 `.system.md`，应该直接写单文件 `.plc`，还是应该切到 `.bundle.toml` + fragments + delivery asset？

## 选择单文件 `.plc` 的场景

只在这些场景优先选单文件：

- 现有单文件程序的局部修复
- 极小、一次性、没有 delivery asset 边界的样例
- 用户明确要求只交一个最小 DSL 片段

## 选择 `.bundle.toml` + fragments` 的场景

以下任一成立，就优先切到 `.bundle.toml` + structured fragments：

- 新 scaffold 项目
- `station` / `module` / `line` 级交付
- 多 task、多语义域或需要并行 authoring
- 用户要求 scenario / gate / `project-check`
- 用户要求 complex delivery、canonical example 或 intent-alignment

## `station` 与 `line` 的快速判别

- 单个可独立测试的工艺单元，默认 `station`
- 多工位集成、跨工位节拍衔接、站间交接或用户直接描述为“装配线/产线”，默认 `line`

像 `welding_station`、`dual_axis_platform` 这类“单机/单工位，但包含并行、互锁、scenario、gate 或 delivery docs”的项目，默认仍是 complex delivery。
它们的默认落点是 `station` delivery asset，而不是退回单文件 `main.plc`。

如果 authoritative source 同时满足以下特征，更应优先 `line`：

- 明确写出多个工位或 station
- 描述从前一工位到后一工位的完整节拍
- 强调整线节拍、整线故障停机或整线产出结果

像“`three_station_assembly` / 三工位装配线”这类输入，除非用户明确要求只交付其中一个独立工位，否则默认按 `line` 处理更稳妥。

部署形态提示如“单机运行”“装配岛”不自动把多工位装配线降成 `station`。
当 source 同时出现“单机/装配岛”与“多工位装配线 + 跨工位完整节拍”时，后者优先，默认仍落 `line`。

## scaffold 项目的 authoritative entry

对 `rust_plc new <dir> --layout structured-fragments --delivery-layer station` 这类新项目：

- root `plc/main.system.md` 是项目级索引或 bridge
- root `plc/main.target_semantics.bundle.toml` 是项目级 compile surface 入口
- 真正对应 station asset 的 authored docs / source entry / scenario 在：
  - `plc/deliveries/station/<slug>/docs/*.md`
  - `plc/deliveries/station/<slug>/plc/main.bundle.toml`
  - `plc/deliveries/station/<slug>/scenarios/nominal/normal.yaml`

对 complex delivery，不要只改 root scaffold 文件而让 delivery asset docs 继续保持占位状态。
即使 delivery layer 最终是 `station`，也仍然要按 delivery asset 方式 authoring docs、bundle、scenario 与默认需要的 intent sidecar。

## confirmed system 的落盘顺序

如果确认版 `.system.md` 来自项目外部：

1. 先把事实落到 delivery asset `docs/*.system.md`
2. 再决定 delivery asset `main.bundle.toml`
3. 再把 topology / constraints / tasks 拆进 fragments

不要把“根目录已经有 `plc/main.system.md`”误当成 delivery asset 已经 authoring 完成。
