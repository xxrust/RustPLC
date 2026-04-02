# 本轮决策

## 假设状态

支持

## 关键证据

- 对 `docs/已实现/wafer_loader.plc` 的真实 `scenario-init` / `scenario-validate` / `scenario-doctor` 都报同类 `unsupported guard expression` 阻塞。
- 这类阻塞此前没有进入 `plc-gen` 的公开面和决策主路径。
- 新增 scenario 工具链限制工件和 scenario-friendly lowering 规则后，skill 能更准确区分“PLC 能生成”与“当前工具链能否验证”。

## 本轮最小动作

- 保留 scenario toolchain 工件与规则。
- 再做一轮验证，确认 `plc-gen` 现在已经能主动暴露这类真实工具链阻塞，而不是继续误导用户重复验证。

## 是否进入下一轮

是

## 下一轮研究问题

在不新增更多文档的前提下，当前 `plc-gen` 是否已经能正确处理“复杂 PLC 可交付，但当前 scenario 工具链被复合 guard 阻塞”这一现实路径。
