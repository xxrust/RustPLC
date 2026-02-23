# PLC 设备端口化改造 Task 清单（执行版）

日期：2026-02-23
负责人：Codex + 用户
目标：把 `X0/Y0/AI0/AO0` 从“设备语义”逐步迁移为“PLC 设备端口语义”，同时保持 no-board / HIL / 实板映射稳定。

---

## 阶段 0：基线冻结（低风险，先做）

### T0.1 统一端口命名解析（基建）
- [x] 新增共享模块，统一解析 `X/Y/AI/AO/DI/DO + id`
- [x] 替换 `runtime_bridge`、`scenario_resolve`、`main` 中分散解析逻辑
- [x] 补充单元测试，确保大小写与别名行为稳定

**验收标准**
- 所有调用点不再各自维护 `parse_x_id/parse_y_id` 私有实现
- 相关测试通过

### T0.2 语义与文档对齐
- [x] 文档明确：当前仍兼容 `device X0: digital_input`，但其语义是 PLC I/O 端口端点
- [x] 为后续 DSL 迁移添加 deprecation 说明（仅提示，不阻断）

**验收标准**
- 关键文档中术语一致：I/O 点位（port endpoint）而非物理设备

---

## 阶段 1：DSL 引入 PLC 设备（兼容模式）

### T1.1 新增 `plc` 设备类型（仅拓扑层）
- [x] 支持 `device plc_main: plc { ports: [...] }`
- [x] relation 允许 `plc_main.<port>` 参与连接

### T1.2 编译前降维
- [x] 在 preprocess 中把 `plc_main.<port>` 规范化降维到内部通道节点（保持 runtime/io_map 不改）
- [x] 保留旧写法兼容路径

**验收标准**
- 旧示例不破
- 新 DSL 可以跑通 parse + semantic + verify + sim

---

## 阶段 2：映射统一（实板/虚拟板）

### T2.1 ChannelRegistry
- [x] 建立统一 `channel_name <-> logical id <-> io_map key` 注册表
- [x] io_map 校验/scenario 名称解析/runtime 解析统一走注册表

### T2.2 映射契约稳定性
- [x] `virtual` 映射行为不变
- [x] build-rp2040 / no-board-gate / scenario-doctor 回归

**验收标准**
- 同一 PLC 在 no-board 与 build-rp2040 输出的逻辑通道 ID 集合一致

---

## 阶段 3：收口与去歧义

### T3.1 去掉“设备即端口”的误导
- [x] UI 拓扑展示默认突出 PLC 设备与端口，不把 X/Y 渲染为独立工艺设备
- [x] parse-plc API 输出补充 endpoint 类型标签（controller_port / process_device）

### T3.2 兼容策略收口
- [x] 给旧语法设置迁移窗口与告警级别
- [x] 发布迁移脚本（可选）

---

## 执行顺序（完成）
1. 完成 T0.1 基建统一（端口解析与调用点替换）
2. 推进 T0.2/T1/T2/T3（DSL、语义降维、API/UI、迁移策略）
3. 完成回归验证（cargo check、关键单测、web-ui build）
