# Scenario-Friendly Guard Patterns

这个工件只回答一个问题：

> 当当前 scenario 工具链吃不下复合 guard 时，`plc-gen` 该优先怎么改写？

## 已验证的最小对照

在 `out/skill_flywheel/plc_gen_guard_lab/` 做过最小对照：

- `composite_wait.plc`
  - `wait: X0 == true AND X1 == true`
  - `scenario-init` 失败
  - 报错：`unsupported guard expression`

- `sequential_wait.plc`
  - 先 `wait: X0 == true`
  - 再 `wait: X1 == true`
  - `scenario-init` 成功
  - `scenario-validate` 成功

## 默认改写优先级

当业务语义允许时，优先级如下：

1. 顺序单条件 `wait`
2. helper readiness step
3. 中间变量 / readiness gate
4. 最后才考虑保留复合 guard 并把状态标成 toolchain-blocked

## 推荐改写

原始写法：

```text
step wait_ready:
    wait: A == true AND B == true
    timeout: 1000ms -> goto fault
```

优先改成：

```text
step wait_a:
    wait: A == true
    timeout: 1000ms -> goto fault

step wait_b:
    wait: B == true
    timeout: 1000ms -> goto fault
```

## 适用边界

这个改写只在业务语义允许“先等 A，再等 B”时优先采用。

如果 system contract 明确要求：

- A 与 B 必须在同一观测点同时成立
- 或 A/B 任一变化都影响后续判定

则不能盲目顺序化；这时应：

- 尝试 helper readiness gate
- 或把当前状态明确标成 toolchain-blocked

## plc-gen 的默认说法

当用户要求当前 scenario 工具链可直接跑时，`plc-gen` 应优先说：

- “我会优先生成 scenario-friendly guard 形态”
- “如果保留复合 guard，我会把它标成当前工具链兼容性阻塞”
