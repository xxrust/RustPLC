# Operator Command Modeling

这个工件只回答一个问题：

> 操作员按钮、模式选择和点动请求在 DSL 里应该如何建模？

## 默认建模

- 启动、停止、复位、确认按钮：优先 `sensor` + `push_button`
- 模式选择开关：优先 `sensor` + `selector_switch`
- 点动、手动请求：优先语义输入设备 + `relation`，不要只写变量别名

## 为什么

- 这类对象是操作员接口，不是纯 controller channel
- 语义 subtype 比裸 `X0` / `Y0` 更可读，也更利于后续 docs / review

## 例子

```plc
device start_button: sensor {
    purpose: "操作员启动按钮",
    subtype: "push_button"
}

device mode_selector: sensor {
    purpose: "自动/手动模式选择",
    subtype: "selector_switch"
}
```

## 不推荐的写法

- `device start_x0: digital_input`
- `device auto_mode_bit: digital_input`
- 把操作员命令直接塞成 controller `ports: [...]` 的语义替代物
