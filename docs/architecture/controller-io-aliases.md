# Controller I/O Aliases

## 定位

`controller_io` 是项目级 I/O 命名层。它解决的是工程可读性问题：在 `connections.plc` 中使用业务语义名称，而不是把 `plc_main.X0`、`plc_main.Y0` 这类物理点位散落到所有拓扑连接里。

它不替代控制器硬件清单：

- `devices/controllers/*.toml` 仍然是控制器 profile，声明真实端口 `X0`、`Y0`、`AI0`、`AO0` 的方向和类型。
- `controller_io` 只在项目源码中把业务别名绑定到这些已存在端口。
- semantic/preprocess 会把别名降级成 canonical 物理端口，再生成内部 `X/Y/AI/AO` 合成节点。
- IR、runtime、verification、codegen 继续消费 canonical I/O 节点，不直接依赖别名。

## 推荐写法

在 `00_topology/controller.plc` 中集中定义：

```plc
device plc_main: plc {
    purpose: "main controller"
    model_ref: openplc_softplc
}

controller_io plc_main {
    input start_cycle_cmd: X0 {
        purpose: "启动按钮输入"
    }

    output feed_belt_run: Y0 {
        purpose: "分料输送带运行输出"
        safe_state: off
    }
}
```

在 `00_topology/connections.plc` 中使用别名：

```plc
relation { from: start_button.out, to: plc_main.start_cycle_cmd, via: reports_to }
relation { from: plc_main.feed_belt_run, to: feed_belt.cmd, via: driven_by }
```

preprocess 后等价于：

```plc
relation { from: start_button.out, to: X0, via: reports_to }
relation { from: Y0, to: feed_belt.cmd, via: driven_by }
```

## 约束

- 别名必须绑定到 controller profile 中已经存在的端口。
- `input` 只能绑定 `X*` / `AI*`，`output` 只能绑定 `Y*` / `AO*`。
- 同一个控制器下别名不能重复。
- 同一个物理端口只能声明一个项目级别名。
- 别名不能写成 `X0`、`Y0`、`DI0`、`DO0` 这类物理或通道名。
- 别名不能与设备名冲突。
- 每个别名必须有非空 `purpose`。
- `safe_state` 只允许用于 `output` 别名。

## 设计边界

`controller_io` 只命名 PLC 边界，不把 I/O 通道建成普通设备，也不允许 task 直接写控制器别名：

```plc
action: set plc_main.feed_belt_run on
```

这种写法仍会按 `SEM-110` 拒绝。正常 task 应操作语义设备，例如：

```plc
action: set feed_belt.cmd on
```

这能保持 task 中的设备动作处在高层语义，避免把控制器点位脚本下沉到流程逻辑。
