# Retain / Persistent 变量生命周期（开发态）

日期：2026-02-19

## 1. 目标

`sim-plc` 支持对选定通道做 retain/persistent：

- 进程重启后恢复上次值；
- 持久化文件带版本 + 校验和；
- 文件损坏时自动回退到配置默认值（不中断仿真流程）。

## 2. CLI 参数

```bash
cargo run --release -- sim-plc examples/force_override_demo.plc \
  --scenario scenarios/force_override_demo/force.yaml \
  --out out/trace.jsonl \
  --retain-config out/retain.toml \
  --retain-state out/retain_state.json
```

- `--retain-config`：必填（启用 retain 时）
- `--retain-state`：可选，默认 `retain_state.json`（与 `--out` 同目录）

## 3. retain 配置格式（TOML）

```toml
schema_version = 1

[digital_inputs]
di0 = false

[analog_inputs]
ai0 = 0.0

[digital_outputs]
do1 = false

[analog_outputs]
ao0 = 0.0
```

说明：

- key 支持 `0` 或前缀写法（`di0/ai0/do0/ao0`）；
- value 是默认值（当状态文件缺失或损坏时使用）。

## 4. 状态文件格式（JSON）

```json
{
  "schema_version": 1,
  "checksum_sha256": "<sha256(payload)>",
  "payload": {
    "schema_version": 1,
    "digital_inputs": {"0": true},
    "analog_inputs": {},
    "digital_outputs": {},
    "analog_outputs": {}
  }
}
```

校验规则：

1. `schema_version` 必须匹配；
2. `checksum_sha256` 必须与 `payload` 实际哈希一致；
3. 失败则回退配置默认值并打印 `[RET-201]` 提示。

## 5. 运行时语义

- DI/AI：在 `at_ms=0` 注入为启动输入；
- DO/AO：在 `at_ms=0` 注入一拍 force 引导，并在 `at_ms=tick_ms` 自动 clear，让程序后续写入接管；
- 运行结束后回写新状态文件。

## 6. 迁移与兼容策略

- 当前仅支持 `schema_version = 1`；
- 后续版本变更时，建议采用“读旧写新”的离线迁移脚本；
- 若遇到不支持版本，系统按损坏处理并回退默认值，不阻塞本次仿真。
