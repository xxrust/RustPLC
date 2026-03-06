# 步进/伺服电机 DSL 抽象设计深度分析报告

> **摘要**：本报告系统调研了 IEC 61131-3/PLCopen、CoDeSys SoftMotion、西门子博途 TIA Portal、Beckhoff TwinCAT、Rockwell Studio 5000 以及 CiA 402 等主流工业控制平台对步进电机与伺服电机的 DSL 层抽象方式，深入分析各方案的设计哲学、优缺点与适用场景，最终综合各家之长，提出一套可供参考的最优 DSL 抽象设计方案。

---

## 一、问题背景与核心挑战

在工业控制系统的 DSL（Domain-Specific Language）设计中，电机抽象是最核心也最困难的问题之一。其根本挑战在于：**步进电机与伺服电机在物理机制、控制方式、反馈能力上存在本质差异，但在应用层面，工程师希望用统一的语义来描述"让轴运动到某个位置"这一意图**。

| 维度 | 步进电机 | 伺服电机 |
|---|---|---|
| 控制方式 | 开环（脉冲计数） | 闭环（编码器反馈） |
| 位置反馈 | 无（或可选外部编码器） | 有（增量/绝对值编码器） |
| 通信接口 | 脉冲+方向（PTO）、UART | EtherCAT、PROFINET、CAN、模拟量 |
| 失步风险 | 存在（过载时丢步） | 不存在（闭环自动补偿） |
| 扭矩控制 | 不支持 | 支持 |
| 典型应用 | 低成本定位、3D 打印 | 高精度、高动态响应 |

这一差异要求 DSL 设计者在以下维度做出权衡：**统一性 vs. 表达力**、**简洁性 vs. 完备性**、**硬件无关性 vs. 硬件特性暴露**。

---

## 二、各平台抽象方案深度分析

### 2.1 PLCopen 运动控制标准（IEC 61131-3 基础）

PLCopen 是工业运动控制领域最重要的软件标准组织，其运动控制规范（Motion Control Function Blocks）建立在 IEC 61131-3 之上，定义了跨硬件平台的可复用运动控制应用库 [^1]。

#### 核心抽象：AXIS_REF

PLCopen 的核心设计决策是引入 `AXIS_REF` 这一**派生数据类型（Derived Data Type）**作为所有运动控制功能块的统一接口。其关键设计原则如下：

> "The data type AXIS_REF is a derived data type, provided by the hardware manufacturer. It is used as a VAR_IN_OUT parameter in all motion control function blocks. The user program does not need to know the internal structure of AXIS_REF."

`AXIS_REF` 通过 `VAR_IN_OUT` 方式传递，意味着同一个轴对象可以被多个功能块共享，每个功能块读取最新状态、写入指令后传递给下一个功能块。这一设计实现了**数据封装**与**硬件无关性**的统一。

#### 轴状态机（Axis State Machine）

PLCopen 定义了严格的 8 状态机，所有运动命令都基于状态转换：

```
Disabled → Standstill → Discrete Motion / Continuous Motion / Homing / Synchronized Motion
                      ↕
                   Stopping
                      ↕
                  ErrorStop（最高优先级，任何状态均可进入）
```

状态机的引入是 PLCopen 最重要的贡献之一：它将电机控制从"发送命令"的命令式范式，转变为"描述轴应处于何种状态"的声明式范式，使得行为可预测、可验证。

#### 标准功能块集合

PLCopen Part 1 定义了完整的单轴功能块集合，Part 4 定义了多轴协调功能块：

| 类别 | 代表功能块 | 语义 |
|---|---|---|
| 使能管理 | `MC_Power`, `MC_Reset` | 轴使能/去使能、错误复位 |
| 运动控制 | `MC_MoveAbsolute`, `MC_MoveRelative`, `MC_MoveVelocity` | 绝对/相对位置运动、速度控制 |
| 停止控制 | `MC_Stop`, `MC_Halt` | 受控停止（进入 Stopping/Standstill 状态） |
| 回零 | `MC_Home` | 参考点建立 |
| 同步 | `MC_GearIn`, `MC_CamIn` | 电子齿轮、电子凸轮 |
| 参数访问 | `MC_ReadParameter`, `MC_WriteParameter` | 运行时参数读写 |

**PLCopen 的核心优势**在于其**标准化程度最高**，同一套应用代码可以在任何兼容 PLCopen 的平台上运行。其**核心局限**在于：`AXIS_REF` 的内部结构由厂商定义，标准本身不规定参数结构，导致跨平台移植时仍需适配。

---

### 2.2 CoDeSys SoftMotion

CoDeSys（3S-Smart Software Solutions）是最广泛使用的 PLC 开发平台之一，其 SoftMotion 库在 PLCopen 基础上进行了重要的架构创新 [^2]。

#### 核心创新：AXIS_REF_SM3 功能块（非结构体）

CoDeSys 最重要的设计决策是将 `AXIS_REF` 实现为**功能块（Function Block）**而非数据结构。`AXIS_REF_SM3` 实现了 `IAxisRef` 接口，这一选择带来了面向对象的能力：

```
AXIS_REF_SM3 (基础物理轴)
├── AXIS_REF_VIRTUAL_SM3 (虚拟轴/仿真轴)
├── AXIS_REF_LOGICAL_SM3 (逻辑轴)
├── AXIS_REF_MAPPING_SM3 (映射轴)
└── ENCODER_REF (只读编码器轴)
```

这一继承体系使得**不同类型的轴可以通过多态性被统一处理**，同时保留了各自的特殊行为。

#### 分层架构设计

CoDeSys SoftMotion 的分层架构是其最大优势：

```
用户应用层 (PLCopen MC 指令)
    ↕
SM3_Basic 库层 (标准 FB + 附加 FB + 帮助函数)
    ↕
驱动接口层 (Drive Interface - 特定驱动库)
    ↕
底层连接层 (CiA402 / SoE / 虚拟驱动)
    ↕
RTS I/O 层 (EtherCAT / CANopen / 本地 I/O)
```

**驱动接口层**是 CoDeSys 的精华所在：每种驱动器（EtherCAT 伺服、步进电机、模拟量驱动）都有对应的驱动库，这些库通过继承 `AXIS_REF_SM3` 并重载驱动相关方法来实现适配，**应用层代码完全不需要改变**。

#### AXIS_REF_SM3 的参数体系

`AXIS_REF_SM3` 包含了极为丰富的参数字段，覆盖了从物理量到控制参数的完整范围：

- **运动参数**：`fSetPosition`/`fActPosition`、`fSetVelocity`/`fActVelocity`、`fSetAcceleration`、`fSetJerk`
- **扭矩/电流参数**：`fSetTorque`/`fActTorque`、`fSetCurrent`/`fActCurrent`
- **比例因子**：`fScalefactor`、`fFactorVel`、`fFactorAcc`、`fFactorTor`（支持任意单位系统）
- **控制模式**：`byControllerMode`（位置/速度/扭矩切换）
- **斜坡类型**：`eRampType`（梯形/S形/sin²）

**CoDeSys 的核心优势**在于其**架构最开放、可扩展性最强**，适合需要支持多种硬件平台的通用 DSL 设计。其**局限**在于参数体系过于庞大，对于简单应用场景存在认知负担。

---

### 2.3 西门子博途 TIA Portal

西门子博途采用了与 PLCopen/CoDeSys 截然不同的设计哲学，其核心抽象是**工艺对象（Technology Object, TO）** [^3]。

#### 核心设计：工艺对象 = 配置 + 运行时数据的统一数据块

博途的工艺对象不是功能块，而是一个特殊的**数据块（Data Block）实例**，它同时承载：
- **静态配置参数**（编译时确定，通过图形化向导配置）
- **运行时状态数据**（实时更新，可直接读取）
- **诊断信息**（错误码、警告信息）

这一设计的核心洞见是：**配置与状态本质上是同一个对象的不同时态**，将其统一在一个数据块中，使得工程师可以通过单一引用访问轴的全部信息。

#### 工艺对象类型层次

博途定义了完整的工艺对象类型层次，按功能复杂度递增：

| 工艺对象类型 | 功能 | 继承关系 |
|---|---|---|
| `TO_SpeedAxis` | 速度控制 | 基础 |
| `TO_PositioningAxis` | 位置+速度控制 | 扩展 SpeedAxis |
| `TO_SynchronousAxis` | 同步+位置+速度 | 扩展 PositioningAxis |
| `TO_ExternalEncoder` | 只读位置测量 | 独立 |
| `TO_Kinematics` | 多轴运动学 | 独立 |

#### TO_PositioningAxis 参数结构

工艺对象的参数结构极为系统化，分为以下主要部分：

```
TO_PositioningAxis
├── Actor (执行器/驱动器配置)
│   ├── DriveParameter (参考速度、最大速度)
│   └── Interface (PROFIdrive / PTO / 模拟量)
├── Encoder (编码器配置)
│   ├── Type (增量/绝对值)
│   └── Resolution, Range
├── Mechanics (机械参数)
│   ├── Ratio (传动比)
│   └── LeadScrew (丝杠导程)
├── DynamicDefaults (动态默认值)
│   └── Velocity, Acceleration, Deceleration, Jerk
├── DynamicLimits (动态限制)
├── PositionLimits (位置限制/软限位)
├── Homing (回零配置)
└── StatusBits (运行时状态，只读)
```

#### 步进/伺服统一的实现方式

博途实现步进/伺服统一的关键在于**驱动接口类型的配置化**：

- **PTO（Pulse Train Output）**：步进电机接口，配置脉冲当量（每转脉冲数）
- **PROFIdrive**：伺服驱动器接口（SINAMICS 等），通过 PROFINET 连接
- **模拟量接口**：±10V 速度给定

**无论底层接口类型如何，上层 MC 指令完全相同**。这是博途最重要的设计原则。

**博途的核心优势**在于**图形化配置体验最佳**，工程师无需手写参数，通过向导即可完成复杂配置。其**局限**在于强绑定西门子生态，跨平台移植困难。

---

### 2.4 Beckhoff TwinCAT

Beckhoff TwinCAT 采用了**NC 任务独立运行**的架构，将运动控制从 PLC 逻辑中分离出来 [^4]。

#### 轴类型体系

TwinCAT 定义了以下轴类型，通过配置选择：

| 轴类型 | 描述 | 编码器 | 典型驱动 |
|---|---|---|---|
| Continuous axis（伺服轴） | 标准闭环伺服 | 有 | EtherCAT 伺服 |
| Encoder axis（虚拟轴） | 只读位置 | 有 | 无驱动 |
| High/Low speed axis | 双速切换 | 可选 | 双速电机 |
| Stepper motor axis | 步进电机 | 无（开环） | 步进驱动器 |
| Low cost stepper axis | 低成本步进 | 无（仿真） | 数字 I/O |

#### 轴对象的组件化设计

TwinCAT NC 轴对象由独立组件构成，各组件可独立配置：

```
NC Axis Object
├── Axis (参数、状态)
├── Drive (驱动接口 - EtherCAT/SERCOS/模拟量)
├── Encoder (编码器接口 - EtherCAT/SSI/增量)
└── Controller (控制器参数 - P/I/D/前馈)
```

这一**组件化设计**使得驱动器和编码器可以独立更换，而不影响轴的其他配置，体现了**关注点分离（Separation of Concerns）**的设计原则。

#### AXIS_REF 与 NC 任务的关系

在 PLC 程序中，轴通过 `AXIS_REF` 结构体引用，其内部包含 `PlcToNc`（PLC→NC 方向）和 `NcToPlc`（NC→PLC 方向）两个子结构，体现了**双向数据流**的设计。NC 任务在独立的实时周期中运行，与 PLC 任务解耦，保证了运动控制的实时性。

**TwinCAT 的核心优势**在于**组件化架构最灵活**，EtherCAT 生态支持最完善。其**局限**在于 NC 任务与 PLC 任务的分离增加了系统复杂度。

---

### 2.5 Rockwell Studio 5000

Rockwell 采用了**轴数据类型（Axis Data Type）**的设计方式，通过不同的数据类型区分不同类型的轴 [^5]。

#### 轴数据类型体系

| 数据类型 | 描述 | 驱动接口 |
|---|---|---|
| `AXIS_SERVO` | 伺服轴（SERCOS/模拟量） | 旧式 SERCOS |
| `AXIS_SERVO_DRIVE` | 伺服驱动轴 | EtherNet/IP（Kinetix） |
| `AXIS_GENERIC` | 通用轴 | 第三方驱动 |
| `AXIS_VIRTUAL` | 虚拟轴 | 无 |
| `AXIS_CONSUMED` | 消耗轴 | 跨控制器 |

#### 运动指令体系

Rockwell 使用自己的运动指令集（与 PLCopen 功能相似但命名不同）：`MSO/MSF`（使能/去使能）、`MAM/MAR/MAJ`（绝对/相对/点动）、`MAH`（回零）、`MAS`（停止）、`MGS`（齿轮同步）。

**Rockwell 的核心优势**在于与 Kinetix 驱动器的深度集成，EtherNet/IP 生态完善。其**局限**在于 PLCopen 兼容性较低，跨平台移植困难。

---

### 2.6 CiA 402（DS402）底层标准

CiA 402 是 CAN in Automation 组织制定的**驱动器设备配置文件**，是上述所有平台的底层基础 [^6]。

#### 核心贡献：统一的驱动器状态机

CiA 402 定义了所有驱动器（无论步进、伺服还是变频器）必须遵循的状态机：

```
NOT READY TO SWITCH ON
  → SWITCH ON DISABLED
    → READY TO SWITCH ON
      → SWITCHED ON
        → OPERATION ENABLE（正常运行）
```

#### 操作模式（Modes of Operation）

CiA 402 通过对象 `0x6060`（Modes of Operation）统一了不同控制模式：

| 模式值 | 模式名称 | 适用场景 |
|---|---|---|
| 1 | Profile Position Mode | 轮廓位置控制 |
| 3 | Profile Velocity Mode | 轮廓速度控制 |
| 4 | Profile Torque Mode | 轮廓扭矩控制 |
| 6 | Homing Mode | 回零 |
| 8 | Cyclic Sync Position (CSP) | 周期同步位置（EtherCAT 高性能） |
| 9 | Cyclic Sync Velocity (CSV) | 周期同步速度 |
| 10 | Cyclic Sync Torque (CST) | 周期同步扭矩 |

CiA 402 的核心价值在于：它将**驱动器的行为标准化**，使得上层 DSL 可以通过统一的接口控制不同厂商的驱动器。

---

## 三、横向对比分析

![各平台电机抽象能力对比雷达图](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/sBj7qbYJDbheLWEwj4FOgK-images_1772554121490_na1fn_L2hvbWUvdWJ1bnR1L3Jlc2VhcmNoL2NoYXJ0X3JhZGFyX2NvbXBhcmlzb24.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L3NCajdxYllKRGJoZUxXRXdqNEZPZ0staW1hZ2VzXzE3NzI1NTQxMjE0OTBfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwzSmxjMlZoY21Ob0wyTm9ZWEowWDNKaFpHRnlYMk52YlhCaGNtbHpiMjQucG5nIiwiQ29uZGl0aW9uIjp7IkRhdGVMZXNzVGhhbiI6eyJBV1M6RXBvY2hUaW1lIjoxNzk4NzYxNjAwfX19XX0_&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=r9BaW5mJrBcZemLxR-oJvlDy3d88Uc3UdcflOfuq3-ptUoDaJvTTRe3NiAglkyDF6xTlD8LhLIBNKuyDlQNU82c3NMVA0VKCE5AUTh9id53f9eoBQmwJ7cl4fnY3zVhzt9jGF7iMm90Pbi1rEN6veQmiNTuViYMcs6ezXTrZSiN26G2RoHXb2inzbEhDwQFWGCk8-KoBFN3qbxnDIvOFVNtHhLnT6XGoe4szDwS0re8Sn563~hMw~Wc2FsRvMym8PGXrX-jlkR7qcgNhvdV1KDzZ8v2gcBdbgo9uHGIi~1TKDUp~KnUHljth6PCK~bj9up4zU5MBdfD4KHCA7QyGMA__)

![各平台电机抽象机制对比](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/sBj7qbYJDbheLWEwj4FOgK-images_1772554121490_na1fn_L2hvbWUvdWJ1bnR1L3Jlc2VhcmNoL2NoYXJ0X2NvbXBhcmlzb25fdGFibGU.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L3NCajdxYllKRGJoZUxXRXdqNEZPZ0staW1hZ2VzXzE3NzI1NTQxMjE0OTBfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwzSmxjMlZoY21Ob0wyTm9ZWEowWDJOdmJYQmhjbWx6YjI1ZmRHRmliR1UucG5nIiwiQ29uZGl0aW9uIjp7IkRhdGVMZXNzVGhhbiI6eyJBV1M6RXBvY2hUaW1lIjoxNzk4NzYxNjAwfX19XX0_&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=Zjog~UOTHd34fJXt6lvkBQbul~FXzi95dSgjP~uXHmdEHlQwe7SWnU8deNh204W~ddVOzakOnKAULw6kKgh2PJhM8ph9f4Lq~pEXFEdSD9Cj9lSZkt8~5f33qO0EdIXAWd30rt2--dMZdNexTng9jqpQS-yKfHEm0wfuvqN4Jwo94b5~yPO7dznirqBkqnmCinRpICt3~e8P09261jnKTy0qQ3jrnp6WEUzg8aNwthexyVq1B0fTaqjpHePz-NstY4x9Y4dbqkgPBRlDeGo6B2GgPfCFGa~YdXqwdcV91zkIG~3EQHCqYEo5RHl9sXEAV4eN7WL7Bx4u69QnyUlqvw__)

### 3.1 轴抽象载体的选择

各平台在"用什么来表示一个轴"这一问题上做出了不同选择：

| 选择 | 代表平台 | 优势 | 劣势 |
|---|---|---|---|
| **结构体/派生类型** | PLCopen 标准、TwinCAT | 轻量、简单、内存友好 | 无法封装行为，扩展性差 |
| **功能块（FB）** | CoDeSys SoftMotion | 支持 OOP 继承、封装行为 | 实例化开销，复杂度较高 |
| **数据块（DB）** | 博途 TIA Portal | 配置与状态统一、图形化友好 | 绑定平台，不可移植 |
| **数据类型（Tag）** | Rockwell Studio 5000 | 直接属性访问、IDE 集成好 | 类型繁多，PLCopen 兼容差 |

### 3.2 步进/伺服统一的实现策略

| 策略 | 实现方式 | 代表平台 | 评价 |
|---|---|---|---|
| **驱动适配层** | 不同驱动器实现相同接口 | CoDeSys（驱动库继承） | 最灵活，最彻底 |
| **配置参数化** | 同一对象通过配置区分驱动类型 | 博途（PTO/PROFIdrive 配置） | 用户体验最好 |
| **轴类型枚举** | 不同类型轴使用不同类型标识 | TwinCAT（步进轴/伺服轴） | 清晰但不够统一 |
| **数据类型区分** | 不同类型轴使用不同数据类型 | Rockwell（AXIS_SERVO_DRIVE 等） | 类型安全但冗余 |

### 3.3 各平台的核心设计哲学总结

- **PLCopen**：**标准优先**——定义最小公约数，让厂商自由实现，保证跨平台可移植性
- **CoDeSys**：**架构优先**——通过清晰的分层和 OOP 继承，实现最大灵活性和可扩展性
- **博途**：**体验优先**——通过工艺对象和图形化向导，最大化工程效率，降低使用门槛
- **TwinCAT**：**性能优先**——通过 NC 任务独立运行和组件化设计，实现最高实时性
- **Rockwell**：**生态优先**——深度集成 Kinetix 生态，在封闭生态内提供最佳体验

---

## 四、推荐的最优 DSL 抽象设计方案

综合以上分析，推荐采用**"取长补短"**的设计策略，核心思路是：**以 PLCopen 状态机为行为规范，以 CoDeSys 的分层架构为骨架，借鉴博途的参数结构化思想，融入 CiA 402 的操作模式体系**。

### 4.1 总体架构：五层分离

![推荐的电机抽象 DSL 分层架构](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/sBj7qbYJDbheLWEwj4FOgK-images_1772554121490_na1fn_L2hvbWUvdWJ1bnR1L3Jlc2VhcmNoL2NoYXJ0X2FyY2hpdGVjdHVyZQ.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L3NCajdxYllKRGJoZUxXRXdqNEZPZ0staW1hZ2VzXzE3NzI1NTQxMjE0OTBfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwzSmxjMlZoY21Ob0wyTm9ZWEowWDJGeVkyaHBkR1ZqZEhWeVpRLnBuZyIsIkNvbmRpdGlvbiI6eyJEYXRlTGVzc1RoYW4iOnsiQVdTOkVwb2NoVGltZSI6MTc5ODc2MTYwMH19fV19&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=ZbJ~B5LhEtmS-bKwZZ~wt~IBlKpxHYMIDXERGrKc4xwqfK5kg-JLOpZfU0LDMEW9BdiJ4RrwIAEIcdNyRiuMPXN6DAfck6kKznN~dshXCJy3ka2OrdpfuxFnPCbRJUTHQs5YAzoNWMMrd1VChNIX7TxP~uGRgbzRYo2CcgEdCH-GnJHoLRkrhOvNEvh~5rcc67sF8-H-Lp6qwrTLarmfLPzv08mIUbBqadZTAmOy0piam56tOQHXQg~N3L8yqyhgGwxq189dn55b~z3Bad8PrtzL5sI0tw8HbatBx27fhi8VgKjA6tZkvW7fsuv2yhx2NEbyZIWRIIj3j2YxiZiwSA__)

```
L5: 用户应用层 ─── MC_MoveAbsolute / MC_GearIn / MC_CamIn
L4: 运动序列层 ─── 状态机 / 命令队列 / Blending 模式
L3: 轴抽象层   ─── Axis 对象（核心 DSL 层）★
L2: 驱动适配层 ─── PulseAdapter / ServoAdapter / AnalogAdapter
L1: 硬件接口层 ─── EtherCAT / 脉冲+方向 / 模拟量 / UART
```

**关键设计原则**：L3（轴抽象层）是核心边界，步进电机与伺服电机的差异在 L2 完全消化，L3 及以上完全感知不到底层差异。

### 4.2 核心：Axis 对象设计

#### 4.2.1 轴类型体系（借鉴博途的类型层次 + CoDeSys 的继承体系）

```
Axis (抽象基类)
├── PositioningAxis      // 定位轴：位置+速度控制（最常用）
│   └── SynchronousAxis  // 同步轴：继承定位轴+电子齿轮/凸轮
├── SpeedAxis            // 速度轴：仅速度控制（变频器场景）
├── VirtualAxis          // 虚拟轴：无物理驱动，用于仿真/主轴
└── EncoderAxis          // 编码器轴：只读位置（外部测量）
```

**设计原则**：轴类型通过**继承**而非配置参数区分，类型层次清晰，功能集合明确（子类型包含父类型的全部功能）。

#### 4.2.2 Axis 对象的参数结构（借鉴博途的结构化思想）

```
PositioningAxis {
  // ── 身份信息 ──────────────────────────────
  id:          String         // 轴唯一标识符
  name:        String         // 人类可读名称
  axisType:    AxisType       // 轴类型枚举

  // ── 驱动器配置（L2 适配层接口）─────────────
  drive: {
    adapter:   DriveAdapter   // 驱动适配器（注入，不暴露类型）
    // 以下由适配器填充，对用户只读
    maxVelocity:    Float     // 驱动器最大速度（工程单位）
    referenceSpeed: Float     // 参考速度（用于归一化）
  }

  // ── 编码器配置 ────────────────────────────
  encoder: {
    type:       EncoderType   // Incremental / Absolute / None（步进开环）
    resolution: Float         // 分辨率（脉冲/转 或 位/转）
    // 以下由适配器填充
    actualPosition: Float     // 实际位置（工程单位，只读）
  }

  // ── 机械参数 ──────────────────────────────
  mechanics: {
    ratio:      Fraction      // 传动比（分子/分母）
    leadScrew:  Float?        // 丝杠导程（mm/转，线性轴）
    backlash:   Float?        // 反向间隙补偿
    isRotary:   Bool          // 旋转轴 / 线性轴
    modulo:     Float?        // 模运动范围（旋转轴可选）
  }

  // ── 动态参数（默认值与限制）──────────────────
  dynamics: {
    defaultVelocity:     Float   // 默认速度
    defaultAcceleration: Float   // 默认加速度
    defaultDeceleration: Float   // 默认减速度
    defaultJerk:         Float?  // 默认加加速度（S形曲线）
    maxVelocity:         Float   // 最大速度限制
    maxAcceleration:     Float   // 最大加速度限制
    emergencyDecel:      Float   // 急停减速度
    rampType:            RampType // Trapezoidal / SCurve / Sin2
  }

  // ── 位置限制 ──────────────────────────────
  limits: {
    softLimitPositive: Float?    // 正向软限位
    softLimitNegative: Float?    // 负向软限位
    hwLimitPositive:   IORef?    // 正向硬限位开关
    hwLimitNegative:   IORef?    // 负向硬限位开关
  }

  // ── 回零配置 ──────────────────────────────
  homing: {
    mode:           HomingMode   // Active / Passive / Absolute
    velocity:       Float
    direction:      Direction    // Positive / Negative
    referenceOffset: Float       // 参考点偏移
    homingSwitch:   IORef?       // 回零开关（主动回零）
  }

  // ── 运行时状态（只读，由运行时填充）──────────
  status: {
    state:          AxisState    // PLCopen 8状态
    actualPosition: Float        // 实际位置
    actualVelocity: Float        // 实际速度
    actualTorque:   Float?       // 实际扭矩（伺服有效）
    isHomed:        Bool         // 已回零
    errorCode:      ErrorCode?   // 错误码
  }
}
```

**关键设计决策**：
1. `drive.adapter` 是注入的接口，不暴露具体类型（步进/伺服对上层透明）
2. `encoder.type = None` 表示步进电机开环模式，系统自动处理
3. `status` 字段全部只读，由运行时系统填充，不允许用户直接写入

#### 4.2.3 驱动适配层接口（借鉴 CoDeSys 的驱动接口分离）

```
interface DriveAdapter {
  // 初始化与使能
  initialize(config: AxisConfig): Result
  enable(): Result
  disable(): Result

  // 运动控制（L3 调用 L2 的统一接口）
  setTargetVelocity(velocity: Float): Result
  setTargetPosition(position: Float): Result   // CSP 模式
  setTargetTorque(torque: Float): Result        // CST 模式（伺服专有）

  // 状态读取
  getActualPosition(): Float
  getActualVelocity(): Float
  getActualTorque(): Float?
  getDriveStatus(): DriveStatus

  // 模式切换（对应 CiA 402 操作模式）
  setOperationMode(mode: OperationMode): Result
}

// 具体实现（对 L3 完全透明）
class PulseAdapter implements DriveAdapter { ... }   // 步进电机
class EtherCATServoAdapter implements DriveAdapter { ... }  // EtherCAT 伺服
class AnalogAdapter implements DriveAdapter { ... }  // 模拟量伺服
class VirtualAdapter implements DriveAdapter { ... } // 仿真/虚拟轴
```

### 4.3 轴状态机（借鉴 PLCopen，完整保留）

![推荐 DSL 的轴状态机](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/sBj7qbYJDbheLWEwj4FOgK-images_1772554121490_na1fn_L2hvbWUvdWJ1bnR1L3Jlc2VhcmNoL2NoYXJ0X3N0YXRlX21hY2hpbmU.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L3NCajdxYllKRGJoZUxXRXdqNEZPZ0staW1hZ2VzXzE3NzI1NTQxMjE0OTBfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwzSmxjMlZoY21Ob0wyTm9ZWEowWDNOMFlYUmxYMjFoWTJocGJtVS5wbmciLCJDb25kaXRpb24iOnsiRGF0ZUxlc3NUaGFuIjp7IkFXUzpFcG9jaFRpbWUiOjE3OTg3NjE2MDB9fX1dfQ__&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=FGu9NwaVoAFHfRI3NNzrRtJJF7b4BtOmvTvvqQAqIhDsCv3eHEamet0fh15MLqGhQX-5nkcB6dX1Eu3vMBMZqKf1a9TFVBKg~-LQS4Xvh4ZGs9flqdDgDD7mwF-B1B85DUQm5HEzNvcArBI5QVUeYnmQtNS3D1qteWuvcOcwgjepKpK7fV25nwkdwNx8itZ1osGMDY-3XT5k2jLBwA~iSyPKVcD9cg-mNUM1YiF4wONoPSW40-JSnh1Bbf6kSAsP3PPtTLWx0pWtc44-RqLtvcpDHq2WLvw~cPWcqqrgXf28ellPd0mA5Xx0zaOfSoeXDOo0fRn~y5OhvBmFbDSMhA__)

轴状态机是 DSL 行为规范的核心，**完整保留 PLCopen 的 8 状态设计**，理由如下：

1. **可预测性**：状态机使得所有命令的前置条件和后置状态都是确定的
2. **安全性**：`ErrorStop` 状态具有最高优先级，任何故障都会强制进入
3. **标准化**：与 PLCopen 兼容，降低工程师学习成本
4. **可验证性**：状态机可以被形式化验证，符合工业安全要求

### 4.4 运动命令接口（借鉴 PLCopen 命名规范）

```
// 使能管理
axis.power(enable: Bool) -> Command
axis.reset() -> Command

// 基础运动
axis.moveAbsolute(position, velocity?, acceleration?, deceleration?, jerk?) -> Command
axis.moveRelative(distance, velocity?, acceleration?, deceleration?, jerk?) -> Command
axis.moveVelocity(velocity, acceleration?, deceleration?) -> Command
axis.halt(deceleration?) -> Command
axis.stop(deceleration?) -> Command   // 进入 Stopping 状态，需 reset 才能继续

// 回零
axis.home(mode?, velocity?, direction?) -> Command

// 多轴同步（SynchronousAxis 专有）
axis.gearIn(master: Axis, ratio: Fraction, masterOffset?, slaveOffset?) -> Command
axis.gearOut(deceleration?) -> Command
axis.camIn(master: Axis, camTable: CamTable, masterOffset?, slaveOffset?) -> Command
axis.camOut(deceleration?) -> Command

// 参数访问
axis.setOverride(velocityFactor, accelerationFactor) -> Command
axis.setPosition(position) -> Command  // 坐标系重置，不产生运动
```

**关键设计**：所有运动命令返回 `Command` 对象，`Command` 包含：
- `done: Bool`：命令完成标志
- `busy: Bool`：命令执行中标志
- `error: Bool`：命令错误标志
- `errorCode: ErrorCode?`：错误码
- `abort()`: 中止命令的方法

这与 PLCopen 功能块的 `Execute/Done/Busy/Error` 模式完全对应，但以更现代的对象接口表达。

### 4.5 单元系统（借鉴 CoDeSys 的比例因子思想）

```
// 轴的工程单位在创建时声明，之后所有参数均使用工程单位
axis.units = {
  position:     "mm",          // 位置单位
  velocity:     "mm/s",        // 速度单位
  acceleration: "mm/s²",       // 加速度单位
  torque:       "Nm"?          // 扭矩单位（伺服有效）
}

// 内部自动完成单位转换：工程单位 ↔ 电机脉冲/编码器计数
// 用户永远只看到工程单位
```

### 4.6 步进电机特殊处理策略

步进电机的核心问题是**开环运行时的失步风险**。推荐的处理策略：

1. **默认开环**：`encoder.type = None` 时，系统使用内部位置计数器（仿真编码器），`status.actualPosition` 反映指令位置而非真实位置
2. **可选闭环**：配置外部编码器后，系统自动切换为闭环模式，`status.actualPosition` 反映真实位置
3. **失步检测**（可选）：在有外部编码器的情况下，系统可检测指令位置与实际位置的偏差，超过阈值时触发 `ErrorStop`
4. **透明性**：上层应用代码**不需要**知道轴是步进还是伺服，失步检测是底层行为

```
// 步进电机配置示例
PositioningAxis stepper {
  drive: {
    adapter: PulseAdapter {
      pulsePerRev: 3200,       // 每转脉冲数（16细分×200步）
      maxFrequency: 200000,    // 最大脉冲频率 Hz
      direction: DirectionPin  // 方向信号引脚
    }
  }
  encoder: {
    type: None                 // 开环模式
    // 或者：
    // type: Incremental
    // resolution: 4000        // 外部编码器，闭环模式
  }
  mechanics: {
    leadScrew: 10.0            // 丝杠导程 10mm/转
  }
  units: { position: "mm", velocity: "mm/s" }
}

// 伺服电机配置示例（上层代码与步进完全相同）
PositioningAxis servo {
  drive: {
    adapter: EtherCATServoAdapter {
      nodeAddress: 1,
      driveType: "OMRON_1S"
    }
  }
  encoder: {
    type: Absolute             // 绝对值编码器（自动配置）
  }
  mechanics: {
    leadScrew: 10.0
  }
  units: { position: "mm", velocity: "mm/s" }
}

// 上层应用代码：步进和伺服完全相同
stepper.moveAbsolute(position: 100.0, velocity: 50.0)  // 移动到 100mm
servo.moveAbsolute(position: 100.0, velocity: 50.0)    // 完全相同的代码
```

### 4.7 各平台优秀特性的取舍总结

| 特性 | 来源平台 | 采纳理由 |
|---|---|---|
| PLCopen 8 状态机 | PLCopen | 行为标准化、可验证、安全 |
| 驱动适配层分离 | CoDeSys SoftMotion | 最彻底的硬件无关性 |
| 参数结构化分组 | 博途 TIA Portal | 清晰、易于图形化配置 |
| 轴类型继承体系 | 博途 + CoDeSys | 功能集合清晰，类型安全 |
| 组件化（Drive/Encoder 分离） | Beckhoff TwinCAT | 关注点分离，独立替换 |
| 操作模式体系 | CiA 402 | 与驱动器标准对齐 |
| Command 对象返回 | 现代 API 设计 | 异步友好，状态可查询 |
| 工程单位系统 | CoDeSys 比例因子 | 用户只关心物理量 |

---

## 五、设计中需要特别注意的问题

### 5.1 步进电机开环的哲学问题

步进电机开环运行时，`status.actualPosition` 的语义是**指令位置**而非**物理位置**。这一语义差异必须在 DSL 文档中明确说明，并在配置中通过 `encoder.type = None` 显式声明，避免用户误解。

### 5.2 扭矩控制的条件性

扭矩控制（`moveVelocity` 的扭矩限制、`setTargetTorque`）仅对伺服电机有效。推荐通过**运行时检查**而非**类型系统**来处理：

```
// 运行时检查，而非编译时类型区分
axis.setTorqueLimit(torque: 5.0)  // 步进电机调用此方法时，返回 NotSupported 错误
```

这避免了为步进和伺服创建不同类型导致的代码分叉问题。

### 5.3 回零的多样性

不同平台的回零模式差异极大（PLCopen 定义了多种，CiA 402 定义了 30+ 种回零例程）。推荐定义**有限的、常用的回零模式**，并提供扩展点：

```
enum HomingMode {
  Active,        // 主动回零：运动到限位开关
  Passive,       // 被动回零：等待外部触发
  Absolute,      // 绝对值编码器：直接读取绝对位置
  SetPosition,   // 直接设定当前位置为参考点（无运动）
  Custom(fn)     // 扩展点：用户自定义回零逻辑
}
```

### 5.4 多轴协调的边界

多轴协调（电子齿轮、电子凸轮）应该是 `SynchronousAxis` 的专有功能，而非 `PositioningAxis` 的可选功能。这一设计决策来自博途的类型层次，其好处是：**类型系统在编译时就能阻止对非同步轴调用同步指令**，而非在运行时报错。

### 5.5 形式化验证友好性

工业控制 DSL 的一个重要需求是**可形式化验证**。推荐的状态机设计（完整的 PLCopen 8 状态机）天然支持形式化验证：每个状态的前置条件和后置状态都是确定的，可以用模型检验工具（如 TLA+、SPIN）验证状态机的正确性。任何新引入的语法扩展都必须能够被准确地形式化验证。

---

## 六、结论

通过对六个主流工业控制平台的深度调研，可以得出以下核心结论：

**没有一个平台在所有维度上都是最优的**。PLCopen 标准化程度最高但参数结构不规范；CoDeSys 架构最开放但认知负担较重；博途用户体验最好但平台绑定严重；TwinCAT 性能最强但系统复杂度高；Rockwell 生态集成最好但标准化程度低。

**最优的 DSL 设计应当取长补短**：以 PLCopen 状态机为行为规范（保证可预测性和可验证性），以 CoDeSys 的驱动适配层为架构骨架（保证硬件无关性），借鉴博途的参数结构化分组（保证配置清晰性），融入 TwinCAT 的组件化设计（保证关注点分离），以 CiA 402 的操作模式体系为底层对齐基础（保证与驱动器标准的互操作性）。

**步进/伺服统一的关键**在于：在驱动适配层（L2）彻底消化硬件差异，在轴抽象层（L3）提供统一的接口，使上层应用代码对底层电机类型完全透明。步进电机的开环特性通过 `encoder.type = None` 的配置声明来处理，而非通过不同的类型系统来区分。

---

## 参考文献

[^1]: PLCopen Technical Committee 2, "Motion Control Function Blocks for IEC 61131-3," PLCopen, https://www.plcopen.org/standards/motion-control/

[^2]: 3S-Smart Software Solutions, "CODESYS SoftMotion Overview," CODESYS Online Help, https://content.helpme-codesys.com/en/CODESYS%20SoftMotion/_sm_components.html

[^3]: Siemens AG, "S7-1500/S7-1500T Motion Control Overview V5.0 in TIA Portal V16," Siemens Industry Support, https://support.industry.siemens.com/cs/attachments/109766459/

[^4]: Beckhoff Automation, "TwinCAT NC Axes," Beckhoff Information System, https://infosys.beckhoff.com/content/1033/tcncgeneral/3447752587.html

[^5]: Rockwell Automation, "Logix5000 Controllers Motion Instructions Reference Manual," Literature Library, https://literature.rockwellautomation.com/idc/groups/literature/documents/rm/motion-rm002_-en-p.pdf

[^6]: CAN in Automation (CiA), "CiA 402 Series: CANopen Device Profile for Drives and Motion Control," https://www.can-cia.org/can-knowledge/cia-402-series-canopen-device-profile-for-drives-and-motion-control
