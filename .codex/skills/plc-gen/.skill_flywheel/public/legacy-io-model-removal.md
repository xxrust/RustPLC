# Legacy IO Model Removal

这个工件只回答一个问题：

> 当输入仍是 legacy IO alias 思路时，`plc-gen` 应该怎么重写，而不是把旧模型直接搬进 complex delivery？

## 需要移除的 legacy 形态

对 scaffold 或 complex delivery，以下形态应优先移除：

- 大量 `device x0: digital_input`
- 大量 `device y0: digital_output`
- 在 `device plc_main: plc` 里直接内联 `ports: [...]`
- 把按钮、模式选择、点动请求、报警位直接建成 channel alias device

`digital_input` / `digital_output` 这类名字 reserved for real hardware equipment at the board edge，不应该成为业务 topology 的默认中间层。

## 推荐替换方式

- controller 改为 `device plc_main: plc { model_ref: ... }`
- 把现场对象重写成 `sensor`、`solenoid_valve`、`cylinder`、`lamp`、`motor`
- 把操作员命令重写成 `push_button`、`selector_switch` 等语义输入设备
- 用 `relation` 做 controller channel 到语义设备的映射

## 触发信号

一旦出现：

- `SEM-108`
- `SCN-MAP-010`

优先判定为 controller / IO 建模问题，不要继续在 task 或 scenario 上打补丁。
