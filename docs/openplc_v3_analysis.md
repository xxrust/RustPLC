# OpenPLC_v3 技术分析与 RustPLC 借鉴点

日期：2026-02-18
来源：https://github.com/thiagoralves/OpenPLC_v3

---

## 1. OpenPLC_v3 架构概览

OpenPLC_v3 是一个两层系统：

- **管理层**：Python/Flask Web 应用，负责程序上传、编译触发、运行时控制、监控 UI
- **运行时核心**：C/C++ 实时执行引擎，负责扫描周期、I/O 缓冲、协议服务

两层通过本地 TCP socket（端口 43628）以文本命令协议通信。

### 编译流水线

```
用户编写 .st 文件（IEC 61131-3 Structured Text）
         ↓
  iec2c（matiec 编译器）→ Config0.c, POUS.c, LOCATED_VARIABLES.h ...
         ↓
  glue_generator（读取 LOCATED_VARIABLES.h）
         ↓
  glueVars.cpp（定义 %IX/%QX/%IW/%QW 到指针的映射）
         ↓
  g++ 链接所有 .cpp/.c → openplc 可执行文件
```

### 扫描周期（50ms 默认）

```
sleep_until(next_cycle)       // 绝对时间睡眠，最小化抖动
updateBuffersIn()             // HAL 读硬件 → 内部缓冲
updateBuffersIn_MB()          // Modbus master 读从站
config_run__(tick++)          // 执行编译后的 PLC 逻辑
handleSpecialFunctions()      // 更新时间/周期计数/性能指标寄存器
updateBuffersOut()            // 内部缓冲 → HAL 写硬件
updateBuffersOut_MB()         // Modbus master 写从站
RecordCycletimeLatency(...)   // 记录周期时间延迟指标
```

---

## 2. 关键设计决策

### 2.1 共享 I/O 缓冲模型

所有协议服务（Modbus、DNP3、EtherNet/IP）和 HAL 共享同一组缓冲数组，通过单一互斥锁保护：

```c
IEC_BOOL  *bool_input[1024][8];   // %IX — 8192 路数字输入
IEC_BOOL  *bool_output[1024][8];  // %QX — 8192 路数字输出
IEC_UINT  *int_input[1024];       // %IW — 1024 路模拟输入
IEC_UINT  *int_output[1024];      // %QW — 1024 路模拟输出
pthread_mutex_t bufferLock;
```

`bool_input[byte][bit]` 的两级索引直接编码 IEC 61131-3 的 `%IX<byte>.<bit>` 地址语法。

### 2.2 HAL 抽象

HAL 仅暴露四个函数：

```c
void initializeHardware();
void finalizeHardware();
void updateBuffersIn();
void updateBuffersOut();
```

构建时从 16 个平台实现中选一个编译进去。覆盖：树莓派 GPIO、SPI（PiXtend）、sysfs（Neuron）、UDP（Simulink 联仿）、Python 子进程（PSM）等。

### 2.3 Python SubModule（PSM）

允许用 Python 编写硬件驱动，通过 Modbus TCP（端口 2605）与 C 运行时通信。这是一个务实的扩展机制：不改动 C 核心，通过标准协议桥接 Python 生态。

### 2.4 IEC 标准库实现方式

标准库以 C 宏头文件实现，每个类型变体显式命名（`SIN_REAL`、`ADD_INT`、`MUL_DINT`），无重载。`accessor.h` 的 `__GET_VAR`/`__SET_VAR` 宏在读写前检查 force-flag，支持在线变量强制（调试用）。

### 2.5 安全机制（全部是运行时机制，无形式化验证）

| 机制 | 实现方式 |
|---|---|
| 输出安全归零 | 进程退出时 `disableOutputs()` 将所有输出缓冲清零 |
| 实时调度 | Linux `SCHED_FIFO` + `mlockall()` 防止页错误 |
| 缓冲互斥 | `pthread_mutex_t bufferLock` 保护所有 I/O 缓冲访问 |
| 周期时间监控 | 记录 max/min/avg 周期时间和调度延迟，暴露为 `%ML` 寄存器 |
| Modbus 错误限制 | 连续 10 次失败后停止重试并记录日志 |

**OpenPLC_v3 没有**：形式化安全证明、BMC/k-归纳、SMT 求解、死锁/活性分析、因果链验证。

---

## 3. RustPLC 可借鉴的方向

### 3.1 I/O 地址模型（高价值）

OpenPLC 的 `%IX<byte>.<bit>` / `%QX` / `%IW` / `%QW` 地址语法是 IEC 61131-3 标准，工业界广泛使用。

**建议**：在 RustPLC DSL 中考虑支持 IEC 61131-3 标准地址语法作为设备引用的别名或映射，提升与现有 PLC 工程师的互操作性。

### 3.2 HAL 四函数接口（可参考）

`initializeHardware / finalizeHardware / updateBuffersIn / updateBuffersOut` 的极简接口设计，使得平台移植只需实现这四个函数。

**建议**：RustPLC 的硬件后端 trait 可参考此模式，保持接口最小化。

### 3.3 输出安全归零模式（应采纳）

进程退出（正常或异常）时强制将所有输出清零，是工业控制的基本安全要求（fail-safe to de-energized）。

**建议**：RustPLC 生成的代码或运行时应在 Drop/panic 路径上实现等价的输出归零逻辑。

### 3.4 周期时间延迟监控（可参考）

将 `cycle_max/min/avg` 和 `latency_max/min/avg` 暴露为可寻址寄存器，让 PLC 程序本身可以读取自身的实时性指标。

**建议**：RustPLC 的时序验证引擎已有静态分析，可补充运行时指标采集接口，形成静态+动态双重时序保障。

### 3.5 PSM 模式的启示（架构参考）

PSM 通过 Modbus TCP 桥接 Python 驱动，本质是"用标准协议解耦语言边界"。

**建议**：RustPLC 未来若需支持第三方驱动扩展，可考虑类似的协议桥接模式（如通过 gRPC 或 Modbus 接口），而非要求驱动必须用 Rust 编写。

### 3.6 变量强制（Force）机制（调试价值高）

`accessor.h` 的 force-flag 机制允许在不修改程序的情况下，从外部强制覆盖任意变量值，是 PLC 调试的核心工具。

**建议**：RustPLC 的调试/仿真层可考虑实现等价的变量强制机制，配合场景回放使用。

---

## 4. RustPLC 的差异化优势（OpenPLC 没有的）

| 能力 | RustPLC | OpenPLC_v3 |
|---|---|---|
| 形式化安全验证 | BMC + k-归纳，编译期证明 | 无 |
| 活性分析 | SCC + 可达性，检测死锁/活锁 | 无 |
| 时序验证 | 关键路径分析，静态保证 | 仅运行时监控 |
| 因果链验证 | 拓扑图 BFS，信号传播证明 | 无 |
| 类型安全 | Rust 类型系统 + 编译期检查 | C 类型系统 |
| DSL 表达力 | 声明式拓扑+约束+状态机 | IEC 61131-3 命令式 |
| 诊断质量 | 结构化错误（位置/原因/建议） | 基本错误信息 |

RustPLC 的核心价值在于**编译期形式化验证**，这是 OpenPLC_v3 完全没有的能力层。OpenPLC_v3 的价值在于**生产就绪的运行时**（实时调度、多协议、多硬件平台）。

---

## 5. 不建议借鉴的部分

- **matiec/iec2c 依赖**：OpenPLC 不自己实现解析器，完全依赖外部 C 编译器。RustPLC 自有 pest PEG 解析器，这是正确选择，保持了完整的编译期控制权。
- **Python/Flask 管理层**：对于 RustPLC 当前阶段（编译器/验证器），不需要 Web 管理层。
- **C/C++ 运行时**：RustPLC 目标是生成可验证的代码，运行时安全由 Rust 类型系统保障，不需要复制 C 的互斥锁模式。

---

## 6. 总结

OpenPLC_v3 是一个成熟的**运行时平台**，解决了"如何在各种硬件上可靠运行 PLC 程序"的问题。RustPLC 解决的是完全不同的问题："如何在编译期证明 PLC 程序是安全的"。

两者互补而非竞争。长期来看，RustPLC 验证通过的程序，可以考虑以 OpenPLC_v3 兼容格式（IEC 61131-3 ST）作为一种输出目标，利用 OpenPLC 的运行时生态。
