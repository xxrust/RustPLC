# Delivery Corpus 三项目自测契约 v2

## 1. v2 修订结论

Delivery Corpus 是统一 runner 驱动的三项目交付验收 harness。它验证工作台能发现、执行、审查并如实呈现三个独立 delivery project，同时保留每个项目真实的 release blocker。

v2 修正四个模型问题：

1. 语料固定为仓库中已有、已有编译或回归依据的 `module/station/line` 三类 source，满足 registry 的层级覆盖要求。
2. `harness_status` 表示工作台和 runner 是否正确执行；`delivery_status` 表示具体项目是否具备交付条件。真实 blocker 可以令项目 `blocked`，同时令 corpus harness 保持 `pass`。
3. scenario gate 按每个项目声明的覆盖集验证。runner 不为缺少 fault scenario 的项目伪造场景。
4. 本轮 fresh-checkout 门禁采用可执行代理：所有权引用不依赖 ignored `out/`，仓库级 runner 连续执行两次。真正 clean clone 未执行时，`fresh_checkout.status` 固定为 `not_proven`。

## 2. 固定语料

| project_id | delivery_layer | canonical source | 覆盖职责 | fixture 状态 |
| --- | --- | --- | --- | --- |
| `module.axis_move_blocking_baseline` | `module` | `examples/axis_move_blocking_baseline.plc` | blocking axis action、Pending 生命周期、timeout/reject/motion/safety fault routes | canonical source 已存在；需要封装为独立 delivery project |
| `station.dual_slot_shuttle_press_cell` | `station` | `delivery-projects/station.dual-slot-shuttle-press-cell/source/rustplc.bundle.toml` | 并发 task、carrier/slot、operator front-door、process model、wiring、human holds | delivery fixture 已存在 |
| `line.three_station_assembly` | `line` | `delivery-projects/line.three-station-assembly/source/plc/main.plc` | 三工位顺序、质量分流、并行复位、跨设备 safety/timing/causality、故障回收 | 独立 delivery project 已落盘；scenario runtime 的 semantic sensor guard 缺口保持为 fail |

三个项目分别拥有独立的 manifest、source snapshot、scenario declaration、wiring projection、hold projection 和 run artifact root。项目可以共享编译器、schema、设备库、controller profile 和 runner 代码；项目不得共享可变 source、scenario、result、trace、signature 或 evidence 目录。

`module.stepper_collision_guard` 与 `station.project_scaffold_demo` 是有效的其他 delivery 候选，但它们不属于本契约固定的三项目 corpus。使用两个 station 会失去 `line` 层 registry 覆盖，因此不能满足本契约。

## 3. 两条状态轴

每个项目结果必须同时输出：

- `harness_status`: `pass | fail`
- `delivery_status`: `pass | blocked | fail`

`harness_status = pass` 表示：

- 项目被正确发现并解析。
- 输入在执行前冻结，结果绑定同次运行 digest。
- 所有声明步骤均执行或产生结构化 blocker。
- API/UI 投影与底层证据一致。
- wiring、holds、anomaly、correction、agent events 均通过 schema 与关联性检查。
- runner 没有伪造物理证据、签名、scenario 或 pass 状态。

`delivery_status` 的计算规则：

- `pass`：编译和正式 verification 通过，声明的 scenario/project-check 通过，全部 release prerequisite 持有当前有效人工签名，HIL/物理证据满足项目契约。
- `blocked`：软件执行链可审查，但缺少人工签名、物理 point-check、HIL、硬件时序或已声明 coverage。每个 blocker 均有稳定 id、证据和 remediation。
- `fail`：编译失败、正式 verification 发现错误、证据自相矛盾、输入运行中变化，或 release gate 被绕过。

缺少人工/HIL 证据应产生 `delivery_status = blocked`。只要 blocker 被准确投影，项目和 corpus 的 `harness_status` 可以为 `pass`。

## 4. 目录与所有权契约

目标目录结构：

```text
delivery-projects/
  module.axis-move-blocking-baseline/
    delivery-project.json
    source/
    scenarios/
    wiring/
    release/
  station.dual-slot-shuttle-press-cell/
    delivery-project.json
    source/
    scenarios/
    wiring/
    release/
  line.three-station-assembly/
    delivery-project.json
    source/
    scenarios/
    wiring/
    release/

out/delivery-corpus-selftest/<corpus_run_id>/
  input-snapshots/<project_id>/input-manifest.json
  projects/<project_id>/
    result.json
    agent-events.jsonl
    anomalies.json
    corrections.json
    compile/
    verification/
    scenarios/
    project-check/
    wiring/
    holds/
  corpus-result.json
```

`out/delivery-corpus-selftest/<corpus_run_id>/` 是本次运行唯一可写证据根。runner 只读取当前 run 的输出作为判定依据。

`project_independence` gate 必须检查：

1. 三个 `project_id` 唯一，delivery layer 恰好覆盖 `module/station/line`。
2. 三个项目根目录互不包含，run output root 互不重叠。
3. `source_entry`、`system_contract`、scenario、wiring 和 release 所有权引用解析到项目根，或解析到显式只读公共 allowlist。
4. authored source 不通过 include、bundle fragment 或 artifact reference 引用其他 corpus 项目。
5. 除 compiler、schema、设备库、controller profile 和 runner 版本外，同一路径不能同时归属于两个项目的 authored input。
6. 任一项目失败时，其余项目继续产生独立 `result.json`；aggregate 不使用一个项目的证据替代另一个项目。

稳定错误码包括：

- `CORPUS_LAYER_COVERAGE_MISSING`
- `CORPUS_PROJECT_ID_DUPLICATE`
- `CORPUS_PROJECT_ROOT_OVERLAP`
- `CORPUS_CROSS_PROJECT_SOURCE_REF`
- `CORPUS_SHARED_MUTABLE_OUTPUT`
- `CORPUS_INPUT_OWNERSHIP_CONFLICT`

## 5. 输入快照

每个项目在任何编译或 scenario 命令前生成 `input-manifest.json`：

```json
{
  "schema_version": 2,
  "corpus_run_id": "<run-id>",
  "project_id": "module.axis_move_blocking_baseline",
  "source_commit": "<git-sha>",
  "git": {
    "dirty_worktree": true,
    "changed_paths": []
  },
  "files": [
    {
      "path": "delivery-projects/module.axis-move-blocking-baseline/source/main.plc",
      "role": "source_entry",
      "sha256": "<lowercase-hex>",
      "size_bytes": 1
    }
  ],
  "runner_version": "<version>",
  "compiler_version": "<version>"
}
```

规则：

- `files` 按 workspace-relative path 排序。
- digest 固定为 SHA-256，normalization 策略进入 manifest。
- pipeline 结束后重新计算输入 digest；变化产生 `INPUT_CHANGED_DURING_RUN`。
- dirty worktree 是证据字段。本轮允许 dirty run，但 corpus result 必须列出 changed paths，不能把它表述成 clean checkout。
- 项目 `result.json` 保存 input manifest 自身的 SHA-256。

## 6. 统一 Runner

仓库级入口固定为：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run_delivery_project_corpus.ps1
```

执行链：

```text
preflight
  -> discover exactly 3 fixed projects
  -> module/station/line coverage gate
  -> project independence gate
  -> freeze 3 input snapshots
  -> execute all project pipelines
  -> recheck input digests
  -> validate chronology and evidence links
  -> aggregate harness_status and delivery_status
  -> write repeatability report
```

项目 pipeline：

```text
compile
  -> safety / liveness / timing / causality
  -> declared scenario validation and execution
  -> project-check
  -> wiring projection validation
  -> human hold and release projection validation
  -> anomaly / correction / agent-event validation
  -> project result
```

每一步记录 `name`、`command`、时间、`elapsed_ms`、`exit_code`、`status`、stdout/stderr、artifact refs、`error_code` 和 remediation。子进程结束后立即读取 exit code；required step 失败后只继续执行安全的诊断和证据收集步骤。

## 7. 编译与四类 Verification

三个项目均须生成 compile report、IR artifact 和 safety/liveness/timing/causality 四类报告。报告绑定 `project_id`、source commit、input manifest digest、report SHA-256、compiler version 和 same-run freshness。

| 类别 | 结构化证据 |
| --- | --- |
| Safety | safety report、规则覆盖、diagnostics |
| Liveness | liveness report、等待与循环诊断 |
| Timing | timing report、局部与全局路径 |
| Causality | causality report、dataflow 链路诊断 |

formal verification 的 pass/fail 进入 `delivery_status`。harness oracle 检查报告确实由本次输入产生、schema 可解析、状态没有被改写。缺失或 stale 报告使 `harness_status = fail`；真实 verification error 使 `delivery_status = fail`。

## 8. 项目声明的 Scenario Coverage

scenario coverage 由项目 manifest 明确声明，runner 只验证声明集合：

| project_id | required coverage |
| --- | --- |
| `module.axis_move_blocking_baseline` | 至少 nominal Pending -> Done，以及 manifest 实际列出的 fault-route scenarios |
| `station.dual_slot_shuttle_press_cell` | manifest 列出的 startup、nominal、axis/cylinder fault、operator rejection/recovery scenarios |
| `line.three_station_assembly` | 至少 good-part、reject-part、timeout recovery 中 manifest 实际列出的场景 |

场景尚未 authored 时，项目以稳定 blocker 记录缺口，`delivery_status = blocked`。runner 保持 `harness_status = pass` 的条件是 blocker 与真实缺失一致，且没有合成 trace 或假场景。

scenario gate 检查：schema、source binding、声明 milestone、trace 与 nominal 的可区分性、current-run output root、trace digest 和 source commit。warning 必须保留在结果中；例如 held-high input 或风险类 warning 不能被静默丢弃。

`project-check` 对每个项目独立运行。required steps 至少包含 `compile_verify`、`state_proof_check`、`sequence_lint`、`scenario_doctor`、`no_board_gate`；项目声明 process model 或 intent contract 时追加对应步骤。runner 记录真实 exit code，不通过重命名、跳过或降级为 warning 制造 pass。

## 9. Wiring、Physical Evidence 与 Human Holds

每个项目必须提供可审查 wiring projection，字段包括 controller、channel/point、alias、direction、device terminal、signal type、safe state 和 provenance。gate 检查点位唯一性、方向、required port binding、输出 safe state 和 source commit。

wiring schema 可以从项目拓扑生成；物理 observation 只能由可归属的人类会话追加。基础 corpus run 期望未实测点为 `human_action_required`。measurement、photo、trace、note 的 API contract test 使用隔离 evidence store 验证角色权限、append-only 和 evidence digest staleness。

每个项目的 release projection 至少包含：

- `wiring_review`
- `point_check_completion`
- `safety_review`
- `hil_review`
- `release_approval`

签名必须包含 user identity、role、timestamp、source commit、digest set、decision 和 comment。共享静态 bearer token 不构成可归属人类身份。missing、rejected、stale 或 blocked prerequisite 必须阻止 release approval。

自动化测试证明 gate 机制；它不代表真实项目获得人工 release approval。无物理/HIL/签名证据时，正确结果是 `delivery_status = blocked` 与 `harness_status = pass`。

## 10. Anomaly、Correction 与 Agent Events

每个项目 run 输出 `anomalies.json`、`corrections.json` 和 append-only `agent-events.jsonl`。事件至少包含项目、agent、连续 sequence、起止时间、phase、action、result、retry index、route 和 artifact refs。

chronology oracle 检查：

- sequence 从 1 连续递增，时间戳不逆序。
- failure、retry、route switch 和 correction 均有对应事件。
- anomaly 引用 event id 和 current-run evidence path。
- correction 引用 anomaly id、affected files、验证命令和验证结果。
- 同一错误连续尝试超过 3 次且没有 root-cause event 时，产生 `EXCESSIVE_BLIND_RETRY` 并令 `harness_status = fail`。

## 11. Fresh-checkout 与 Repeatability

本轮可执行代理门禁包含两部分：

1. 所有权可移植性：项目 manifest 的 source、scenario、wiring、release 和 contract 引用均来自版本化仓库文件，不依赖 ignored `out/`。历史 `out/` 引用只能作为 supplemental provenance，缺失时不能影响 discovery 或 runner 执行。
2. 仓库级重复执行：在同一 source commit 上连续运行两次统一 runner；两次均使用新的 `corpus_run_id`，并比较项目集合、required step 集合、schema、harness verdict 和 delivery blocker 集合。时间戳、run id 和性能测量值允许变化。

真正 clean clone/临时 checkout 的状态单独记录：

```json
{
  "fresh_checkout": {
    "status": "not_proven",
    "reason": "A clean clone was not executed in this run.",
    "ownership_portability": "pass",
    "repository_repeat_runs": "pass"
  }
}
```

只有在新的 clean clone 或等价 clean worktree 中实际执行完整 runner 后，`fresh_checkout.status` 才能写为 `pass`。本轮代理门禁通过不能改写这一事实。

## 12. Result Schema

项目结果核心字段：

```json
{
  "schema_version": 2,
  "corpus_run_id": "<run-id>",
  "project_id": "line.three_station_assembly",
  "delivery_layer": "line",
  "harness_status": "pass",
  "delivery_status": "blocked",
  "source_commit": "<git-sha>",
  "input_set_sha256": "<config-source-review-digest>",
  "source_set_sha256": "<source-only-digest>",
  "steps": [],
  "verification": {},
  "scenario_summary": {},
  "wiring_summary": {},
  "hold_summary": {},
  "blockers": [],
  "warnings": [],
  "freshness": "same_run",
  "error_code": null
}
```

Corpus result 核心字段：

```json
{
  "schema_version": 2,
  "corpus_run_id": "<run-id>",
  "harness_status": "pass",
  "source_commit": "<git-sha>",
  "dirty_worktree": true,
  "project_count": 3,
  "layer_coverage": ["module", "station", "line"],
  "project_results": [
    { "project_id": "module.axis_move_blocking_baseline", "delivery_status": "blocked", "harness_status": "pass", "result_ref": "..." },
    { "project_id": "station.dual_slot_shuttle_press_cell", "delivery_status": "fail", "harness_status": "pass", "result_ref": "..." },
    { "project_id": "line.three_station_assembly", "delivery_status": "fail", "harness_status": "pass", "result_ref": "..." }
  ],
  "fresh_checkout": {
    "status": "not_proven",
    "ownership_portability": "pass",
    "repository_repeat_runs": "pass"
  },
  "error_code": null
}
```

示例中的 `blocked` 是允许结果，不是预写 verdict。runner 必须从 current-run evidence 和 hold prerequisites 计算实际 `delivery_status`。

## 13. 验收阈值

`harness_status = pass` 需要同时满足：

| 指标 | 阈值 |
| --- | --- |
| 固定项目发现 | 恰好 3 |
| delivery layer coverage | module/station/line 各 1 |
| unique project id | 3/3 |
| cross-project ownership violation | 0 |
| shared mutable output violation | 0 |
| input snapshot completeness | 100% |
| input changed during run | 0 |
| current-run compile/IR report presence | 3/3 |
| 四类 verification report freshness | 100% |
| declared scenario gate coverage | 100% 或结构化 blocker |
| project-check result presence | 3/3 |
| wiring schema/provenance validity | 3/3 |
| missing physical evidence 被投影为 hold | 100% |
| release prerequisite bypass | 0 |
| signature attribution violation | 0 |
| anomaly 有 evidence 引用 | 100% |
| correction 有复验结果 | 100% |
| agent chronology error | 0 |
| unresolved blind retry violation | 0 |
| current-run artifact freshness | 100% |
| ownership references independent of ignored `out/` | 3/3 |
| repository-level consecutive runner passes | 2 |

Corpus harness 的计算式：

```text
layer_coverage_gate
AND independence_gate
AND all(project_result.harness_status == pass)
AND all(harness_acceptance_thresholds satisfied)
AND ownership_portability == pass
AND repository_repeat_runs == pass
```

`all(project_result.delivery_status == pass)` 不属于 corpus harness pass 条件。aggregate 必须汇总 `pass/blocked/fail` 项目数量，并保留每个 blocker 的来源、evidence 和 remediation。

以下行为直接令 `harness_status = fail`：跨项目借用 result/trace、缺失或 stale current-run 报告、伪造 scenario/物理 observation/签名、吞掉 warning、绕过 release prerequisite、把 ignored `out/` 当所有权输入、修改真实 `delivery_status` 来制造 corpus pass。
