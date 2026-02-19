# Scenario Assetization（模板库 + 覆盖策略 + feedback.json）

日期：2026-02-19

## 1. 模板库资产

新增项目级模板库目录：

- `scenarios/templates/metadata.json`（元数据契约）
- `scenarios/templates/nominal_cycle.yaml`
- `scenarios/templates/fault_sensor_stuck.yaml`

`metadata.json` 契约（schema_version=1）：

- `templates[].id`
- `templates[].path`
- `templates[].kind`（如 `nominal` / `fault`）
- `templates[].description`
- `templates[].parameters`

## 2. scenario-gen 新能力

命令：

```bash
cargo run --release -- scenario-gen \
  --plc examples/assembly_station.plc \
  --config examples/scenario_gen/basic.yaml \
  --out-dir out/scenario_gen \
  --coverage-mode boundary-first \
  --template-library scenarios/templates/metadata.json
```

支持：

- `--coverage-mode pairwise|boundary-first|risk-first`
- `--dry-run`（仅输出 `summary.json`，不落地 `scenario_*.yaml`）
- `--template-library <metadata.json>`

## 3. 输出格式变更（summary.json）

### 3.1 旧格式（关键字段）

- `schema_version`
- `plc`
- `config`
- `count`
- `cases[]`

### 3.2 新格式（新增字段）

- `coverage_mode`
- `dry_run`
- `template_library`
- `templates[]`（模板库元数据快照）
- `cases[].template_id`

## 4. sim-regress 新增 feedback.json

当 `sim-regress --minimize-failure` 启用时，输出：

- `artifacts_dir/feedback.json`

schema（v1）：

- `schema_version`
- `total_failures`
- `feedback[]`
  - `plc`
  - `scenario`
  - `failure_kind`
  - `template_hint`
  - `parameter_hints[]`
  - `minimized_scenario_path`（可选）

该文件用于把最小化失败直接回灌到模板选择与参数调优流程。
