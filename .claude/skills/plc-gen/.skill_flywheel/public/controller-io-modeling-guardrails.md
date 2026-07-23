# Controller IO Modeling Guardrails

这个工件只回答一个问题：

> 对 scaffold 项目或 complex delivery，controller 与 IO topology 应该怎么建模，哪些写法应视为硬失败？

## complex delivery 的默认规则

- controller 优先写成 `device plc_main: plc { model_ref: ... }`
- 业务 DSL 里优先 authoring 语义设备，如 `sensor`、`solenoid_valve`、`cylinder`、`lamp`、`motor`
- 用 `relation { from, to, via }` 把现场对象接到 `plc_main.<port>`

## 不推荐的写法

对 scaffold 或 complex delivery，不要默认使用：

- `device plc_main: plc { ports: [...] }`
- 大量只叫 `X0`、`Y0` 的业务 `device`
- 把 controller channel 直接当作整个业务 topology

## 先修拓扑，再修 task

如果 validation 暴露：

- `SEM-108`
- `SCN-MAP-010`

优先重写 controller / IO topology，再继续修 task、scenario 或 gate。

## 最低可接受形态

- `plc_main` 使用 `model_ref`
- 语义设备有 `purpose`
- `relation` 显式闭合
- 操作员输入优先建模成 `push_button`、`selector_switch` 这类语义设备，而不是裸 channel alias
