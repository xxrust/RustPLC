# Axis Fault 路由映射规则（US-008）

本文档定义 `axis.move_*` 的“主桶 + 细分路由”映射规则，用于 parser/semantic/IR 的一致实现。

## 1. DSL 语法

在 `axis.move_relative` / `axis.move_absolute` 后，保留三类主桶分支：

- `on_reject -> task.step`
- `on_motion_fault -> task.step`
- `on_safety_fault -> task.step`

并支持细分 matcher：

- `on_<bucket>(kind: reject|motion|safety|vendor) -> task.step`
- `on_<bucket>(code: <int>) -> task.step`
- `on_<bucket>(kind: ..., code: ...) -> task.step`

示例：

```plc
action: axis.move_relative(axis_x, distance: 10, speed: 2)
    timeout: 500ms -> fault.timeout
    on_reject -> fault.reject
    on_motion_fault -> fault.motion_default
    on_motion_fault(kind: vendor) -> fault.motion_vendor
    on_motion_fault(code: 17) -> fault.motion_code_17
    on_safety_fault -> fault.safety
```

## 2. 编译期规则

- 每个主桶分支仍然必填；缺失时保持原有报错：
  - `AXIS-002` / `AXIS-003` / `AXIS-004`
- 同一主桶只能声明一次“无 matcher”分支；重复主桶在 parser 阶段报错。
- matcher 字段白名单仅允许 `kind` 与 `code`，并且各字段在单条路由中最多出现一次。
- 桶兼容性在语义阶段校验：
  - `on_reject` 仅允许 `kind: reject|vendor`
  - `on_motion_fault` 仅允许 `kind: motion|vendor`
  - `on_safety_fault` 仅允许 `kind: safety|vendor`
  - 违规报 `AXIS-010`

## 3. 路由匹配顺序（IR 语义）

给定某主桶的 `primary` 分支和 `routes[]` 细分路由，匹配顺序固定为：

1. 按声明顺序遍历 `routes[]`
2. 对每条路由执行 matcher：
   - `kind` 未声明视为通配
   - `code` 未声明视为通配
   - 同时满足才命中
3. 首条命中即返回对应目标
4. 若无命中，回退到主桶 `primary`

该规则在 `src/ir/mod.rs` 的 `resolve_axis_fault_route_target(...)` 中固化。
