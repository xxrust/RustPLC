# 工件模型规范（Workpiece Model Spec, WPM）

## 1. 这份文档是干什么的

这份文档不是代码实现文档，而是功能审查文档。

它回答四类问题：

1. RustPLC 为什么要有工件模型。
2. 工件模型第一版到底准备支持什么。
3. 哪些功能是第一版明确不做的。
4. 每个功能是否真的可实现，最小例子长什么样。

这份文档的目标不是把术语写得“高级”，而是让你能直接审查：

- 这个功能有没有必要
- 这个定义是否清楚
- 这个范围是否过大
- 这个第一版到底能不能落地

---

## 2. 先用人话讲问题

现在 RustPLC 比较擅长建模这些东西：

- 设备
- 动作
- task / step
- 风险区互锁
- 并发推进

但一个工业控制系统的真正目标，通常不是“某个气缸伸出”或“某个轴转到某个位置”，而是：

- 某个对象进入系统
- 被搬运
- 被加工
- 被检测
- 被分流
- 最后变成成品、废品、返修件，或者被组装成新对象

这里的“某个对象”，就是工件。

如果系统只建模设备动作，不建模工件本身，就会出现一个问题：

- 你能证明机器动了
- 你能证明某个危险区被占用了
- 但你很难证明“工件到底发生了什么”

比如你很难严格证明：

- 这个工件现在到底在哪
- 它是不是已经被切成 4 片了
- 它是不是还挂在铁板上
- 两个工件是不是已经组装成一个新工件
- 某个工件是不是已经正常结束，而不是“逻辑里丢了”

所以 RustPLC 需要工件模型。

---

## 3. 先解释术语

这一节只做一件事：把后面要用到的词先翻译成人话。

### 3.1 workpiece

`workpiece` 就是工件，也就是系统真正处理的对象。

例子：

- 晶棒
- 晶片
- 电芯
- 模组
- 零件
- 托盘中的单个物料

注意：

- RustPLC 内核不内建 `wafer`
- RustPLC 内核只内建“工件”这类通用概念
- `wafer`、`rod`、`cell` 都只是具体项目里的工件类型

### 3.2 workpiece type

`workpiece type` 就是工件类型。

你可以理解成：

- “这类工件叫什么”
- “它有哪些属性”
- “它允许经历哪些变化”

例如：

- `rod`：晶棒
- `slice`：切出来的片
- `cell`：电芯
- `module`：组装后的模组

### 3.3 token

`token` 可以理解成“一个具体工件实例”。

不是“这类工件”，而是“这一个工件”。

比如：

- 一根具体的晶棒
- 一片具体的晶片
- 一个具体的电芯

为什么要有这个概念：

- 否则你只能说“有晶片”
- 但不能说“这片晶片是从哪根晶棒切出来的”

### 3.4 site

`site` 是更通用的“放置位”。

它的意思是：

- 某个对象当前被放在哪里

这里的对象不一定是工件，也可能是：

- 托盘
- 铁板
- 治具
- 夹板

所以 `site` 是总概念。

### 3.5 workpiece location

`workpiece location` 是“专门放工件的位置”。

例如：

- 原料架中的工件位
- 切割位
- 检测位
- 出料位
- 废料盒

如果一个位置上放的是“真正被加工的对象”，那它属于这一类。

### 3.6 carrier location

`carrier location` 是“专门放载具或可流转治具的位置”。

例如：

- 原料架上放托盘的位置
- 等待区放铁板的位置
- 上料区放治具的位置

如果一个位置上放的不是工件本体，而是托盘、铁板、治具这类承载对象，那它更应属于这一类。

### 3.7 capacity

`capacity` 就是容量上限。

意思是：

- 这个位置最多同时允许放几个工件

例如：

`capacity: 4`

意思就是：

- 这个位置最多同时有 4 个工件

不是说默认有 4 个工件，而是上限是 4。

### 3.8 holder

`holder` 是“主动持有工件的东西”。

一般是执行机构，不是静态位置。

例如：

- 机械手夹爪
- 真空吸嘴
- 夹具

和 `location` 的区别：

- `location` 更像“工件在某处”
- `holder` 更像“工件被某机构拿着”

### 3.9 carrier

`carrier` 是“载具”。

你可以理解成：

- 一个东西上面可以装多个工件
- 然后整个东西一起移动

例如：

- 铁板
- 托盘
- 治具
- 夹板

这里要特别区分两类“治具”：

1. 可流转治具

- 会被上料、下料、搬运、更换
- 上面会挂载工件
- 这种应建模成 `carrier`

2. 固定治具

- 属于设备固定组成部分
- 不在流程里被搬运
- 这种不应建模成 `carrier`
- 更接近设备、拓扑对象或语义资源

### 3.10 slot

`slot` 是载具上的“槽位”或“位置编号”。

例如：

- `steel_plate.slot[0]`
- `tray_a.slot[5]`
- `tray_scan.slot[12, 7]`

意思就是：

- 某个工件挂在某个载具的第几个位置上

如果载具本身是二维阵列，也可以把 `slot` 写成多维离散地址。

例如：

- `tray_scan.slot[row, col]`

它的意思不是连续坐标，而是：

- 这个工件位于 tray 的第 `row` 行、第 `col` 列

对扫描类设备，还需要一个额外概念：

- 槽位遍历顺序

例如：

- `row_major`：先按行扫，再扫列
- `column_major`：先按列扫，再扫行

这不是几何引擎，只是有限离散槽位的访问顺序声明。

### 3.11 mount / unmount

`mount`：

- 把工件挂到某个载具上

`unmount`：

- 把工件从载具上取下来

例如：

- 把 3 根晶棒固定到一块铁板上
- 再把其中一根从铁板上拆下来

### 3.12 split

`split` 就是“一个工件分裂成多个工件”。

例如：

- 一根晶棒切成 400 片晶片
- 一块板料切成 8 个零件

### 3.13 merge

`merge` 就是“多个工件聚合成一个新工件”。

例如：

- 多个零件装配成一个模块
- 多个电芯组装成一个模组

### 3.14 lineage

`lineage` 就是“工件谱系”。

也就是：

- 这个工件从哪里来
- 它是不是由别的工件派生出来的
- 它是不是由多个工件组装出来的

例如：

- 某片晶片来自哪根晶棒
- 某个模块由哪几个电芯组成

### 3.15 terminal state

`terminal state` 是“终态”。

意思是：

- 工件生命周期最后可以合法结束在哪些状态

但在 WPM-v1 里，终态不再只写成一组，而是显式分成两组：

- `normal_terminal_states`：正常终态
- `abnormal_terminal_states`：异常终态

例如：

- `finished`：完成，通常属于正常终态
- `scrapped`：报废，通常属于异常终态
- `rejected`：拒收，通常属于异常终态

如果一个工件最后既不在流程位置里，也不在合法终态里，那通常说明：

- 工件语义没有闭合
- 也就是“逻辑上把工件弄丢了”

所以：

```plc
normal_terminal_states: [finished]
abnormal_terminal_states: [scrapped]
```

意思就是：

- 这种工件最后合法结束时，只能是完成或报废
- 而且完成和报废在语义上不是同一类结束

再补一条边界：

- `consumed` 不建议作为正常终态或异常终态
- 它更适合表示“该工件被 split / merge / consume 显式消耗掉了”
- 也就是说，它通常不经过 egress，而是在工艺变换中结束自身身份

### 3.16 ingress / egress

这组概念用来描述工件“从哪里进入流程、最后从哪里离开流程”。

它和 `transfer` 不是一个层级的问题：

- `transfer` 讲的是某一步把工件从 A 挪到 B
- `ingress / egress` 讲的是整个流程中工件允许从哪里进、从哪里出

WPM-v1 只保留 3 项：

- `ingress_sites`：工件允许从哪些位置进入流程
- `normal_egress_sites`：工件正常结束时允许从哪些位置离开流程
- `abnormal_egress_sites`：工件异常结束时允许从哪些位置离开流程

并且要求把“终态”和“出口”明确对应起来：

- `normal_terminal_states` 只能走 `normal_egress_sites`
- `abnormal_terminal_states` 只能走 `abnormal_egress_sites`
- 不能出现“正常终态走异常出口”
- 也不能出现“异常终态走正常出口”
- 如果某工件是被 `split / merge / consume` 显式消耗，则它不属于 egress 离开，而属于工艺内终结

V1 采用闭世界规则：

- 合法出口全集 = `normal_egress_sites ∪ abnormal_egress_sites`
- 任何未声明出口一律视为非法出口

例子：

```plc
workpiece slice: workpiece_type {
    normal_terminal_states: [finished]
    abnormal_terminal_states: [scrapped]
    ingress_sites: [steel_plate.slot[*]]
    normal_egress_sites: [good_outfeed]
    abnormal_egress_sites: [scrap_bin, rework_bin]
}
```

这段话的意思是：

- `slice` 从 `steel_plate` 的槽位进入流程
- 如果它以 `finished` 结束，只能去 `good_outfeed`
- 如果它以 `scrapped` 结束，只能去 `scrap_bin` 或 `rework_bin`
- 其他任何出口都视为非法

---

## 4. 第一版到底要支持什么

WPM-v1 只做“有限、离散、可验证”的工件模型。

第一版准备支持：

1. 工件类型
2. site
3. workpiece location
4. carrier location
5. holder
6. carrier 和 slot
7. ingress / egress 契约
8. 工件转移
9. 工件挂载和解绑
10. 有限 split
11. 有限 merge
12. 基本谱系追踪
13. 基本验证

这里的关键词有三个：

- 有限
- 离散
- 可验证

这三个词决定了第一版为什么是可实现的。

---

## 5. 第一版明确不支持什么

这一节很重要，因为它决定了方案不会无限膨胀。

WPM-v1 明确不支持：

1. 不支持无界数量工件推理。
2. 不支持连续几何求解。
3. 不支持精确碰撞检测。
4. 不支持任意深度的复合件树。
5. 不支持动态创建无限新工件。
6. 不支持任意复杂的浮点属性验证。

换句话说，第一版不是在做：

- 3D 物理仿真器
- CAD 几何内核
- 无界模型检查器

第一版只做：

- 有限状态工件语义系统

---

## 6. 第一版的核心功能

下面每个功能都按同样的顺序来写：

1. 先讲它是什么
2. 再讲为什么要有
3. 再给最小例子
4. 最后说第一版怎么限制，保证可实现

### 6.1 工件类型

#### 它是什么

工件类型就是“系统里有哪些工件类别”。

#### 为什么要有

没有工件类型，系统只能说：

- 某个东西在移动

但不能说：

- 这是晶棒
- 那是晶片
- 这是电芯
- 那是模组

#### 最小例子

```plc
[topology]
workpiece rod: workpiece_type {
    properties: [
        cut_ready: bool,
        grade: enum(a, b)
    ]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [scrapped]
    allows: [split_into(slice)]
}
```

这段话用人话解释就是：

- 定义一种工件，名字叫 `rod`
- 它有两个属性：
- `cut_ready`：是否可切割
- `grade`：等级，只有 `a` 或 `b`
- 它最后合法结束时，可以是正常完成，也可以是异常报废
- 它允许被切分成 `slice`

#### 第一版限制

- 属性只支持 `bool` 和 `enum`
- 不支持任意浮点表达式
- 对于 `split` / `merge` 这类能力，第一版要求在类型层显式声明允许关系
- 对于流程收敛，第一版要求在类型层显式声明 `ingress_sites` / `normal_egress_sites` / `abnormal_egress_sites`

### 6.2 location

#### 它是什么

这里需要先拆成两层：

- `site`：更通用的放置位
- `workpiece location`：专门放工件的位
- `carrier location`：专门放托盘、铁板、治具等载具的位

#### 为什么要有

你必须能表达：

- 工件在原料架
- 工件在检测位
- 工件在废料盒

同时你也必须能表达：

- 托盘在原料架
- 铁板在等待位
- 治具在上料位

否则验证时根本没法判断工件是否走到了正确位置。

#### 最小例子

```plc
[topology]
site raw_rack: carrier_location { capacity: 4 }
location cut_zone: workpiece_location { capacity: 1 }
location scrap_bin: workpiece_location { capacity: 20 }
```

解释：

- `raw_rack`：原料架，这里放的是托盘/铁板/治具这类载具，最多同时放 4 个
- `cut_zone`：切割位，最多同时放 1 个工件
- `scrap_bin`：废料盒，最多同时放 20 个工件

#### 第一版限制

- `capacity` 必须是有限整数
- 第一版要求 `workpiece location` 和 `carrier location` 显式区分，不允许混写

### 6.3 holder

#### 它是什么

holder 是主动拿着工件的执行机构。

#### 为什么要有

只用 `location` 不够，因为“被机械手拿着”和“放在某个工位上”是两件不同的事。

#### 最小例子

```plc
[topology]
holder arm_head: workpiece_holder { capacity: 1 }
```

解释：

- `arm_head` 是一个 holder
- 它最多同时拿 1 个工件

#### 第一版限制

- 推荐 holder 容量为 1
- 但规范不强制只能为 1

### 6.4 carrier 和 slot

#### 它是什么

carrier 是载具，slot 是载具上的槽位。

#### 为什么要有

因为很多时候不是单个工件自己动，而是：

- 多个工件先固定到一个载具上
- 然后整个载具一起移动

这正是你提到的：

- 多根晶棒固定在铁板上
- 通过平台抬升整块铁板进入切割位置

#### 最小例子

```plc
[topology]
carrier steel_plate: workpiece_carrier { slots: 4 }
```

解释：

- `steel_plate` 是一个载具
- 它有 4 个可用槽位

再看一个更贴近你问题的例子：

```plc
[topology]
site raw_rack: carrier_location { capacity: 4 }
carrier tray_a: workpiece_carrier { slots: 20 }
```

这段话的意思是：

- `raw_rack` 这个位置上放的不是工件，而是托盘
- `raw_rack` 最多同时放 4 个托盘
- `tray_a` 是一个托盘
- `tray_a` 里最多有 20 个工件槽位

如果载具本身是阵列盘，也可以显式声明二维布局。

```plc
[topology]
carrier tray_scan: workpiece_carrier {
    layout: grid(rows: 32, cols: 24)
}
```

这段话的意思是：

- `tray_scan` 不是一串线性槽位，而是 32 行 24 列的离散工件盘
- 其中一个槽位可以写成 `tray_scan.slot[12, 7]`
- 这里的 `12, 7` 是逻辑行列，不是连续空间坐标

对于扫描类流程，还需要显式声明遍历顺序。

```plc
scan tray_scan by row_major
```

它的意思是：

- 扫描 `tray_scan` 时，按“先行后列”的固定顺序遍历所有槽位

如果要写得更接近流程语义，还可以进一步写成：

```plc
foreach slot in tray_scan by row_major
```

它的意思是：

- 对 `tray_scan` 的每个槽位逐个处理
- 遍历顺序固定为 `row_major`

#### 第一版限制

- `slots` 必须是有限整数
- `layout: grid(rows: m, cols: n)` 中的 `m`、`n` 必须是编译期已知的有限整数
- 第一版只支持离散槽位，不支持连续坐标
- `slot[row, col]` 中的维度数量必须与载具布局定义一致
- `foreach slot in <carrier> by <order>` 只允许遍历已声明的有限槽位集合
- 第一版建议只支持 `row_major` / `column_major` 两种固定遍历顺序
- 只有“可流转治具”才能建模成 `carrier`
- 设备固定部分不进入 `carrier` 主路径

### 6.5 transfer

#### 它是什么

`transfer` 是工件从一个地方到另一个地方。

#### 为什么要有

这是工件模型最基本的能力。

你必须能表达：

- 从原料架到机械手
- 从机械手到切割位
- 从切割位到检测位

#### 最小例子

```plc
site raw_rack: carrier_location { capacity: 4 }
carrier tray_a: workpiece_carrier { slots: 20 }

step pick_from_rack:
    action: axis.move_absolute(axis_x, position: 10, speed: 5)
    effect: acquire holder arm_head from tray_a.slot[0]

step place_to_cut_zone:
    action: set clamp_release = true
    effect: transfer from arm_head to cut_zone
```

解释：

- `raw_rack` 这个位置上放的是托盘
- `tray_a.slot[0]` 里放的是具体工件
- 第一段表示：机械手从托盘第 0 号槽位拿起一个工件
- 第二段表示：机械手把工件放到切割位

这里：

- `action` 是设备动作
- `effect` 是工件语义结果

这两层必须区分。

#### 第一版限制

- `from` 和 `to` 必须是已知位置或 holder
- 不支持运行时自由拼接目标

### 6.6 mount / unmount

#### 它是什么

`mount` 是把工件装到载具上。  
`unmount` 是把工件从载具上取下。

#### 为什么要有

否则你根本无法表达：

- 多根晶棒固定在铁板上
- 多个零件排在托盘上
- 一组工件跟随同一载具整体流转

#### 最小例子

```plc
step load_slot0:
    action: set clamp_a = true
    effect: mount rod on steel_plate.slot[0]

step unload_slot0:
    action: set clamp_a = false
    effect: unmount rod from steel_plate.slot[0] to cut_zone
```

解释：

- 第一步把一个 `rod` 挂到铁板的第 0 号槽位
- 第二步把它从铁板上取下并放到 `cut_zone`

#### 第一版限制

- slot 必须事先存在
- 一个工件未解绑前，不允许同时声明为自由工位占位

### 6.7 split

#### 它是什么

`split` 是一个工件分裂成多个工件。

#### 为什么要有

没有这个能力，你永远无法正式表达：

- 一根晶棒切成多片晶片
- 一块板材切成多个零件

而且只在 step 里临时写：

- `effect: split rod into slice`

还不够。

因为系统还需要在更上层先声明：

- `rod` 这种工件是否真的允许被分裂
- 它是否允许分裂成 `slice`

否则 step 里只是“写了一个动作效果”，但没有正式定义这是不是合法工艺关系。

#### 最小例子

```plc
[topology]
workpiece rod: workpiece_type {
    properties: [grade: enum(a, b)]
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    properties: [grade: enum(a, b)]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [scrapped]
    derived_from: [rod]
}

[tasks]
task cut:
    step do_cut:
        action: set saw_on = true
        effect: split rod into slice count 4 consumed
```

解释：

- 输入工件类型是 `rod`
- 输出工件类型是 `slice`
- `rod` 在类型层显式声明：允许切分成 `slice`
- `slice` 在类型层显式声明：它可以由 `rod` 派生而来
- 一次切割生成 4 个 `slice`
- 原来的 `rod` 在 split 之后被显式消耗掉
- 这里的“被消耗”不是走异常出口，而是工艺内身份终结

#### 第一版限制

- `count` 必须是编译期已知的有限整数
- 不支持无界 split
- split 后输入工件的命运必须写清楚
- `split` 必须同时满足两层条件：
- 类型层允许
- step 层显式触发

### 6.8 merge

#### 它是什么

`merge` 是多个工件变成一个新工件。

#### 为什么要有

没有这个能力，你无法正式表达：

- 多个零件组装成一个模块
- 多个电芯组装成一个模组

同样，第一版不建议只在 step 里临时写：

- `effect: merge [cell_a, cell_b] into module`

还应在类型层显式声明：

- `module` 是否接受由哪些输入类型组装而成

#### 最小例子

```plc
[topology]
workpiece cell: workpiece_type {
    properties: [ok: bool]
}

workpiece module: workpiece_type {
    properties: [sealed: bool]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    derived_from: [merge(cell, cell)]
}

[tasks]
task assemble:
    step merge_cells:
        action: set press_on = true
        effect: merge [cell_a, cell_b] into module consumed_inputs
```

解释：

- 输入是两个 `cell`
- 输出是一个 `module`
- `module` 在类型层显式声明：它允许由两个 `cell` 组装得到
- 输入工件在 merge 后被消耗

#### 第一版限制

- 输入集合必须有限且显式
- 不支持“随便来几个都能组装”的无界 merge
- `merge` 必须同时满足两层条件：
- 类型层允许
- step 层显式触发

### 6.9 lineage

#### 它是什么

`lineage` 是工件谱系。

#### 为什么要有

你需要能追问：

- 这片晶片来自哪根晶棒
- 这个模块由哪几个电芯组成

否则 split 和 merge 只是瞬时效果，没有可追踪性。

#### 最小例子

如果有：

- `rod_1` split 成 `slice_1`、`slice_2`、`slice_3`、`slice_4`

那么系统内部必须能保留类似关系：

- `rod_1 -> slice_1`
- `rod_1 -> slice_2`
- `rod_1 -> slice_3`
- `rod_1 -> slice_4`

又比如：

- `cell_a + cell_b -> module_1`

系统内部必须能保留：

- `cell_a -> module_1`
- `cell_b -> module_1`

#### 第一版限制

- 第一版只要求基本谱系关系
- 不支持任意深度任意复杂图分析

### 6.10 normal / abnormal terminal states

#### 它是什么

终态就是工件最后可以合法结束的状态。

但 WPM-v1 不建议只写一组总的终态，而是显式分成：

- `normal_terminal_states`
- `abnormal_terminal_states`

#### 为什么要有

因为工件不能“逻辑上消失”。

你必须明确：

- 最后它是完成了
- 报废了
- 被拒收了
- 还是被工艺显式消耗了

#### 最小例子

```plc
workpiece slice: workpiece_type {
    properties: [grade: enum(a, b)]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [scrapped]
}
```

解释：

- `slice` 这种工件最后合法终结时，只能是完成或报废
- 如果它以 `finished` 结束，后续只能走正常出口
- 如果它以 `scrapped` 结束，后续只能走异常出口

#### 第一版限制

- 终态是离散名字
- 不支持复杂终态表达式
- 正常终态与异常终态必须分开声明
- 正常终态只能对应正常出口
- 异常终态只能对应异常出口
- `consumed` 这类工艺内消耗，不通过 egress 建模

---

## 7. 第一版验证到底验证什么

这部分很关键，因为它决定这个功能是不是“真的值得做”。

WPM-v1 至少验证以下内容。

### 7.1 唯一性

同一个工件不能同时：

- 在两个 location
- 被两个 holder 拿着
- 既在 carrier 上又自由存在于别的位置

### 7.2 容量约束

例如：

- `cut_zone.capacity = 1`

那就不能同时放两个工件。

### 7.3 基本守恒

工件不能：

- 无中生有
- 无声消失

只有以下情况允许生命周期结束：

- 进入合法终态
- 被 split / merge / consume 显式处理

### 7.4 基本谱系合法性

例如：

- 一个 `slice` 必须来自某个 `rod`
- 一个 `module` 必须来自合法输入集合

### 7.5 基本载具一致性

如果一个工件挂在 carrier 上，那么：

- carrier 变化时，它的工艺位置也必须同步变化

这不要求做连续几何，只要求做离散状态一致性。

### 7.6 出入口契约一致性

如果某工件类型声明了：

- `normal_terminal_states`
- `abnormal_terminal_states`
- `ingress_sites`
- `normal_egress_sites`
- `abnormal_egress_sites`

那么 WPM-v1 应至少验证：

- 正常终态集合与正常出口集合必须成对存在，不能只声明一边
- 异常终态集合与异常出口集合必须成对存在，不能只声明一边
- 工件只能从已声明入口进入流程
- 工件只能从已声明正常出口或异常出口离开流程
- 正常终态只能通过正常出口离开流程
- 异常终态只能通过异常出口离开流程
- 不能出现终态类别与出口类别不一致
- 未声明出口一律视为非法出口
- 已声明出口若在模型中根本不可达，应报告建模错误或死契约

---

## 8. 为什么第一版是可实现的

这一节直接回答你的审查问题：这些功能是不是太大了。

答案是：

- 如果按无限系统做，确实太大
- 但如果按 V1 的有限约束做，是可实现的

原因是 V1 有这些强限制：

1. 工件数量有限
2. 位置数量有限
3. carrier 槽位有限
4. 属性离散
5. split / merge 有界
6. 不做连续几何

这意味着：

- parser 只是加有限声明和有限 effect 语法
- semantic 只是做有限校验
- IR 只是增加有限数据结构
- verification 仍然在有限状态空间工作
- runtime 只需要维护离散工件状态，不需要几何引擎

所以这件事是能落地的，但前提是必须守住 V1 边界。

---

## 9. 第一版建议的实现顺序

为了避免一次做太大，推荐按四阶段推进。

### Phase 1

先做：

- workpiece type
- location
- holder
- transfer
- 唯一性 / 容量 / 基本终态验证

### Phase 2

再做：

- carrier
- slot
- grid slot addressing
- mount / unmount
- 基本载具一致性

### Phase 3

再做：

- split
- merge
- basic lineage

### Phase 4

最后再做：

- classify
- rework / 回流
- foreach slot / scan order
- 更复杂批次语义

---

## 10. 最小例子总览

这一节把最重要的 4 类能力放在一起，方便你审。

### 10.1 单件搬运

```plc
[topology]
workpiece part: workpiece_type {
    properties: [inspected: bool]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    ingress_sites: [tray_a.slot[*]]
    normal_egress_sites: [outfeed]
    abnormal_egress_sites: [reject_bin]
}

holder arm: workpiece_holder { capacity: 1 }
location outfeed: workpiece_location { capacity: 1 }
location reject_bin: workpiece_location { capacity: 20 }
carrier tray_a: workpiece_carrier { slots: 4 }

[tasks]
task transfer_part:
    step pick:
        action: axis.move_absolute(axis_x, position: 10, speed: 5)
        effect: acquire holder arm from tray_a.slot[0]
        goto place

    step place:
        action: set clamp_release = true
        effect: transfer from arm to outfeed
```

这例子要证明的不是“轴动了”，而是：

- 工件从 `tray_a.slot[*]` 这类合法入口进入
- 进入 `arm`
- 再进入 `outfeed`
- 如果最终是 `finished`，它只能从 `outfeed` 离开
- 如果最终是 `rejected`，它只能从 `reject_bin` 离开
- 且它没有跑到未声明出口

### 10.2 split

```plc
[topology]
workpiece rod: workpiece_type {
    properties: [grade: enum(a, b)]
    allows: [split_into(slice)]
}

workpiece slice: workpiece_type {
    properties: [grade: enum(a, b)]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [scrapped]
    derived_from: [rod]
}

[tasks]
task cut:
    step do_cut:
        action: set saw_on = true
        effect: split rod into slice count 4 consumed
```

这例子要证明：

- `rod` 明确允许切分成 `slice`
- 一个 `rod` 可以切成 4 个 `slice`
- 原来的 `rod` 被工艺内显式消耗
- 4 个 `slice` 成为新工件

### 10.3 carrier

```plc
[topology]
workpiece rod: workpiece_type {
    properties: [angle_class: enum(left, center, right)]
    normal_terminal_states: [finished]
}

carrier steel_plate: workpiece_carrier { slots: 2 }

[tasks]
task load_plate:
    step mount_a:
        action: set clamp_a = true
        effect: mount rod on steel_plate.slot[0]
        goto mount_b

    step mount_b:
        action: set clamp_b = true
        effect: mount rod on steel_plate.slot[1]
        goto raise

    step raise:
        action: axis.move_relative(axis_z, distance: 5, speed: 2)
        effect: transform carrier steel_plate to frame cut_height
```

这例子要证明：

- 两个 `rod` 可以挂到同一块 `steel_plate` 上
- 抬升铁板时，挂在其上的工件一起进入新的离散工艺高度

### 10.4 merge

```plc
[topology]
workpiece cell: workpiece_type {
    properties: [ok: bool]
}

workpiece module: workpiece_type {
    properties: [sealed: bool]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    derived_from: [merge(cell, cell)]
}

[tasks]
task assemble:
    step merge_cells:
        action: set press_on = true
        effect: merge [cell_a, cell_b] into module consumed_inputs
```

这例子要证明：

- `module` 明确允许由两个 `cell` 组装得到
- 两个 `cell` 可以聚合成一个 `module`
- 输入件被消耗
- 输出件获得新身份

### 10.5 tray 扫描与 NG 分拣

```plc
[topology]
workpiece die: workpiece_type {
    properties: [
        inspect_result: enum(unknown, ok, ng)
    ]
    normal_terminal_states: [finished]
    abnormal_terminal_states: [rejected]
    ingress_sites: [tray_scan.slot[*, *]]
    normal_egress_sites: [tray_scan.slot[*, *]]
    abnormal_egress_sites: [ng_box]
}

carrier tray_scan: workpiece_carrier {
    layout: grid(rows: 32, cols: 24)
}

holder nozzle: workpiece_holder { capacity: 1 }
location ng_box: workpiece_location { capacity: 200 }

scan tray_scan by row_major
foreach slot in tray_scan by row_major
```

这例子要证明：

- 阵列 tray 可以显式建模成二维离散槽位
- 工件可以从 `slot[row, col]` 被逐个扫描
- 遍历顺序是模型的一部分，而不是隐藏在外部代码里
- NG 工件可以通过异常出口进入 `ng_box`

---

## 11. 你可以怎么审这份规范

如果你要快速判断这套东西值不值得做，可以只看下面 6 个问题。

1. RustPLC 是否需要正式表达“工件在什么位置”，而不仅是“设备做了什么动作”？
2. RustPLC 是否需要正式表达“一个工件变多个工件”？
3. RustPLC 是否需要正式表达“多个工件装成一个工件”？
4. RustPLC 是否需要正式表达“多个工件挂在同一载具上整体移动”？
5. 第一版是否必须限制在有限、离散、可验证范围内？
6. 如果某个功能不能进 IR 和 verification，是否就不应该算正式完成？

如果这 6 个问题里，你对前 4 个都答“是”，对后 2 个也答“是”，那么工件模型就是 RustPLC 合理且必要的下一步。

---

## 12. 结论

WPM-v1 不是为了让 DSL 看起来更复杂，而是为了让 RustPLC 第一次能正式建模：

- 对象如何进入系统
- 对象如何移动
- 对象如何加工
- 对象如何分裂
- 对象如何组装
- 对象如何跟随载具整体流转
- 对象如何合法结束

这套能力如果严格限制在：

- 有限对象
- 离散状态
- 有限载具
- 有界 split / merge
- 无连续几何依赖

那么它不是空想，而是一个可以逐层落到 parser / semantic / IR / verification / runtime 的真实工程能力。
