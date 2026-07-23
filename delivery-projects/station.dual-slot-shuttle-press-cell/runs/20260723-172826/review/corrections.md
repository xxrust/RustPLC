# Corrections

## COR-001: state-proof 未覆盖装载夹具残件

- 发现阶段：主 agent 修正，首次完整 compile 后的 `state-proof` 自动门禁。
- 修正前证据：`[state-proof:SPF-021] automatic workpiece flow starts without any proven startup check/cleanup/manual confirmation for residual parts`，首个自动工件步骤为 `load_unload.acquire_slot0`。
- 根因：`collect_critical_residual_symbols()` 把所有 `workpiece_holder` 与 `workpiece_carrier` 视为关键残件端点；`config/state_proof.toml` 已覆盖 `load_nest`、`press_nest` 和 `shuttle_tray`，但遗漏 `load_gripper` 与 ingress carrier `raw_infeed`。诊断只报告聚合失败，没有列出未覆盖 symbol，增加了一次源码检查成本。
- 分类：`task-implementation-gap`。验证器规则与系统的残件风险一致，不属于 gate 误报。
- 修正：为 `load_gripper` 增加空夹具基线；为 `raw_infeed` 增加有限批次基线，要求现场证明 slot[0]/slot[1] 各有一件且其他槽位不存在未登记残件。两项均带 `reason` 与 `proof_basis`。
- 语义边界：该配置只声明可审计的启动基线，不把 scenario seed 或内部布尔量当作物理空夹具证明。
- 复验：待执行 `state-proof-check ... --output json` 与完整 compile。

## COR-002: 轴运动前缺失 enable 前置状态

- 发现阶段：`SPF-021` 修正后，完整 compile 的 safety verification。
- 修正前证据：设备库约束 `axis_shuttle.enable.off conflicts_with axis_shuttle.pulse.active` 在 `startup_self_check.move_to_check_position` 的 Pending 状态被违反。
- 根因：Agent B 与首次主代理语法修正只生成了 `axis.move_absolute`，没有像 canonical stepper example 一样先执行 `set axis_shuttle.enable on`。设备默认状态为 `enable.off`，而 safety lowering 会在轴动作期间令 `pulse.active` 成立。
- 分类：`task-implementation-gap`；同时 `dsl-capabilities` 没有导出这一设备动作前置示例，属于已有 `public-surface-gap` 的具体表现。
- 修正：在 startup self-check 中新增 `enable_shuttle_axis` step，所有生产和恢复轴动作都发生在该启动证明之后。
- 复验：待重新执行完整 compile；不得通过删除设备库安全约束来消除报错。

## COR-003: 未实现的 rejected workpiece 契约

- 发现阶段：首次 compile/verification 通过后的 warning 审查。
- 修正前证据：Safety 报告警告 `insert_part` 声明了异常终态 `rejected` 与异常出口 `reject_outfeed`，但不存在任何可达 finish 满足该契约。
- 根因：Agent B 从通用 workpiece 模板保留了 reject 声明；冻结系统契约只要求故障时保留精确 token 位置并进入人工协助，明确禁止静默 finish/discard。
- 分类：`task-implementation-gap`，属于 authored contract 漂移。
- 修正：删除未使用的 `abnormal_terminal_states`、`abnormal_egress_sites` 和 `reject_outfeed` location。保留 `reject_lamp`，它表达非法启动的操作员反馈，与工件拒料无关。
- 复验：待重新 compile，预期 workpiece abnormal-terminal warnings 消失。

## COR-004: acquire 后直接 mount 导致 token 复制与夹具占用

- 发现阶段：process-operation reverse audit 与 runtime-core 语义审查。
- 修正前证据：`acquire` 把 token 移到 `load_gripper`；`WorkpieceMount` 只复用“目标 slot 上的 free-standing 同类型 token”，目标 slot 无 token 时会新建 token。原流程因此保留夹具 token并在 tray slot 新建另一 token，第二次 acquire 会面临夹具容量冲突。
- 根因：Agent B 将业务语义“从夹具装到槽位”错误映射为单独 `mount`，遗漏 `transfer load_gripper -> shuttle_tray.slot[n]`。compile 的静态验证未暴露该 source-holder 丢失。
- 分类：`task-implementation-gap`，同时暴露 mount source 未进入 IR 的 `code-gap`/可诊断性风险。
- 修正：每个 mount step 先执行 holder-to-slot transfer，再执行 mount；更新注释使其描述真实 token 路径。
- 额外修正：为末端 `finish_slot1` 与 `mark_carrier_pressed` 增加显式后继 hold step，保证末端 effects 进入 transition-based runtime/process model。
- 复验：待重新 compile、operation-model、nominal sim；必须确认 active token 数量最终为 0 且无第三次 acquire。

## COR-005: 第一件 finish 与第二件 unmount 的无依据相邻串行

- 发现阶段：修正 token 路径后的 `operation-model` v2 audit。
- 修正前证据：`OP-002 load_unload.unmount_slot1`，checker 认为它在 `finish_slot0` 后无共享端点/资源而被串行化。
- 真实约束：第二次 transfer 之前必须先结束 good_outfeed 上的第一件，否则两个 active token 会让 endpoint-based finish 产生实例歧义；同时 unmount 第二件是下一次 load_nest transfer 的直接前置。
- 修正：把 `finish_slot0` 与 `unmount_slot1` 合并为 `finish_slot0_and_unmount_slot1` compound operation，按顺序先 finish 第一件、再把第二件取回 load_nest。该 operation 与前一操作共享 `good_outfeed`，与后一操作共享 `load_nest`。
- 分类：`task-implementation-gap`；修正后应消除 OP-002，而不使用普通 guard 伪造 predecessor proof。
- 复验：待运行 operation-model v3 与 process-model-check。

## COR-006: source-side process model 缺少 operations

- 发现阶段：首次 `process-model-check`。
- 修正前证据：`[OPMODEL-010] missing field operations`；原文件只写了高层 `operation_classes`，且 operation IDs 与当前 IR 不一致。
- 根因：Agent B 把工艺意图摘要误当成完整 schema，没有生成 `[[operations]]`、contract key、admission、effect 和 predecessor 结构。
- 分类：`public-surface-gap` 为主，Agent B 拿到的 capability 输出没有完整 process model schema；同时属于 `task-implementation-gap`。
- 修正过程：先修正真实 task/workpiece flow，连续生成 v1/v2/v3 reverse audit；v1 暴露 4 个 OP-002，v2 剩 1 个 OP-002 + 1 个 OP-003，v3 在程序修正后只剩 OP-003。主 agent 按冻结 `process-operation-intent.md` 审核 v3 的 10 个 semantic contract，再写回 source-side TOML。
- 证据边界：reverse output 只用于迁移/审计，不能证明源侧意图；审定依据仍是冻结 system/process intent。
- 预期复验：`process-model-check` 不再出现 OPMODEL/OPREF/OP-002，只保留已知 `transform carrier` 的 OP-003 工具限制。

## COR-007: axis enable 缺少可执行 I/O 映射

- 发现阶段：全部 scenario 的首次 `scenario-validate` 与 nominal `sim-plc`。
- 修正前证据：`unable to resolve a unique physical digital output for device axis_shuttle (state startup_self_check.enable_shuttle_axis)`。
- 根因：COR-002 补了设备语义前置动作，但 controller/topology 没有把任何 PLC output 映射到 `axis_shuttle.enable`。compile/safety 能验证抽象状态，runtime bridge 需要唯一物理 DO。
- 分类：`task-implementation-gap`，同时说明 compile-only 不能代表 executable runtime contract。
- 修正：新增 controller alias `axis_enable_cmd: Y6`，并添加 `plc_main.axis_enable_cmd -> axis_shuttle.enable` 的 `driven_by` relation。
- 复验：待重新运行 scenario-validate 与 sim-plc。

## COR-008: 启动首个 absolute move 缺少 homing proof

- 发现阶段：补齐 axis enable I/O 后的 nominal `scenario-validate`/`sim-plc`。
- 修正前证据：runtime error `AxisNotHomed { target: "axis_shuttle" }`。
- 根因：startup self-check 首条轴命令是 absolute move，IR 保留 `require_homed=true`；项目没有可执行的 `axis.home` 原语或已证明 home 状态。
- 分类：`task-implementation-gap`，并暴露“设计文档出现 axis.home，但当前 parser/runtime 无该原语”的 `public-surface-gap`。
- 修正：把检查位动作改为受控 `axis.move_relative(distance: 10)`；当前语义分析会把成功 relative move 作为 homing proof，随后 absolute return-to-load 的 runtime guard 被静态消解。
- 模型边界：这证明当前 RustPLC 执行模型中的位置基线，不等价于真实硬件回零开关流程；真实设备交付仍需独立 home device contract。
- 复验：待重跑 compile、scenario-validate 和 sim-plc。

## COR-009: 关键片段仍以 placeholder 命名而未进入 bundle

- 发现阶段：nominal trace 业务审查与 flowchart task inventory。
- 修正前证据：flowchart 只有 11 个 task，缺少 `supervision`、`manual_assist`、`hmi_feedback`；`03_constraints/_placeholder.plc` 的 resource claims 也未进入编译。bundle loader 忽略以 `_` 开头的 scaffold placeholder。
- 影响：AC-10 的危险资源排斥没有真实参与 verification；`ready_next_cycle` 无发布者；manual/HMI 只存在于未编译文件。
- 分类：`task-implementation-gap`，同时说明 scaffold placeholder replacement gate 缺少硬校验。
- 修正：将四个文件改为真实编译单元 `resources.plc`、`supervision.plc`、`manual_assist.plc`、`hmi_feedback.plc`；新增 `ready_next_cycle` 状态及发布动作。
- 复验：待确认 flowchart task 数增加、resource claims 出现在 IR、Safety 仍通过、nominal trace 到达第七里程碑。

## COR-010: 末端 step 动作未执行且 fault 分支线性穿透

- 发现阶段：nominal `sim-plc` 虽 exit 0，但 trace 在 `require_startup_ready` 超时并进入 illegal-start；flowchart 显示多个关键末端 step 没有 outgoing transition。
- 根因：runtime 执行动作依附 transition；最后 step 无后继时，其 statements 不执行。多个 fault task 还把互斥分支写成同一 task 的顺序 steps，导致错误 fallthrough。
- 修正：为 startup、operator、shuttle、supervision、manual、HMI 的发布 step 增加显式 hold 后继；fault clear/latched/reject steps 使用 `goto` 汇入 terminal hold，避免串入下一故障分支。
- 分类：`task-implementation-gap`；`sim-plc` 的零退出码只表示没有 runtime error，不是业务里程碑 oracle，属于 harness/oracle 缺口。
- 复验：待用同次 trace 检查七个 milestone 与目标 fault step，而不是只检查进程退出码。

## COR-011: illegal-start 场景混入 startup safety fault

- 发现阶段：五个非 nominal trace 的 intent-doctor 映射。
- 修正前证据：场景先进入 `illegal_start.reject_not_ready`，随后因 `safety_chain=false` 又进入 `fault_recovery_safety.startup_safety_fault`。
- 根因：场景同时改变两个独立因果条件，无法把可见拒绝归因到 front-door admission。
- 修正：保持 safety/material/output inputs 健康，显式在 0ms 置低、20ms 置高、80ms 释放；上升沿早于 100ms visible-output self-test 完成点，因此命令发生在 `startup_ok` 发布前。
- 分类：`task-implementation-gap`（scenario 因果设计）。
- 复验：目标 trace 应只进入 illegal-start reject/hold，不进入 startup safety fault。

## COR-012: intent contract 缺失，业务里程碑无法进入统一门禁

- 发现阶段：主 agent 收口 nominal trace 后。
- 修正前证据：source entry 同级不存在 `rustplc.bundle.intent_alignment.contract.json`，`project-check` 无法追加 `intent_alignment`。
- 根因：Agent B 正确拒绝在真实 trace 产生前猜测 evidence anchor，但 hard stop 后没有后续角色完成 trace-backed contract。
- 修正：以 `plc/main.system.md` 为权威 source，计算规范化 lowercase SHA-256；建立 7 个业务 milestone、6 条顺序 edge、1 个 next-cycle postcondition，并绑定同一次 nominal trace 中的精确 transition。
- 复验：`intent-doctor` 映射 41 个唯一 transition，7 个 milestone binding 全部为 `stable`；`project-check` 的 `intent_alignment` step 为 `pass/aligned`。
- 证据边界：当前 trace 只有一个完整周期，`cross_cycle_ready=false`，跨周期稳定性保留为 evidence gap。

## COR-013: 自测 harness 在 Windows PowerShell 5 上连续暴露兼容问题

- 目标：生成可重复的一键自测、逐步日志和 `result.json`。
- 异常与修正：
  1. 环境没有 `pwsh`；入口改为 `powershell.exe -NoProfile -ExecutionPolicy Bypass`。
  2. Windows PowerShell 不接受 `if` 作为圆括号参数表达式；改为预计算 evidence 变量。
  3. .NET Framework 没有 `Path.GetRelativePath`；改用 `System.Uri.MakeRelativeUri`。
  4. PowerShell 数组中的未加括号函数调用与逗号发生参数绑定冲突；所有路径函数调用显式加括号。
  5. `ProcessStartInfo.ArgumentList` 在 .NET Framework 不可用；改为显式 Windows command-line quoting 后写入 `.Arguments`。
  6. `if` 输出管道把单元素数组解包，导致 project-check 的失败步骤 `.Count` 为空；改为先初始化数组，再在语句块内赋值。
  7. acceptance v2 新增注释 oracle 时重复使用了圆括号内 `if`；再次改为预计算 evidence，并将“禁止 `(if ...)`”作为脚本审查规则。
- 失败证据：`harness-runs/20260723-152330` 至 `harness-runs/20260723-153028` 保留部分或完整失败运行。
- 首次最终复验：`harness-runs/20260723-153210` 在 24.896 秒内完成；21 个 step pass、3 个 step known_gap、10 个 required oracle pass、4 个 known_gap oracle；harness status 为 `pass`。
- timing 风险补强复验：`harness-runs/20260723-155536` 增加 static-budget/no-board timing evidence oracle 后仍为 `pass`；静态 warning 被归类为 target-hardware evidence gap。
- 结论：harness 自身试错来自运行时版本假设。最终脚本只依赖 Windows PowerShell 5 可用 API。

## COR-014: plc-gen 公开生成规则不足，导致 Agent B 七次语法试错

- 发现阶段：Agent B 执行日志与主 agent 修正对照。
- 修正前证据：Agent B 连续试错 axis device type、workpiece transform declaration、carrier schema、bundle phase variable placement、axis action syntax 和 route block；公开 skill 只描述能力与约束，没有给出可编译语法骨架。
- 根因分类：`skill-gap/public-surface-gap`。能力清单回答“支持什么”，没有回答“最小正确源码长什么样”。
- 修正：更新 `.codex/skills/plc-gen`：
  - 增加已验证的 `stepper_motor` 与 `axis.move_*` 完整语法。
  - 明确 enable、物理输出映射与 homing proof。
  - 明确 bundle section 和下划线 placeholder 忽略规则。
  - 明确末端 step 需要显式后继才能执行副作用。
  - 明确 carrier `slots` 形状，以及 holder-to-slot 必须先 transfer 再 mount。
  - 增加 fault scenario trace oracle、`trace-doctor` bundle 缺口与 OP-003 判定规则。
- canary 异常：skill 指定的 `out/wafer_loader_project` fixture 不存在，首次 canary 产生 4 个 file-not-found 派生失败。
- canary 修正：在 workflow 中增加 source/scenario preflight；缺失时记录 `CANARY_FIXTURE_MISSING` 并停止该路线，避免误判回归。

## COR-015: skill 修正首次写入了错误的 carrier effect 语法

- 发现阶段：最终独立 reviewer 审查主 agent 的 skill 改动。
- 错误内容：`generation-rules.md` 首稿写成 `effect: transfer load_gripper -> ...` 与 `effect: mount insert_part at ...`。
- 正确证据：当前 specimen 已 compile/runtime 通过的语法为 `effect: transfer from <source> to <target>` 与 `effect: mount <type> on <slot>`；`tests/workpiece_model_phase23.rs` 使用相同语法。
- 风险：错误示例会把本轮要消除的 parser 试错重新写入 skill，属于高风险 `skill-correction regression`。
- 修正：示例改为 `transfer from ... to ...` 和 `mount ... on ...`，并请求 reviewer 基于修正后版本继续复审。

## COR-016: harness pass 被错误解释为完整 acceptance 通过

- 发现阶段：最终独立 reviewer。
- 修正前：runner 只聚合已实现 step/oracle；known-gap 不计 fail，因此输出 `harness=pass / delivery=blocked_by_known_product_gaps`。
- 风险：17 项 acceptance 中未建 oracle 的项目可以静默缺失，形成结构性假阳性。
- 修正：`result.json` 升级为 schema v2，逐项输出 AC-01..AC-17 的 `pass/fail/blocked`；新增 IR concrete slot、resource claim、中文注释、cylinder 高层语义、same-run path 和 manifest contract oracle。
- 预检：manifest 增加 schema、17 个 required IDs、reviewer/gap 状态；runner 记录 acceptance、runner、intent source、manifest 四类 digest。
- 聚合：runner execution 与 acceptance verdict 分离；任何 AC fail 得到 `failed_validation`，blocked 单独计数；acceptance 非 validated 时脚本返回非零。
- 最终复验：`harness-runs/20260723-161549`，11 pass、5 blocked、1 fail；delivery=`failed_validation_with_product_blockers`。result 中 runner/manifest digest 与当前文件逐字规范化摘要一致。

## 最终复验闭环

- COR-001：`state-proof-check` pass，issue_count=0。
- COR-002/COR-007/COR-008：compile 与 nominal axis Pending->Done trace pass。
- COR-003：workpiece abnormal-terminal warning 已消失；当前 timing warning 单独记录。
- COR-004/COR-005：两次 acquire、两个 slot mount/unmount/finish 完成，无第三次 acquire。
- COR-006：process model expected=10、actual=10，仅剩 OP-003。
- COR-009：IR/flow 包含真实 resource、supervision、manual、HMI fragments；underscore placeholder 规则已回写 skill。
- COR-010：七个业务 transition 在同次 nominal trace 中各出现一次。
- COR-011：illegal-start before-ready reject/hold trace pass；running/faulted front-door 仍为 AC-09 fail。
- COR-012：7 个 intent binding stable，project-check intent verdict=`aligned`。
- COR-013/COR-016：Windows PowerShell 5 runner execution pass，acceptance 非 validated 时返回非零。
- COR-014/COR-015：skill canonical syntax 已修正；workpiece focused suite 53 passed。

## COR-017: 持续 front-door 从跨 task 监控修正为单 task 状态分类循环

- 发现阶段：AC-09 优化设计。
- 初始候选：新增 running/faulted 两个监控 task，分别等待第二次 start。
- 风险：监控 task 与 `illegal_start` 的跨 task 回跳会形成 SCC，改变“无跨 task 入边”的 runtime root 推导；并行监控还可能在 fault 与 running 同时成立时重复消费同一命令。
- 子代理修正：`frontdoor_optimization` 将拒绝分类收归唯一 `operator_front_door`，用顺序 `if` 在命令采样时点判断 fault/running/batch-complete/readiness，并在接受或拒绝后回到 rising-edge wait。
- 主 agent 实现：删除独立 `illegal_start` task；新增 `automatic_cycle_active` 与 `start_reject_reason`；拒绝灯保持到下一条 start；one-shot 完成后拒绝第三批次。
- 同次证据：`harness-runs/20260723-164903` 中 before-ready reject tick 2、后续合法 accept tick 100；running reject tick 620；faulted reject tick 150；三条路径均有 IR reason + reject_lamp action。
- 结果：AC-09 从 fail 提升为 pass，主要 active task 索引保持 `startup=0/operator=1/load=2/shuttle=3/press=4/supervision=5/manual=6`。

## COR-018: residual/manual 与 PLC 内部 HMI code 从文档义务修正为可执行 producer

- 发现阶段：最终 reviewer 的残余风险清单与 `residual_hmi_optimization` 独立审查。
- 修正前：`residual_part_detected` 没有 producer，`manual_assist` 永久等待；HMI task 只有灯与蜂鸣器，具体 status/fault code 没有变量和写入点。
- 修正：增加物理 `residual_present` X7、操作员 `operator_empty_confirm` X8；startup 先证明残件输入清零再消费人工确认；manual task 从真实残件输入进入，发布 status 50/fault 210，待现场清零与人工确认后发布 ack。各 fault entry 写入唯一 code，startup/accept/batch-complete 写入机器状态。
- 可执行证据：residual trace 依次出现 detect tick 0、physical clear tick 30、manual confirm tick 40、startup ready tick 50；同次 IR 校验 12 个 status/fault producer 全部存在。
- 边界：runtime 仍不能注入并保留任意非 ingress 位置的既有 token；PLC 内部 code 尚无目标 HMI register/transport/consumer binding。两项保留为产品 blocker。

## COR-019: 子代理建议中的 int code 与复合 wait 未通过真实编译

- 发现阶段：优化后首次整包 compile。
- 失败 1：`compute machine_status_code = 10` 等裸数字被表达式系统推断为 float，赋给 `int` 触发 40 余条 `type_mismatch`。
- 失败 2：`wait: residual_present == false OR manual_acknowledged == true` 的第二个 operand 被 wait 约束解析为设备引用，触发 `undefined_reference manual_acknowledged`。
- 归因：子代理只读审查正确识别了业务边界，但没有用当前编译器验证 `int` 字面量与复合 wait 的公开语法；这是 `skill/public-surface-gap + review-without-compile`，也是主 agent 在落盘前未先做最小语法探针的执行失误。
- 修正：status/reason code 使用 float 数值枚举；startup 拆为两个显式步骤：`wait residual_present == false` 与 `wait rising_edge(operator_empty_confirm)`。
- 复验：整包 Safety/Liveness/Timing/Causality 通过，三个新增 scenario validate 通过，六条目标 trace 全部执行成功。

## COR-020: plc-gen 主文件仍残留错误 carrier transfer 简写

- 发现阶段：继续优化时对 skill 主文件做精确检索。
- 残留内容：`transfer <holder> -> <carrier>.slot[n]`。
- 风险：虽然 references 中的示例已经修正，主 `SKILL.md` 仍会把不存在的简写提供给后续 agent，重复触发 parser 试错。
- 修正：统一为 `effect: transfer from <holder> to <carrier>.slot[n]`，并同时给出 `effect: mount <type> on <carrier>.slot[n]`。
- canary 状态：`out/wafer_loader_project` 仍不存在，记录 `CANARY_FIXTURE_MISSING`；本轮使用当前 compile 通过 specimen 与既有 53 项 workpiece 回归作为语法证据。

## 优化复验闭环

- 最新 authoritative run：`harness-runs/20260723-172826`。
- Harness execution：pass；required step/oracle failure 均为 0。
- Acceptance：12 pass、5 blocked、0 fail；delivery=`blocked`。
- AC-09：三种 start 拒绝与持续服务通过。
- AC-16：state-proof 与 residual/manual 可执行路径通过；exact token injection 单独保留 blocker。
- Intent：7 个 binding stable，project-check intent=`aligned`，`cross_cycle_ready=false`。

## COR-021: blocking wait 与同 step goto 组合导致门禁被绕过

- 发现阶段：根据最终 reviewer 修正 manual ack 因果后生成 startup trace。
- 首次写法：`confirm_empty_baseline` 同时包含 `wait: rising_edge(operator_empty_confirm)` 和无条件 `goto recheck_residual_clear`。
- 异常：compile 与四类 verification 全部通过，但 trace 在 tick 0 直接从 confirm step 跳到 recheck；X8 的 100ms 上升沿没有参与 startup。
- 根因：显式 goto 成为该 step 的可执行 transition，覆盖了预期 blocking wait 的自然完成路径。
- 修正：blocking step 只保留 wait；残件分支在 `wait_manual_ack` 后增加独立 `route_after_manual_ack`，用该非阻塞 step 汇入共同 recheck。
- 复验：nominal startup 在 X8 tick 10 后推进；residual 场景中 manual ack tick 40，startup 在 tick 41 消费 `manual_acknowledged`，随后复查 X7 并于 tick 51 发布 ready。
- skill 修正：`plc-gen` 增加“Blocking Steps Use Natural Completion” hard guardrail 与 wrong/right 示例。

## COR-022: final reviewer 指出 acceptance 因果与证据冻结不足

- reviewer 首轮异常：宽范围只读复核持续较长时间且没有落盘工件；主 agent 中断后限定到 result、最新 run、核心 DSL 与两份子代理报告，第二回合完成 `optimization-final-review.md`。
- 有效 findings：manual ack 未进入 startup admission；AC-09 缺 recycle/重复 edge oracle；external-reinit schema gap 被 nominal `aligned` 文字掩盖；同次输入缺少完整摘要。
- 修正：startup residual 分支显式等待 manual ack 并复查 physical input；running 场景增加 batch-complete 第三 edge，faulted 场景增加第二 edge，三类拒绝都断言 `18 -> 0` recycle；AC-12 同时报告 OP-003 与 external-reinit schema blocker；每次 run 生成逐文件 `input-manifest.json` 并写入 result digest。
- 评价：reviewer 首回合交付效率不足，限定范围后 finding 质量高，直接发现了一个 harness 假因果和一个输入审计缺口。

## COR-023: input manifest 两次暴露 PowerShell 类型与路径过滤假阳性

- 目标：冻结同次 run 的 definition、bundle fragments、config、contract、process model 与全部 scenario 输入摘要。
- 异常 1：`FileInfo` 与字符串路径混合后直接访问 `.FullName`，PowerShell 5 在 manifest 生成前报空 Path；修正为先统一 `Get-Item` 得到 `FileInfo`。
- 异常 2：排除 generated `implementation/out/` 的正则同时匹配了绝对路径中的顶层 `out/complex_selftest`，导致全部 implementation 输入被排除；原 oracle 只检查 file_count > 0，因此 8 个 definition 文件形成 pass 假阳性。
- 修正：使用 `implementation/out` 的规范化绝对路径前缀做排除；oracle 强制 `implementation_file_count > 0`，并逐项要求 bundle、intent contract、process model 与 9 个 scenario 均存在。
- 最终证据：`harness-runs/20260723-172240/input-manifest.json` 包含 49 个文件，其中 13 PLC、10 YAML、7 TOML、6 JSON、13 Markdown；implementation_file_count=41，missing_required 为空。
- 结论：证据冻结 oracle 本身需要负例验证。仅检查“文件存在/数量大于零”不足以证明覆盖完整输入集合。
