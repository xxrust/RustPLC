# 测角机上料器验证说明

## 必跑检查

```bash
cargo run --release --bin rust_plc -- out/wafer_loader_standard_project/rustplc.bundle.toml --no-print-ir
```

```bash
cargo run --release --bin rust_plc -- process-model-check out/wafer_loader_standard_project/rustplc.bundle.toml --model out/wafer_loader_standard_project/process_model/process_operation_model.toml --output json
```

```bash
cargo run --release --bin rust_plc -- state-proof-check out/wafer_loader_standard_project/rustplc.bundle.toml --output json
```

```bash
cargo run --release --bin rust_plc -- project-check out/wafer_loader_standard_project/rustplc.bundle.toml --scenario out/wafer_loader_standard_project/scenarios/nominal/normal.yaml --out-dir out/wafer_loader_standard_project/out/check --require-process-model --output json
```

## 验证口径

- compile/verification 必须通过 safety、liveness、timing、causality。
- `process-model-check` 必须通过，且 `expected_operation_count` 与 `actual_operation_count` 都为 10。
- `state-proof-check` 必须通过 `SPF-030/031/032/033/040`：启动回零先命令后反馈、本地传感器不得无限等待、残片失败必须显式人工/恢复边界、真空释放必须有接收方证明、执行类设备必须有自检或机器可读豁免。
- `project-check --require-process-model` 必须包含 `process_model_check` 步骤。

## 当前边界

nominal scenario 当前验证单片流程。多片连续节拍需要上游补料或 scenario 重新置位 `feed_cassette_present`，不得把出料盒有料传感器长保持当作无限源。
