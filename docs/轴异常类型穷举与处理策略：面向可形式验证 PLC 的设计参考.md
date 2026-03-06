# 轴异常类型穷举与处理策略：面向可形式验证 PLC 的设计参考

> **摘要**：本文系统穷举工业控制领域中轴（Axis）可能发生的全部异常类型，按照物理来源进行七大类分类，分析各类异常的触发条件、典型表现与处理策略，并从可形式验证 PLC 的视角给出异常处理的 DSL 设计建议。分类体系综合参考了 CiA 402（DS402）、PLCopen、西门子博途 TIA Portal、Beckhoff TwinCAT 以及 Kollmorgen KAS 等主流平台的错误码标准。

---

![轴异常类型分布](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/IijY58fk86aSZCwo3OKR6u-images_1772763192015_na1fn_L2hvbWUvdWJ1bnR1L2ZhdWx0X2NoYXJ0X3BpZQ.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L0lpalk1OGZrODZhU1pDd28zT0tSNnUtaW1hZ2VzXzE3NzI3NjMxOTIwMTVfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwyWmhkV3gwWDJOb1lYSjBYM0JwWlEucG5nIiwiQ29uZGl0aW9uIjp7IkRhdGVMZXNzVGhhbiI6eyJBV1M6RXBvY2hUaW1lIjoxNzk4NzYxNjAwfX19XX0_&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=UxVPMEAw2agUReS7etiEA2ys8Rf5dWmK6c6UNYYmtIyr3~pkP~5lyUh7k3la5ywM3LGtuZcYHsl-3pWoz8OOigs6n2DFaPm-5LGxpvO~i4JxeUQSShhXIXy2fYl-gr8gsc7-nyQO84gKxpNMJan8~r5zO9~G3A4jVpZiwaoQiMmt99yaMJR0~RDfYJafWgnWC~GX1z4smBcS21XIvtD9jyTNsqSJNi4ZkjLmU8zL6ufNJ6YocNyiRmfRdS84hycIxBaoRE6JhN-bb2xPEA9wncRn98FtyxxVwiE-08YxRt~n~yoVjRXuVfr8meFLDBf9nWHpkxREIhWzh5qqBQgE6A__)

![异常严重程度与恢复方式矩阵](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/IijY58fk86aSZCwo3OKR6u-images_1772763192015_na1fn_L2hvbWUvdWJ1bnR1L2ZhdWx0X2NoYXJ0X21hdHJpeA.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L0lpalk1OGZrODZhU1pDd28zT0tSNnUtaW1hZ2VzXzE3NzI3NjMxOTIwMTVfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwyWmhkV3gwWDJOb1lYSjBYMjFoZEhKcGVBLnBuZyIsIkNvbmRpdGlvbiI6eyJEYXRlTGVzc1RoYW4iOnsiQVdTOkVwb2NoVGltZSI6MTc5ODc2MTYwMH19fV19&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=OamoEgID9pxexRLaWs8vNStQGgS96LB7mojwUpW2O4N1JY-gAetkfgZDsl6IUL-2vDgaOSCnVeYG57QX5v2SDWShl0~URawhgRltJPYDM5PzEhaW7xEAWuoZR6MyK0B5MFvEEKd8fLJkOVt7c~2UIIw~7Ig1bWoUT1oHfU0lb74m~pw2jw~K0958yZfbM~NfO6HzPeC4Nd1bW6yujYfGTwhKXbZP6Z~obhY3GiPmZxe1kPNtUSI37Unw8Pu8acagAkRDkB846sQAuLucxOsnUi5y~vGXnONYgZgpIRAhFDs2de5EnqCRiboBBgz64Hb5QorgydJgcyc6JowMtnn8QQ__)

![PLCopen 状态机与异常处理流程](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/IijY58fk86aSZCwo3OKR6u-images_1772763192015_na1fn_L2hvbWUvdWJ1bnR1L2ZhdWx0X2NoYXJ0X3N0YXRlbWFjaGluZQ.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L0lpalk1OGZrODZhU1pDd28zT0tSNnUtaW1hZ2VzXzE3NzI3NjMxOTIwMTVfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwyWmhkV3gwWDJOb1lYSjBYM04wWVhSbGJXRmphR2x1WlEucG5nIiwiQ29uZGl0aW9uIjp7IkRhdGVMZXNzVGhhbiI6eyJBV1M6RXBvY2hUaW1lIjoxNzk4NzYxNjAwfX19XX0_&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=OxLheqZLtlKArwygWR1twDsKnJVnUmDO~DHT-83Wb0irCsL9FWkrbByYuqS4dtiiJtvqbTaWRQdRB~32VtvDns1asmP-I1NCrXnxkR7C22lmqhZUgKECiXMc9adjnzL4OTImA5m3QBeAdTi2~RSaIVJjTfeFrLD9a~DLFnTzdvn8LZw9nQvweKIOgzeaj2gwUgiW~YczjemBYut6g~58ltmB7kNLHzvHPdkdwpIp0Q7C-1zFhGZXa1BPlx5BiZz85ZJawaeo1gFIhLcL0DGtwgPucv9yAUTV3I8SgOiPx35rrfxeHD8myHPo1UlbfrAMrIpOYXe3KneRwiM55d4Ujw__)

![步进 vs 伺服异常适用性对比](https://private-us-east-1.manuscdn.com/sessionFile/yHm4A790RbLQVuc0pHmlfU/sandbox/IijY58fk86aSZCwo3OKR6u-images_1772763192015_na1fn_L2hvbWUvdWJ1bnR1L2ZhdWx0X2NoYXJ0X2NvbXBhcmlzb24.png?Policy=eyJTdGF0ZW1lbnQiOlt7IlJlc291cmNlIjoiaHR0cHM6Ly9wcml2YXRlLXVzLWVhc3QtMS5tYW51c2Nkbi5jb20vc2Vzc2lvbkZpbGUveUhtNEE3OTBSYkxRVnVjMHBIbWxmVS9zYW5kYm94L0lpalk1OGZrODZhU1pDd28zT0tSNnUtaW1hZ2VzXzE3NzI3NjMxOTIwMTVfbmExZm5fTDJodmJXVXZkV0oxYm5SMUwyWmhkV3gwWDJOb1lYSjBYMk52YlhCaGNtbHpiMjQucG5nIiwiQ29uZGl0aW9uIjp7IkRhdGVMZXNzVGhhbiI6eyJBV1M6RXBvY2hUaW1lIjoxNzk4NzYxNjAwfX19XX0_&Key-Pair-Id=K2HSFNDJXOU9YS&Signature=LuvKUBsKwhNjGol221Gg3yO6QJHWzmRZEx3O1WHfWiXkBB-d-QR4I2VfWvxzmSynhT4tCb2tVYRuHkCEpXodEhQWukybSLqdwELcQgHiVz8atycUTKRXcKSbxclkhiek6rQXfPl~lH0JpxxcnoQLdWXyyQOrGPKz~JhImWhB~TkJgzOmqDcgUrriUCLuEYrIHpjUsF-vOhg1l17ys93Bv0cxE0FTc2dlLqvp5TXO1LgZDDnRg44Zz3-Zo4O2cbadSZ46h~RhnpEftV6TlX3iNZDaanOseIhGF9h1MoXxvFeSf~uF~ziMp47DqLiO5EzAhdWhLVJxJX5Xxcw98qSi7Q__)

---

## 一、异常的本质分类框架

在正式穷举之前，需要建立一个**正交的分类框架**。工业控制领域的轴异常，从根本上可以按两个维度划分：

**维度一：来源层次**（异常发生在哪一层）

| 层次 | 描述 | 典型来源 |
|---|---|---|
| 硬件层 | 电气、机械、传感器的物理故障 | 过流、过温、编码器断线 |
| 驱动器层 | 驱动器内部的控制与保护 | 直流母线过压、制动电阻过热 |
| 通信层 | 控制器与驱动器之间的通信 | EtherCAT 丢帧、看门狗超时 |
| 运动控制层 | 轨迹规划与位置控制的逻辑 | 跟随误差超限、软限位触发 |
| 应用层 | 用户程序的调用逻辑错误 | 参数非法、状态不满足 |

**维度二：严重程度**（异常应如何响应）

| 级别 | 名称 | 响应要求 | 典型例子 |
|---|---|---|---|
| 0 | 信息（Info） | 记录，不影响运行 | 位置窗口未到达（超时警告） |
| 1 | 警告（Warning） | 记录，继续运行，提示关注 | 电机温度偏高、跟随误差偏大 |
| 2 | 可恢复故障（Recoverable Fault） | 停止运动，等待复位，可重新启动 | 软限位触发、跟随误差超限 |
| 3 | 不可恢复故障（Non-recoverable Fault） | 立即切断使能，需要重新上电或维修 | 编码器硬件损坏、驱动器内部短路 |
| 4 | 安全相关故障（Safety Fault） | 触发安全功能（STO/SBC），需要安全评估 | STO 输入断开、安全门打开 |

这两个维度的交叉构成了完整的异常空间。以下按**来源层次**展开穷举，每类异常均标注严重程度。

---

## 二、第一类：电气与功率异常（Electrical & Power Faults）

此类异常来自驱动器的电气保护电路，对应 CiA 402 错误码的 `0x2xxx`（电流类）和 `0x3xxx`（电压类）范围。

### 2.1 过流类（Over-Current）

| 异常名称 | CiA 402 码 | 触发条件 | 严重程度 |
|---|---|---|---|
| 输入侧短路/接地漏电 | 0x2110 | 输入电源侧短路或接地 | 3 |
| 驱动器内部过流 | 0x2220 | 驱动器功率器件过流 | 3 |
| 电机侧短路 | 0x2320 | 电机绕组或电缆短路 | 3 |
| 电机侧接地漏电（U/V/W 相） | 0x2330~0x2333 | 电机绕组对地绝缘破坏 | 3 |
| I²t 热状态过载 | 0x2350 | 长时间过载导致热积累超限 | 2~3 |

**典型处理**：过流类异常通常是**不可恢复故障**，需要立即切断功率输出（驱动器自动执行），上层控制器接收到故障信号后进入 `ErrorStop` 状态。恢复前必须排查电气故障原因，不允许程序自动复位。

### 2.2 过压/欠压类（Over/Under-Voltage）

| 异常名称 | CiA 402 码 | 触发条件 | 严重程度 |
|---|---|---|---|
| 电网过压（各相） | 0x3110~0x3113 | 电网电压超过额定范围上限 | 2~3 |
| 电网欠压（各相） | 0x3120~0x3123 | 电网电压低于额定范围下限 | 2~3 |
| 缺相 | 0x3130~0x3134 | 三相电源某相断路 | 3 |
| 直流母线过压 | 0x3210 | 制动能量回馈导致母线电压过高 | 3 |
| 直流母线欠压 | 0x3220 | 电源掉电或电压跌落 | 2~3 |
| 输出过压（各相） | 0x3310~0x3313 | 输出侧电压异常 | 3 |

**典型处理**：直流母线过压是**减速制动时最常见的异常**，通常由制动电阻容量不足引起。可恢复故障（如短暂欠压）在电源恢复后可自动或手动复位；不可恢复故障（如缺相）需要维修。

**对步进电机的特殊说明**：步进电机驱动器通常不检测电网三相，但会检测直流母线电压（欠压保护），以及驱动电流（过流保护）。

### 2.3 制动与能量回收异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 制动电阻过热 | 频繁制动导致制动电阻过热 | 2 |
| 制动斩波器故障 | 制动斩波器 IGBT 损坏 | 3 |
| 再生能量过大 | 回馈能量超过制动电阻额定功率 | 2 |

---

## 三、第二类：温度异常（Thermal Faults）

温度异常是**最常见的渐进式异常**，通常先出现警告，若不处理则升级为故障。对应 CiA 402 的 `0x4xxx` 范围。

| 异常名称 | CiA 402 码 | 触发条件 | 严重程度 |
|---|---|---|---|
| 环境温度过高 | 0x4110 | 控制柜散热不良 | 1→2 |
| 驱动器本体过温 | 0x4210 | 散热片温度超限 | 1→3 |
| 电机过温（NTC/PTC 传感器） | 0x4310 | 电机绕组温度超限 | 1→3 |
| 控制电源过温 | 0x4410 | 控制电源模块过热 | 2 |

**典型处理**：温度类异常的处理策略应当是**分级响应**：
- **警告阶段**（温度达到 80% 阈值）：记录日志，降低速度/加速度，通知操作员
- **故障阶段**（温度超过阈值）：受控停机（MC_Stop），等待冷却后复位
- **紧急阶段**（温度持续上升）：立即切断使能（MC_Power=FALSE）

**对可形式验证 PLC 的意义**：温度异常的分级响应是一个典型的**多阶段状态机**，需要在 DSL 中显式建模"警告→降速→停机"的状态转换路径。

---

## 四、第三类：编码器与传感器异常（Encoder & Sensor Faults）

编码器异常是**伺服系统特有的高频异常**，对步进电机（开环）不适用，但对带外部编码器的步进闭环系统同样适用。对应 CiA 402 的 `0x7300` 范围。

### 4.1 编码器硬件异常

| 异常名称 | CiA 402 码 | 触发条件 | 严重程度 |
|---|---|---|---|
| 增量编码器断线/信号丢失 | 0x7305~0x7307 | A/B/Z 信号线断路或短路 | 3 |
| 旋转变压器故障 | 0x7303~0x7304 | Resolver 励磁或反馈信号异常 | 3 |
| 绝对值编码器通信错误 | 0x7305 | BiSS/EnDat/SSI 通信超时或 CRC 错误 | 3 |
| 编码器电源故障 | — | 编码器 5V/12V 供电异常 | 3 |
| 编码器极性错误 | 0x7302 | 编码器方向与电机方向不一致 | 2 |

### 4.2 编码器逻辑异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 编码器计数溢出 | 位置值超过编码器量程（绝对值编码器多圈溢出） | 2 |
| 编码器跳变（位置突变） | 单周期内位置变化超过合理范围 | 2~3 |
| 编码器与电机不同步（换相错误） | 绝对值编码器初始化失败 | 3 |

**对步进电机的特殊说明**：步进电机开环运行时无编码器，因此**不存在编码器类异常**。但若配置了外部编码器（闭环步进），则上述所有编码器异常均适用，且还需额外处理**失步检测**（见第六类）。

**典型处理**：编码器硬件异常通常是**不可恢复故障**，驱动器会立即切断使能（因为失去位置反馈后无法安全控制）。上层控制器必须捕获此异常并进入 `ErrorStop`，**禁止自动复位**。

---

## 五、第四类：通信异常（Communication Faults）

通信异常是**分布式运动控制系统**（EtherCAT、PROFINET、CANopen）特有的一类异常，在集中式系统（步进电机脉冲接口）中不存在。

### 5.1 现场总线通信异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| EtherCAT 帧丢失 | 网线断开、交换机故障、EMI 干扰 | 3 |
| EtherCAT 看门狗超时（WDT） | 主站周期超时，驱动器触发 WDT | 3 |
| PDO 数据无效 | 主站发送的过程数据包含无效值 | 2~3 |
| 从站状态机错误 | EtherCAT 从站无法进入 OP 状态 | 3 |
| PROFINET 连接中断 | 控制器与驱动器之间的 PROFINET 连接断开 | 3 |
| CANopen 心跳超时 | 节点心跳报文丢失 | 3 |

### 5.2 脉冲接口通信异常（步进电机特有）

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 脉冲频率超限 | 发出的脉冲频率超过驱动器最大接收频率 | 2 |
| 使能信号丢失 | EN 引脚信号意外断开 | 2~3 |
| 报警信号触发 | 驱动器 ALM 引脚输出报警 | 2~3 |

**典型处理**：通信异常的处理策略取决于**通信恢复的可能性**：
- **瞬时中断**（< 1 个周期）：驱动器 WDT 触发，立即进入快速停止（Quick Stop），上层进入 `ErrorStop`
- **持续中断**：必须等待通信恢复后重新初始化，**不允许在通信中断状态下运动**

**对可形式验证 PLC 的意义**：通信异常引入了**时间不确定性**——通信恢复时间是不可预测的。DSL 需要显式建模"等待通信恢复"这一状态，并设置超时机制。

---

## 六、第五类：运动控制逻辑异常（Motion Control Faults）

此类异常是**运动控制层特有的**，与电气硬件无关，由轨迹规划与位置控制算法检测。这是 PLCopen 和各平台运动控制层最关注的异常类型。

### 6.1 位置相关异常

| 异常名称 | 触发条件 | 严重程度 | CiA 402 / PLCopen 对应 |
|---|---|---|---|
| **跟随误差超限**（Following Error） | 指令位置与实际位置的偏差超过阈值 | 2~3 | CiA 402: 0x8611；PLCopen: ErrorStop |
| 软件正限位触发 | 指令位置超过正向软限位 | 2 | PLCopen: ErrorStop |
| 软件负限位触发 | 指令位置超过负向软限位 | 2 | PLCopen: ErrorStop |
| 硬件正限位开关触发 | 正向限位开关信号激活 | 2 | PLCopen: ErrorStop |
| 硬件负限位开关触发 | 负向限位开关信号激活 | 2 | PLCopen: ErrorStop |
| 位置窗口超时 | 运动完成后，在规定时间内未进入位置窗口 | 1 | PLCopen: Warning |
| 参考点丢失（未回零） | 轴未完成回零即执行绝对位置运动 | 2 | PLCopen: ErrorStop |

**跟随误差超限是最重要的运动控制异常**，其含义是：控制器发出的位置指令与编码器反馈的实际位置之间的差值超过了设定阈值。这通常意味着：
- 机械卡死或阻力过大（最常见）
- 驱动器参数整定不当（增益过低）
- 运动速度/加速度超过电机能力
- 编码器故障（见第四类）

**对步进电机的特殊说明**：步进电机开环运行时**没有跟随误差**的概念（因为没有实际位置反馈）。但存在**失步**问题：

| 异常名称 | 触发条件 | 严重程度 | 检测方式 |
|---|---|---|---|
| **步进失步**（Step Loss） | 电机实际步数少于指令步数 | 2~3 | 需要外部编码器或失步检测电路 |
| 步进堵转 | 电机完全停止但仍在发脉冲 | 3 | 需要外部编码器 |

### 6.2 速度相关异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 速度超限（Over-Speed） | 实际速度超过最大允许速度 | 2~3 |
| 速度控制器饱和 | 速度控制器输出长时间处于饱和状态 | 1 |
| 速度振荡 | 速度反馈出现持续振荡 | 1~2 |

### 6.3 扭矩/电流相关异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 扭矩超限 | 实际扭矩超过最大允许扭矩 | 2 |
| 扭矩控制器异常 | 扭矩控制模式下控制器发散 | 2~3 |
| 持续满扭矩运行 | 长时间在额定扭矩附近运行（过热前兆） | 1 |

### 6.4 回零相关异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 回零超时 | 在规定时间内未找到参考点 | 2 |
| 回零开关未找到 | 运动到行程末端仍未触发回零开关 | 2 |
| 回零开关信号异常 | 回零开关信号抖动或持续激活 | 2 |
| 回零被中断 | 回零过程中发生其他故障 | 2 |
| 绝对值编码器回零失败 | 绝对值编码器读取失败，无法建立参考点 | 3 |

**对可形式验证 PLC 的意义**：回零是一个**有明确前置条件和后置条件的操作**，非常适合形式化建模。前置条件：轴处于 `Standstill` 状态；后置条件：`isHomed = TRUE` 且 `actualPosition` 在合理范围内。任何中断回零的异常都必须将 `isHomed` 置为 `FALSE`。

---

## 七、第六类：机械与外部干预异常（Mechanical & External Faults）

此类异常来自机械系统或外部安全设备，是**最难预测但最重要**的一类。

### 7.1 机械异常

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| 机械卡死（Jam/Stall） | 机械结构卡死，电机无法转动 | 2~3 |
| 传动系统故障 | 皮带断裂、联轴器损坏、丝杠卡死 | 3 |
| 制动器未释放 | 电磁制动器未正常释放即启动运动 | 3 |
| 制动器未夹紧 | 去使能后制动器未正常夹紧（垂直轴危险） | 3（安全） |
| 负载超重 | 实际负载超过电机额定能力 | 2 |

**制动器异常**对**垂直轴**（Z 轴、悬挂轴）尤为危险。制动器未夹紧意味着轴可能在重力作用下自由下落，属于安全相关故障，必须在 DSL 中特殊处理。

### 7.2 外部安全干预

| 异常名称 | 触发条件 | 严重程度 |
|---|---|---|
| **紧急停止（E-Stop）** | 操作员按下急停按钮 | 4（安全） |
| **安全转矩关断（STO）** | 安全门打开或安全 PLC 触发 STO | 4（安全） |
| **安全制动控制（SBC）** | 安全 PLC 触发制动器夹紧 | 4（安全） |
| 安全限速（SLS）触发 | 速度超过安全限速阈值 | 4（安全） |
| 安全位置（SLP）触发 | 位置超过安全位置限制 | 4（安全） |
| 安全停止（SS1/SS2）触发 | 安全 PLC 触发受控停止 | 4（安全） |

**E-Stop 与 STO 的区别**是一个常见的混淆点：
- **E-Stop（急停）**：通常通过 PLCopen `MC_Stop` 实现受控减速停止，保留位置信息，可以恢复
- **STO（安全转矩关断）**：直接切断驱动器功率输出，**不执行减速**，位置可能丢失，属于 IEC 62061/ISO 13849 安全功能

**对可形式验证 PLC 的意义**：安全相关故障（STO/SBC）**不应由用户程序处理**，而应由独立的安全回路处理。DSL 需要明确区分"可编程处理的异常"和"安全硬件处理的异常"，后者在 DSL 层面只能**感知**（读取状态），不能**控制**。

---

## 八、第七类：应用层与配置异常（Application & Configuration Faults）

此类异常来自用户程序的调用逻辑或配置错误，是**最可避免**的一类，也是**可形式验证 PLC 最应该在编译时消除**的一类。

### 8.1 参数非法异常

| 异常名称 | PLCopen ErrorID | 触发条件 | 是否可编译时检测 |
|---|---|---|---|
| 速度参数为零或负数 | 17（out of range） | `MC_MoveAbsolute(velocity=0)` | **是** |
| 加速度参数为零或负数 | 17 | `MC_MoveAbsolute(acceleration=0)` | **是** |
| 速度/加速度比例非法 | 21 | 加速度过大导致速度/加速度比无效 | 部分 |
| 目标位置超出行程范围 | 17 | 目标位置超过软限位配置 | **是**（若限位已知） |
| 传动比参数非法 | 9 | 齿轮比分母为零 | **是** |
| 凸轮表无效 | 15 | 引用了未初始化的凸轮表 | 部分 |

### 8.2 状态前置条件不满足

| 异常名称 | PLCopen ErrorID | 触发条件 | 是否可编译时检测 |
|---|---|---|---|
| 轴未使能即执行运动 | 65（axis not powered） | 未调用 `MC_Power` 即调用 `MC_MoveAbsolute` | **是**（静态分析） |
| 轴处于 ErrorStop 状态 | 10 | 在 ErrorStop 状态下发出运动指令 | **是**（状态机分析） |
| 轴处于 Stopping 状态 | 10 | 在 Stopping 状态下发出运动指令 | **是**（状态机分析） |
| 未回零即执行绝对运动 | — | `isHomed=FALSE` 时调用 `MC_MoveAbsolute` | **是**（前置条件检查） |
| 同步轴未脱离同步即执行独立运动 | — | 在 `SynchronizedMotion` 状态下调用 `MC_MoveAbsolute` | **是**（状态机分析） |

### 8.3 多轴协调逻辑异常

| 异常名称 | PLCopen ErrorID | 触发条件 |
|---|---|---|
| 主从轴循环依赖 | 4 | 轴 A 跟随轴 B，轴 B 同时跟随轴 A |
| 主从轴更新率不匹配 | 4 | 主轴和从轴的控制周期不同 |
| 凸轮表主轴范围溢出 | 102 | 主轴位置超出凸轮表定义范围 |
| 齿轮比导致速度超限 | — | 从轴跟随主轴时，计算出的从轴速度超过最大速度 |

**对可形式验证 PLC 的核心意义**：应用层异常是**最应该在 DSL 设计中消除**的一类。通过以下手段，可以将大量运行时异常转化为编译时错误：

1. **类型系统**：将速度、加速度定义为正数类型（`PositiveFloat`），编译时拒绝零值或负值
2. **状态机约束**：在 DSL 层面强制检查调用前置条件，如"只有在 `Standstill` 状态才能调用 `MC_MoveAbsolute`"
3. **回零前置条件**：将 `isHomed` 作为 `MC_MoveAbsolute` 的隐式前置条件，编译时检查
4. **参数范围约束**：目标位置必须在 `[softLimitNegative, softLimitPositive]` 范围内，编译时可验证

---

## 九、异常处理的核心策略

### 9.1 各类异常的标准响应动作矩阵

| 异常类别 | 立即响应 | 轴状态转换 | 恢复方式 | 自动复位 |
|---|---|---|---|---|
| 过流/短路 | 立即切断功率输出 | → ErrorStop | 排查硬件后手动复位 | **禁止** |
| 过压/欠压 | 快速停止（Quick Stop） | → ErrorStop | 电源恢复后手动复位 | 谨慎允许 |
| 过温（故障级） | 受控停机 | → ErrorStop | 冷却后手动复位 | **禁止** |
| 过温（警告级） | 降速继续运行 | 保持当前状态 | 自动恢复 | 自动 |
| 编码器硬件故障 | 立即切断功率输出 | → ErrorStop | 更换编码器后手动复位 | **禁止** |
| 通信中断 | 快速停止（Quick Stop） | → ErrorStop | 通信恢复后手动复位 | 谨慎允许 |
| 跟随误差超限 | 受控停机或快速停止 | → ErrorStop | 排查机械后手动复位 | 谨慎允许 |
| 软限位触发 | 受控停机 | → ErrorStop | 反向运动后手动复位 | **允许** |
| 硬限位触发 | 快速停止 | → ErrorStop | 反向运动后手动复位 | **允许** |
| 回零失败 | 受控停机 | → ErrorStop | 重新执行回零 | **允许** |
| E-Stop | 受控减速停止 | → ErrorStop | 释放急停后手动复位 | **禁止** |
| STO 触发 | 立即切断功率（硬件） | → Disabled（硬件强制） | 安全评估后恢复 | **禁止** |
| 参数非法 | 拒绝执行指令 | 保持当前状态 | 修正参数后重试 | 自动 |
| 状态不满足 | 拒绝执行指令 | 保持当前状态 | 满足前置条件后重试 | 自动 |

### 9.2 Quick Stop vs. 受控停机 vs. 立即切断

这三种停止方式的区别是异常处理设计的核心：

| 停止方式 | 描述 | 位置保持 | 适用场景 |
|---|---|---|---|
| **受控停机**（Controlled Stop） | 按照设定的减速度减速到零，轴保持使能 | 是 | 软限位、回零失败、参数错误 |
| **快速停止**（Quick Stop） | 按照急停减速度（更大）快速减速到零 | 是 | 跟随误差、通信中断、E-Stop |
| **立即切断**（Immediate Disable） | 立即切断功率输出，电机自由停止 | 否 | 过流、编码器故障、STO |

**对垂直轴的特殊处理**：垂直轴在"立即切断"时，如果制动器未夹紧，轴会在重力作用下自由下落。因此垂直轴的异常处理必须遵循以下顺序：
1. 触发制动器夹紧（SBC）
2. 等待制动器夹紧确认信号
3. 切断功率输出

### 9.3 异常的传播与隔离

在多轴系统中，一个轴的异常可能需要传播到其他轴：

| 传播场景 | 传播规则 | 示例 |
|---|---|---|
| 主轴故障 | 所有从轴必须同步停止 | 主轴编码器故障 → 所有电子齿轮从轴停止 |
| 从轴故障 | 主轴继续，从轴脱离同步后停止 | 从轴限位触发 → 从轴 GearOut 后停止 |
| 协调轴组故障 | 组内所有轴同步停止 | 插补组中某轴跟随误差 → 整组停止 |
| 独立轴故障 | 不影响其他轴 | 独立定位轴故障 → 仅该轴停止 |

---

## 十、面向可形式验证 PLC 的 DSL 设计建议

### 10.1 异常类型的 DSL 表达

推荐将异常类型定义为**代数数据类型（Algebraic Data Type）**，而非简单的错误码整数：

```
// 异常类型定义（代数数据类型）
enum AxisFault {
  // 第一类：电气与功率
  OverCurrent(phase: Phase?)           // 过流（可选：哪相）
  OverVoltage(source: VoltageSource)   // 过压（来源：母线/电网/输出）
  UnderVoltage(source: VoltageSource)  // 欠压
  PhaseFailure(phase: Phase)           // 缺相
  BrakeResistorOverheat                // 制动电阻过热

  // 第二类：温度
  OverTemperature(component: ThermalComponent, value: Float)
  TemperatureWarning(component: ThermalComponent, value: Float)

  // 第三类：编码器
  EncoderDisconnected                  // 编码器断线
  EncoderSignalError                   // 编码器信号异常
  EncoderCommunicationError            // 编码器通信错误（BiSS/EnDat）
  EncoderPositionJump(delta: Float)    // 位置突变

  // 第四类：通信
  BusDisconnected(bus: BusType)        // 总线断开
  WatchdogTimeout(bus: BusType)        // 看门狗超时
  PulseFrequencyExceeded               // 脉冲频率超限（步进）

  // 第五类：运动控制逻辑
  FollowingErrorExceeded(actual: Float, limit: Float)  // 跟随误差超限
  SoftLimitPositive(position: Float)   // 正向软限位
  SoftLimitNegative(position: Float)   // 负向软限位
  HardLimitPositive                    // 正向硬限位
  HardLimitNegative                    // 负向硬限位
  OverSpeed(actual: Float, limit: Float) // 速度超限
  StepLoss(estimated: Float)           // 步进失步（步进电机）
  HomingFailed(reason: HomingFailReason) // 回零失败
  NotHomed                             // 未回零

  // 第六类：机械与外部
  Stall                                // 堵转
  BrakeNotReleased                     // 制动器未释放
  BrakeNotEngaged                      // 制动器未夹紧（垂直轴危险）
  EStop                                // 急停
  SafetyFault(function: SafetyFunction) // 安全功能触发（STO/SBC/SLS）

  // 第七类：应用层（编译时可消除）
  InvalidParameter(param: String, value: Float, reason: String)
  PreconditionNotMet(required: AxisState, actual: AxisState)
  NotHomed                             // 绝对运动前未回零
}
```

### 10.2 异常处理的强制显式化

对于可形式验证的 PLC，**所有可能产生异常的操作都必须显式处理异常分支**。推荐使用 `Result` 类型或模式匹配：

```
// 方案一：Result 类型（类似 Rust）
let result = axis.moveAbsolute(position: 100.0, velocity: 50.0)
match result {
  Ok(command) => {
    // 等待命令完成
    await command.done
  }
  Err(fault) => {
    match fault {
      SoftLimitPositive(pos) => {
        // 软限位：记录日志，反向退出
        log.warn("Soft limit triggered at {pos}")
        axis.moveRelative(distance: -5.0)  // 退出限位
        axis.reset()
      }
      NotHomed => {
        // 未回零：先执行回零
        axis.home()
        axis.moveAbsolute(position: 100.0, velocity: 50.0)  // 重试
      }
      FollowingErrorExceeded(actual, limit) => {
        // 跟随误差：记录，等待人工干预
        log.error("Following error: actual={actual}, limit={limit}")
        alarm.trigger(AlarmLevel.Critical, "Axis following error")
        // 不允许自动复位
      }
      _ => {
        // 其他未预期异常：安全停机
        axis.stop()
        alarm.trigger(AlarmLevel.Critical, fault.description())
      }
    }
  }
}
```

### 10.3 异常严重程度的类型系统约束

推荐在类型系统层面区分**可自动复位**和**禁止自动复位**的异常：

```
// 类型系统区分
enum RecoverableFault { ... }    // 可自动复位（软限位、参数错误）
enum NonRecoverableFault { ... } // 禁止自动复位（过流、编码器故障）
enum SafetyFault { ... }         // 安全相关（STO、E-Stop）

// 编译器强制：NonRecoverableFault 的处理函数不能包含 axis.reset() 调用
// 编译器强制：SafetyFault 只能读取状态，不能控制轴
```

### 10.4 垂直轴的特殊异常处理约束

```
// 垂直轴声明
PositioningAxis verticalAxis {
  axisOrientation: Vertical    // 声明为垂直轴
  brake: {
    type: ElectromagneticBrake
    engageSignal: Q0.0          // 制动器夹紧信号
    engageConfirm: I0.0         // 制动器夹紧确认
    engageTimeout: 500ms        // 夹紧超时
  }
}

// 编译器约束：垂直轴的任何故障处理中，
// 必须在 axis.disable() 之前调用 axis.brake.engage()
// 否则编译报错
```

### 10.5 异常处理的完备性检查

可形式验证 PLC 的核心价值在于：**编译器可以检查异常处理的完备性**。具体而言：

1. **穷举检查**：对 `AxisFault` 的 `match` 表达式，编译器检查是否覆盖了所有变体（类似 Rust 的 exhaustive match）
2. **前置条件检查**：`MC_MoveAbsolute` 的调用必须在 `isHomed = TRUE` 的代码路径上，否则编译警告
3. **状态机合法性检查**：不允许在 `ErrorStop` 状态下直接调用运动指令，必须先调用 `reset()`
4. **禁止自动复位约束**：`NonRecoverableFault` 的处理块中，静态分析禁止出现 `axis.reset()` 调用

---

## 十一、异常分类总览

| 大类 | 子类数量 | 典型代表 | 步进适用 | 伺服适用 | 可编译时消除 |
|---|---|---|---|---|---|
| 电气与功率 | ~15 | 过流、直流母线过压 | 部分 | 是 | 否 |
| 温度 | ~8 | 电机过温、驱动器过温 | 是 | 是 | 否 |
| 编码器与传感器 | ~10 | 编码器断线、位置突变 | 否（开环）/ 是（闭环） | 是 | 否 |
| 通信 | ~8 | EtherCAT WDT、脉冲频率超限 | 部分 | 是 | 否 |
| 运动控制逻辑 | ~15 | 跟随误差、软/硬限位、失步 | 部分 | 是 | 部分 |
| 机械与外部 | ~8 | 堵转、E-Stop、STO | 是 | 是 | 否 |
| 应用层与配置 | ~12 | 参数非法、状态不满足 | 是 | 是 | **大部分** |
| **合计** | **~76** | — | — | — | — |

---

## 参考文献

[^1]: CAN in Automation (CiA), "CiA 402 Series: CANopen Device Profile for Drives and Motion Control," TI EtherCAT SDK Documentation, https://software-dl.ti.com/processor-industrial-sw/esd/ind_comms_sdk/am64x/09_00_00_03/docs/am64x/ethercat_slave/group___ci_a402.html

[^2]: Kollmorgen, "PLCopen Function Block ErrorID Output," KAS 3.07 Online Help, https://webhelp.kollmorgen.com/kas3.07/Content/3.UnderstandKAS/FB_PLCopen_ErrorID_output.htm

[^3]: PLCopen Technical Committee 2, "Motion Control Function Blocks for IEC 61131-3," https://www.plcopen.org/standards/motion-control/

[^4]: Beckhoff Automation, "Axis errors from the motion controller," TwinCAT Information System, https://infosys.beckhoff.com/content/1033/tccncmcplatform/15303257227.html

[^5]: Siemens AG, "S7-1500/S7-1500T Motion Control alarms and error IDs," Siemens Industry Support, https://support.industry.siemens.com/cs/attachments/109974352/

[^6]: SEW-Eurodrive, "Fault description CiA402 profile," SEW Documentation, https://download.sew-eurodrive.com/download/html/31546080/en-EN/2823303553141445550091.html
