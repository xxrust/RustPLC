# two_cylinder 语义与验证缺陷检讨（2026-02-22）

## 背景
在 `examples/two_cylinder.plc` 中发现如下写法：

- `device start_button: digital_input { driven_by: X4 }`
- `device sensor_B_ret: sensor { driven_by: X3, detects: cyl_B.retracted }`

这暴露出两个问题：

1. DSL 语义使用不当（输入设备被输入设备驱动、传感器语义混叠）。
2. 语义不当配置未被验证链路阻断，仍可通过形式化验证。

## 事实核查

### 1) 当前语义规则确实允许了这类关系
`DrivenBy` 在语义层被允许如下组合：

- `digital_input -> sensor`
- `digital_input -> digital_input`

见：`src/semantic/mod.rs:2735`、`src/semantic/mod.rs:2737`。

### 2) 示例与测试把该模式“固化”为合法
以下测试样例直接使用了 `sensor driven_by X*`：

- `src/semantic/mod.rs:2807`
- `src/semantic/mod.rs:2849`
- `src/semantic/mod.rs:2867`
- `src/verification/causality.rs:782`
- `src/verification/safety.rs:2011`

这会导致错误语义长期不被发现。

### 3) 验证通过的原因
当前验证主路径关注 Safety/Liveness/Timing/Causality 的约束可满足性，
而“设备关系是否符合端口与角色物理语义”的强约束没有被设置为硬性失败条件。

此外，`parse-plc` API 对未声明端口的设备会返回空端口数组（如 `X0..X4`），见 `crates/web-server/src/main.rs:290`；
再配合前端端口句柄绑定策略，会放大“连线显示与语义一致性”问题。

## 责任检讨

本次问题不是单点失误，而是“语义规则、示例、验证口径”三处一致性失配：

- 规则层允许了有争议关系；
- 示例层沿用了该关系；
- 验证层未把该关系设为阻断条件。

这属于建模边界定义不清与回归门禁不完整，责任在实现与评审流程。

## 改进措施（后续必须落地）

1. 收紧 `driven_by` 设备对设备类型矩阵，默认禁止 `digital_input -> digital_input` 与 `digital_input -> sensor`。
2. 对 `sensor` 的建模做单义约束：明确 `detects` 与 IO 映射关系，避免混用导致语义漂移。
3. 在拓扑语义阶段新增“硬失败”校验，作为形式化验证前置门禁。
4. 清理并重写 `two_cylinder` 及相关测试夹具，确保语义正确样例通过、错误样例必失败。
5. 增加回归测试：
   - 非法 `driven_by` 关系必须报错；
   - 端口元数据缺失与连线绑定异常必须有显式诊断。

## 立即状态

- 已先执行本地回滚，撤销本轮临时性错误修改（前端端口契约试改）。
- 本文仅记录问题与改进方向，暂未在本文件中直接改动 DSL 语义规则。
