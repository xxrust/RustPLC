# Component Library + Fault Model Migration

对应详细文档：

- `docs/component_library_fault_model_migration.md`

## 快速命令

```bash
# 1) 校验拓扑
cargo run -- component-topology-validate examples/component_model/topology.json --output json

# 2) 校验场景
cargo run -- component-scenario-validate examples/component_model/scenario_faults.json --output json

# 3) 运行组件级仿真
cargo run -- component-sim examples/component_model/topology.json \
  --scenario examples/component_model/scenario_faults.json \
  --out out/component_fault_trace.jsonl \
  --fault-audit-out out/component_fault_audit.jsonl \
  --diagnosis-out out/component_fault_diagnosis.json \
  --keypoints-out out/component_fault_keypoints.json \
  --output json
```

## 关键变更

- 旧 `faults.sensor_stuck` / `forces` 已不再作为组件场景接口
- 新场景统一使用 `component_faults`
- 同 tick 多异常冲突按固定优先级执行，结果可审计
- 诊断输出增加了“注入证据 + 程序行为证据”区分
