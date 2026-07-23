# RustPLC Autonomous PLC Delivery Workbench 自测报告

日期：2026-07-24

## 结论

Autonomous PLC Delivery Workbench 是 RustPLC 的项目交付与编译器进展接口。三类正式项目在同一个 Cursor/VS Code 风格桌面工作台中展示源码、编译阶段、验证、接线、人工点检、异常修正和放行证据。

当前结论为：

- `harness_status=pass`
- 交付状态：`0 pass / 1 blocked / 2 fail`
- `execution_unattended_verdict=proven`
- `source_authoring_verdict=not_proven`
- `unattended_verdict=not_proven`

工作台、三项目 corpus、重复性、项目搜索、证据深链和双视口浏览器流程已经通过。完整 WCAG 2.1 AA 审计、法规电子签名和文件级无人源码创作归因仍属于开放能力。

## 系统边界

```text
canonical delivery-projects/ (module / station / line)
        |
        v
input manifest + agent provenance + immutable specimen
        |
        v
DSL source -> Parser -> AST -> Semantic -> IR
                                      |-> Safety / Liveness
                                      |-> Timing / Causality
                                      |-> Runtime Bridge / Simulation
                                      `-> Codegen
        |
        v
same-run artifacts -> Problems / Tests / Search / Editor / Inspector
        |
        v
compiler-derived wiring -> human point checks -> holds -> release projection
```

编译器进展由交付项目直接暴露。项目树负责选择 module、station、line；编辑器组承载源码、IR、拓扑和证据；底部 Problems/Tests 面板聚合诊断与测试；检查器承载接线点检和人工责任。系统没有独立 compiler-monitor 页面，也不按项目优劣排序。

## 三项目结果

| 项目 | 交付状态 | Validator | 直接结论 |
| --- | --- | ---: | --- |
| `module.axis_move_blocking_baseline` | `blocked` | 898 checks / 0 errors | 编译、scenario、project-check 通过；HIL、物理点检和人工 hold 尚未完成 |
| `station.dual_slot_shuttle_press_cell` | `fail` | 1587 checks / 0 errors | scenario 通过；`OP-003` 暴露 transform-carrier admission/refinement 缺口 |
| `line.three_station_assembly` | `fail` | 885 checks / 0 errors | 编译产物存在；scenario runtime 尚不能执行 `sensor_push_ext == true` 语义 guard |

Corpus `20260724-210000` 与 `20260724-211000` 均为 `harness_status=pass`。第二轮 `repeatability=pass`，differences 为 0；输入 digest 覆盖 config、source、review，运行中输入未变化。交付失败保留为编译器和 runtime 的真实能力边界。

## 工作台覆盖

| 能力 | 结果 | 证据 |
| --- | --- | --- |
| 固定桌面壳 | PASS | title bar、activity bar、Explorer、editor groups、Inspector、Bottom Panel、status bar |
| 三项目 registry | PASS | API 和浏览器均只返回 module、station、line 三个正式项目 |
| 项目会话恢复 | IMPLEMENTED / PARTIAL BROWSER PROOF | store 按 `project_id` 保存 tabs、selected run、split layout、panel、evidence filter；浏览器直接验证 evidence filter 恢复 |
| 项目搜索 | PASS | `project/stage/diagnostic/evidence/commit/status` 的 `field:value` 浏览器断言通过 |
| Problems / Tests | IMPLEMENTED | API 与组件显式建模 partial、freshness、CI unavailable、请求错误、重试和 test scope；浏览器直接验证 grouping 与 artifact deep-link |
| Tests 分类 | PASS | library、integration、canonical example、delivery project 使用显式 `test_scope` |
| Artifact 深链 | PASS | Problems 与 Tests 可打开交付项目 artifact；路由不再限定 `out/` |
| Topology | PASS | 真实 `geometry.json`，152 nodes / 210 edges，节点和边支持键盘操作与文本证据状态 |
| Wiring | PASS | 三项目分别为 15 / 1 / 16 rows；行来自同次运行 `ir_bundle.json/topology.links`，controller/channel/compiler status 与物理观察按 `point_id` 合并 |
| Point check | PASS | observation、photo upload、append-only ledger、projection refresh 通过，release 仍保持 blocked |
| Agent provenance | PASS | 界面同时显示 execution proven、source authoring not_proven、overall not_proven |
| 视口 | PASS | 1440x900、1920x1080 无页面级 overflow 或标签逐字换行 |

## Subagent 完整度分析

| Subagent | 负责范围 | 完整度判断 | 主线程审计与修正 |
| --- | --- | --- | --- |
| Definition agent | 定义 module/station/line 三项目与验收轴 | 最终通过 | v1 在实现后冻结并混合 harness 与 delivery；v2 重新冻结三项目和双状态轴 |
| Corpus execution agent | 物化 specimen、重建 compiler、执行 scenario/project-check/validator/repeatability | 通过 | 补 config/source/review input-set digest、显式 registry root、真实 current-run 执行 |
| Provenance/backend agent | registry、provenance、wiring、physical evidence、signature、release gate、artifact API | 通过 | 修正 Rust borrow 错误、空 event attribution、交付 artifact 深链和 registry 范围 |
| Projection/session agent | 按项目恢复工作台状态，Problems/Tests 投影 | 通过 | 主线程审计 store/API/component；浏览器直接覆盖 evidence filter 恢复、grouping 与 deep-link |
| Search/accessibility agent | command palette、键盘拓扑、双视口浏览器 harness | 通过但过程试错较多 | 修正 React 状态时序、controlled textarea、selector、focus、session 假设和 artifact route |
| PRD completion audit | 识别错误完成声明与残余风险 | 通过 | 阻止把 deterministic fixture materialization 写成无人源码创作证明 |
| Desktop contract audit | 无独立监控页、桌面壳、接线表与三项目浏览器覆盖 | 通过，发现 2 个 P1 | 删除旧 Dashboard；修正 wiring 字段合并；把浏览器覆盖扩展为逐项目 pipeline/holds/wiring/provenance |
| Two-phase evidence audit | 暂存区、旧 run、绝对路径、PowerShell stderr 与报告真实性 | 通过，阻止错误提交 | 识别 `213000` 过强声明、绝对工具路径、clean runner stderr 包装风险和 PRD 验收弱化 |

完整度判断依据是可执行门禁和 same-run artifact。Subagent 的完成声明不直接成为结论；主线程复查编译、API、浏览器行为、fixture 真值和文档表述。

## 异常与修正记录

| ID | 异常 | 根因或试错信号 | 修正与验证 |
| --- | --- | --- | --- |
| `ANOM-001` | `impeccable` 项目内 context 脚本不存在 | 技能安装路径与仓库内假定路径不一致 | 使用技能安装目录脚本；PRODUCT.md 与 product register 成功加载 |
| `ANOM-002` | Subagent 曾跨 sibling 重定向工作 | 分工边界没有在第一次派发时冻结 | 恢复 definition/execution/backend/frontend 文件边界 |
| `ANOM-003` | Definition v1 在实现后冻结 | 验收对象和实现顺序倒置 | v2 先固定三项目、harness/delivery 双状态轴和独立性规则 |
| `ANOM-004` | 旧 validator 依赖 ignored `out/` 残留 | specimen 没有自包含 | 改为项目本地 config/source/review 与 run-specific output |
| `ANOM-005` | 旧 runner 校验静态 JSON 却声明 full-flow | oracle 只检查文件存在 | runner 真正执行 build、materialize、scenario、project-check、validator |
| `ANOM-006` | line 的真实 exit 1 曾被投影为 blocked | harness 成功与 delivery readiness 混淆 | 保留 `delivery_status=fail` 和 gap code |
| `ANOM-007` | station canonical run 有 7 次语法试错 | skill/prompt 未一次给出正确闭环 | retry 和 long-search 信号进入 anomaly；不从历史中删除 |
| `ANOM-008` | PowerShell path、stderr、空参数和插值多次失败 | 脚本未统一 native process 与 future path 契约 | 使用 contained path、局部 ErrorActionPreference、可空 artifact、`${field}` |
| `ANOM-009` | Backend 中间态 Rust borrow 错误 | 新投影结构的借用生命周期错误 | 编译探针定位，随后 web-server 测试全过 |
| `ANOM-010` | 浏览器曾连接旧 binary，projection API 404 | 服务版本与工作树不一致 | 停止已确认 PID，重建并启动最新服务 |
| `ANOM-011` | Windows binary 锁住 Cargo 输出 | 活跃服务占用目标文件 | 确认 PID 后停止并重建 |
| `ANOM-012` | Validator 硬编码 canonical registry | run-specific specimen 无法独立验证 | 增加仓库范围内显式 `RegistryRoot` |
| `ANOM-013` | repeatability digest 漏掉 config | 只散列 source 会遗漏改变 verdict 的输入 | 使用 config/source/review 的 `input_set_sha256` |
| `ANOM-014` | 历史 retry 被当成本轮 event | provenance 没区分 historical/current | 分离 `retry_count` 与 `historical_retry_count` |
| `ANOM-015` | Command palette 对 869 项全量渲染 | 搜索结果没有视图上限 | 仅渲染前 80 项并显示命中总数 |
| `ANOM-016` | Point-check 自测写入 synthetic observation | UI 测试需要可追踪写入，但容易被误读为物理验收 | note 标记非物理验收，release projection 保持 blocked |
| `ANOM-017` | `field:value` 首轮输入变化但结果未过滤 | harness 向 Search Explorer 输入框发送字符，palette 没有获得焦点 | 打开 palette 后显式锁定 dialog input；双视口过滤断言通过 |
| `ANOM-018` | controlled status 在 React state 落定前提交 | harness 紧接选择事件读取旧状态 | 等待受控值稳定再提交 |
| `ANOM-019` | textarea 保留旧受控值 | 直接赋值没有完整触发 React 输入链 | 使用原生 setter 加 input/change 事件并等待状态回显 |
| `ANOM-020` | photo 文件名断言命中 icon `<span>` | selector 范围过宽 | 改为 `.wb-photo-input > span:last-of-type`；完整 harness 复跑通过 |
| `ANOM-021` | 全局 split-layout 假设在项目切换后失败 | layout 已改为 per-project session | harness 按项目断言恢复状态，不再假定全局共享 |
| `ANOM-022` | 交付项目 artifact deep-link 404 | artifact route 只允许 `out/` | 扩展为受控 delivery-project artifact root，并补服务端与浏览器测试 |
| `ANOM-023` | Tests scope 由字符串推断 | 展示逻辑依赖命名约定 | API 增加显式 `test_scope`，UI 按结构分组 |
| `ANOM-024` | Registry 曾扫描 `out/`，项目数超出三项 | 运行产物和正式项目共享发现路径 | registry 只索引正式 delivery-project roots；浏览器确认 `projectCount=3` |
| `ANOM-025` | 并行只读命令被错误单引号封装 | PowerShell 把 executable 与参数解析为相邻字符串 | 改为原生命令字符串；无文件变更，后续检查成功 |
| `ANOM-026` | Fixture materialization 的 proven 曾被解释成源码无人创作 | 执行归因和创作归因共用一个 verdict | 拆分 execution、source authoring、overall 三个 verdict；浏览器显示 `proven/not_proven/not_proven` |
| `ANOM-027` | Run freshness 用 raw SHA 对比 normalized JSON digest | 同一语义内容因格式不同永久 stale | 引入 `normalized_sha256_file`，freshness 按规范化内容计算 |
| `ANOM-028` | Wiring 曾把 config 中的 wiring points 当权威源 | 审计输入和编译器产物边界混淆 | 行从 current-run `ir_bundle.json/topology.links` 派生，config 只做预期审计 |
| `ANOM-029` | Git dirty 状态漏读 `dirty_worktree_at_start` | provenance 字段兼容路径不完整 | 后端同时读取稳定字段，当前运行正确显示 dirty-at-start |
| `ANOM-030` | 前端 API 将 Problems/Tests projection 压成数组 | partial、sources、CI 和请求错误元数据丢失 | 保留完整 projection 结构，Bottom Panel 显式消费元数据 |
| `ANOM-031` | Source root 从错误目录推导 | artifact source resolver 没有以 `source_entry` 为锚点 | 从 `source_entry` 父目录解析并补交付 artifact deep-link 测试 |
| `ANOM-032` | PowerShell 自检包装器再次触发 `$f:` 插值错误 | 既有修正没有上升为临时命令的固定生成规则 | 改为 `${f}:`；三个 runner 均通过 PowerShell parser，并将带冒号变量插值列为提示规则 |
| `ANOM-033` | 准备提交的 canonical runs 基于 dirty worktree 和旧 base commit | 生成证据与实现改动计划在同一提交中落地 | 改为两阶段提交：先提交实现 A，再从干净 A 重新物化并提交证据 B |
| `ANOM-034` | PowerShell 5 把成功命令 stderr 包装成 `NativeCommandError` | 原生 `2>` 重定向经过 PowerShell error stream | 改用 `Start-Process` 独立重定向 stdout/stderr；corpus 213000 扫描无包装和绝对路径堆栈 |
| `ANOM-035` | Delivery artifact GET 在远程配置下仍可匿名读取 | 通用 middleware 只保护 mutation | artifact GET 纳入 Bearer/session 鉴权；新增服务端回归测试 |
| `ANOM-036` | Registry 同时扫描 `delivery-projects` 与兼容 `projects` | 兼容发现路径会破坏固定三项目契约 | registry 仅扫描 canonical `delivery-projects` 根 |
| `ANOM-037` | Materializer 与 validator 只解析 IR 的 topology 子串 | 两端共用局部解析会形成同源盲区 | 两端先完整 `ConvertFrom-Json`，再读取 topology；corpus 213000 三项目通过 |
| `ANOM-038` | 报告曾声明 corpus 213000 stderr 完全干净 | 扫描只覆盖项目命令日志，漏掉 build stderr 中的 `NativeCommandError` | 旧结论撤回；corpus runner 改用 `Start-Process`；dirty diagnostic 230000 全日志扫描无 wrapper |
| `ANOM-039` | Agent event `action` 写入本机绝对编译器路径 | `tool` 已规范化，但 `action` 仍拼接 `$exe` | `action` 改用仓库相对 tool ref；230000 specimen 的 agent-events/provenance 无盘符路径 |
| `ANOM-040` | Clean-checkout runner 仍使用 PowerShell 原生重定向 | clone、checkout、corpus stderr 仍可能被包装成 `NativeCommandError` | clean runner 统一改为 `Start-Process` 独立捕获；最终 Commit A/B clean run 继续验证 |
| `ANOM-041` | 旧 Dashboard/MainLayout 与 `Run Monitor` 文案残留 | 路由已切到工作台，但死代码仍保留传统 Web 产品模型 | 删除旧页面和布局，运行入口命名为 `Run & Gate`，主路由只进入固定桌面壳 |
| `ANOM-042` | Wiring 表把已验证 controller/compiler 显示成 Unknown/Missing | physical projection 覆盖了 `/wiring` 的规范化编译器字段 | 前端按 `point_id` 合并两个投影；输入 safe state 显示 `n/a`；1440 浏览器三项目均无 Unknown/Missing |
| `ANOM-043` | 浏览器报告把 station 深链验证概括为三项目全链覆盖 | 项目循环只检查可选择与标题，后续断言固定在 station | harness 逐项目检查 pipeline、5 holds、wiring row count、normalized fields 和三个 provenance verdict |
| `ANOM-044` | PRD 用条件从句弱化 warning/blocker 深链验收 | 缺失引用的 artifact 被自动排除在验收之外 | 恢复为未完成项；缺失 source/semantic object/artifact 引用继续作为 compiler-evidence gap |
| `ANOM-045` | Commit A clean-checkout 首轮在 cold cargo build 后悬停 | `Start-Process -Wait` 已无 cargo/rustc 子进程，但 PowerShell 未返回，specimen 阶段没有开始 | 终止本轮自有进程；拆除 `-Wait` 并用最小 cargo probe 检查进程对象行为 |
| `ANOM-046` | `Start-Process` 进程对象在 `WaitForExit/Refresh` 后仍返回 null ExitCode | PowerShell 5 宿主无法可靠暴露该对象的退出码 | 三个 runner 改用 `ProcessStartInfo`、并行 `ReadToEndAsync`、真实 `Process.ExitCode`；corpus 231000 通过 |
| `ANOM-047` | Commit A clean-checkout 中 line 通过，module/station 在读取 `virtual_board_meta.json` 时分别报 `DirectoryNotFoundException` | 初步误判为产物目录替换竞争；subagent 复核完整路径长度为 254/261/263，失败点与 Windows 传统 `MAX_PATH=260` 精确吻合。clean runner 把外部 RunId 重复写入 corpus 与项目 run 目录 | 内部运行 ID 缩短为 `cc`；clone 前计算最深产物路径，超过 240 字符返回 `CLEAN_CHECKOUT_PATH_BUDGET_EXCEEDED`；保留 line 的真实 delivery fail，不改写项目逻辑 |

这些记录区分产品缺陷、harness 缺陷、脚本缺陷和环境问题。重复试错集中在浏览器 harness 的 controlled input、焦点与 selector，说明此前的 UI 自动化提示缺少“等待 React 状态稳定、使用窄 selector、按项目恢复 session”的明确规则。

## 验证门禁

| Gate | 结果 | 证据 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | Rust 格式通过 |
| `cargo test -p web-server` | PASS | 65 / 65 tests，包含 artifact GET 鉴权 |
| `npm --prefix web-ui run lint` | PASS | ESLint 无错误 |
| `npm --prefix web-ui run build` | PASS | Production build 通过；Monaco/Ant Design large chunk warning 保留 |
| `node --check scripts/verify_workbench_browser.mjs` | PASS | 浏览器 harness 语法通过 |
| Corpus 210000/211000 | PASS | 两轮三项目 materializer/validator/harness 通过；repeatability differences 0 |
| Corpus 213000 hardening rerun | SUPERSEDED | 3481 checks / 0 errors，但 build stderr 含 PowerShell wrapper，不能作为干净日志证据 |
| Corpus 231000 diagnostic rerun | PASS / DIRTY | 3481 checks / 0 errors；所有日志无 wrapper；specimen event/provenance 无绝对路径；该轮不声明 clean checkout |
| Browser 1440x900 / 1920x1080 | PASS | 三项目逐项 pipeline/holds/wiring/provenance、field filters、session、topology、point check、deep links、无 overflow |
| WIR negative contracts | PASS | `WIR-001` 至 `WIR-007` 均有拒绝路径 |
| Clean-checkout corpus | PENDING FINAL COMMIT | 该门禁只能在最终提交形成后执行；执行后不再修改 tracked files |

## 残余风险

- `source_authoring_verdict=not_proven`：当前证据证明确定性 fixture materialization 和无人执行，没有证明源文件最初由 Agent 在无人干预下创作。
- 完整 WCAG 2.1 AA 审计尚未执行。当前证据覆盖键盘拓扑、focus、状态文本和双视口，不等同于完整 axe/equivalent 审计。
- 电子签名属于内部 engineering attestation，未声明满足特定法规。
- Synthetic point observation 只证明 UI/API 路径，不替代人类接线、仪表测量、HIL 和安全签署。
- 远程部署仍需要组织身份系统、密码哈希、密钥管理和允许来源配置。
- Monaco/Ant Design 大 chunk 仍是加载性能风险。

## 主要工件

- `tasks/prd-autonomous-plc-delivery-workbench.md`
- `scripts/run_delivery_project_corpus.ps1`
- `scripts/run_clean_checkout_delivery_corpus.ps1`
- `scripts/materialize_delivery_project_fixture.ps1`
- `scripts/validate_delivery_project_fixture.ps1`
- `scripts/verify_workbench_browser.mjs`
- `delivery-projects/module.axis-move-blocking-baseline/`
- `delivery-projects/station.dual-slot-shuttle-press-cell/`
- `delivery-projects/line.three-station-assembly/`
- `crates/web-server/src/delivery.rs`
- `web-ui/src/components/workbench/WorkbenchShell.tsx`
- `web-ui/artifacts/workbench-*.png`
