# 测角机上料器验证说明

## 必跑检查

```bash
cargo run --release --bin rust_plc -- out/wafer_loader_standard_project/rustplc.bundle.toml --no-print-ir
```

```bash
cargo run --release --bin rust_plc -- process-model-check out/wafer_loader_standard_project/rustplc.bundle.toml --model out/wafer_loader_standard_project/process_model/process_operation_model.toml --output json
```

```bash
cargo run --release --bin rust_plc -- project-check out/wafer_loader_standard_project/rustplc.bundle.toml --scenario out/wafer_loader_standard_project/scenarios/nominal/normal.yaml --out-dir out/wafer_loader_standard_project/out/check --require-process-model --output json
```

## 验证口径

- compile/verification 必须通过 safety、liveness、timing、causality。
- `process-model-check` 必须通过，且 `expected_operation_count` 与 `actual_operation_count` 都为 9。
- `project-check --require-process-model` 必须包含 `process_model_check` 步骤。

## 当前边界

nominal scenario 当前验证单片流程。多片连续节拍需要上游补料或 scenario 重新声明 `feed_cassette_has_seed`，不得把出料盒隐式当作无限源。
