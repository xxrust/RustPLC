# Scenario Toolchain Limitations

当前 `plc-gen` 不能默认假设“只要 `.plc` 语义成立，就一定能直接通过 `scenario-init` / `scenario-validate` / `scenario-doctor`”。

## 已观察到的真实阻塞

对 `docs/已实现/wafer_loader.plc` 执行：

- `scenario-init`
- `scenario-validate`
- `scenario-doctor`

都会在 runtime bridge 阶段失败，并报：

```text
unsupported guard expression in supervisor.wait_start: mode_auto == true AND start_button == true
```

这说明当前 scenario 工具链至少对一部分复合 `wait` guard 还不兼容。

## plc-gen 的默认应对

如果用户明确要求当前 scenario 工具链可直接跑通，优先采用 scenario-friendly lowering：

- 避免把关键等待写成单条复合 `wait: a AND b`
- 避免把关键等待写成单条复合 `wait: a OR b`
- 优先拆成：
  - helper readiness step
  - 单条件 `wait`
  - 随后的 `if` / 中间 step 决策

## 什么时候要明确报阻塞

如果现有 `.plc` 已经用了复合 guard，且 scenario 工具链报 `unsupported guard expression`：

- 不要说成“PLC 一定是错的”
- 不要继续机械推荐同一组 scenario 命令
- 应明确写成：
  - 当前是 toolchain compatibility blocker
  - 若要保留现有 DSL 形态，则当前 scenario 链路无法完成验证
  - 若必须走 scenario 链路，则需要重写相关 wait 形态

## 建模建议

当 system contract 原本写的是：

- “等待 A 且 B”
- “等待 X 或 Y”

若当前交付必须兼容 scenario 工具链，优先考虑：

1. 先等待主条件
2. 再在下一 step 用 `if` 检查次条件
3. 或拆成多个中间 readiness step

这样会让 DSL 更长，但更接近当前工具链的可验证路径。
