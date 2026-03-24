# Workpiece To ST Codegen Policy

## 1. 目标

本文件定义 RustPLC 中 `workpiece` 语义进入 IEC 61131-3 ST 代码生成时的固定策略。

结论只有一条：

- `workpiece` 属于 `verification / simulation / diagnostics` 语义
- ST 只承载可执行控制语义
- ST 后端不负责保留工件对象模型

这不是临时 workaround，而是分层边界。

## 2. 基本原则

RustPLC 中的 PLC 程序在运行时只能直接依赖以下信息做控制决策：

- 传感器输入
- 显式变量
- 动作完成 / 动作失败结果
- `wait / timeout`
- 设备与执行器状态

运行时不应直接依赖以下信息做控制决策：

- 工件 token 状态
- slot / holder / carrier 的工件占用语义
- lineage / 派生关系
- 工件数量推理结果

原因很直接：

- 控制器不能直接“操纵工件语义对象”
- 控制器只能驱动执行器、读取传感器、维护普通控制变量
- 工件模型的主要价值是验证动作与观测是否自洽，而不是作为 ST 中的运行时对象

## 3. 可擦除语义

以下 `workpiece effect` 在 ST codegen 前统一擦除：

- `acquire`
- `transfer`
- `finish`
- `mount`
- `unmount`
- `split`
- `merge`
- `transform carrier`

这些 effect 的含义是：

- 描述某一步对工件语义产生了什么影响
- 供 verification、simulation、trace、diagnostics 使用
- 不是 ST 目标代码必须显式执行的对象操作

因此：

- 它们允许进入 IR
- 允许进入 runtime 的工件跟踪侧
- 允许进入 verification
- 但不直接进入 ST 输出

## 4. ST 后端策略

`generate_st` 的策略固定为：

1. 保留普通控制语义
2. 擦除全部 workpiece topology / contract / effect
3. 继续对擦除后的 IR 做 ST 可生成性检查

这意味着：

- workpiece 的存在本身不再导致 ST backend 直接拒绝
- 语义资源互锁这类真正无法映射到当前 ST 后端的能力，仍然继续拒绝

为了避免静默语义漂移，ST 输出头部必须显式标注：

- 已执行 workpiece semantics erasure
- 输出只保留 executable control semantics

## 5. 明确禁止的方向

下列能力不应作为 workpiece 模型的后续扩展方向：

- 基于工件状态的条件分支
- 基于 slot / token 占用状态的 `if` 或 `wait`
- 基于 lineage 的运行时决策
- 基于工件数量推理结果的运行时控制流

如果业务上需要表达“有料才继续”，正确方式应是：

- 使用传感器
- 使用执行结果
- 使用普通变量

例如应写成：

```plc
wait: tray_slot_0_present == true
```

而不是：

```plc
wait: tray_a.slot[0] occupied
```

前者是控制器真实可观测语义，后者不是。

## 6. 对 DSL 作者的约束

如果一个 step 的业务含义既包含物理动作，也包含工件语义变化，应按两层理解：

- 物理动作由普通 `action` 表达
- 工件变化由 `effect` 表达

如果一个示例当前只有 `effect` 没有 `action`，它仍可用于：

- 验证
- 仿真
- 谱系追踪
- 语义回归

但生成 ST 时，这些 effect 会被擦除，最终只保留控制流骨架。

这符合当前架构目标，因为 ST 不是工件验证报告，也不是谱系数据库导出。

## 7. 实现落点

当前策略的主要落点为：

- `src/codegen/st.rs`

该后端负责：

- 擦除 workpiece 相关约束与 effect
- 对擦除后的状态机继续做 ST codegen
- 在输出头部写明已发生语义擦除

语义与验证层仍继续保留完整 workpiece 模型，不受此策略影响。
