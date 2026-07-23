# RustPLC Autonomous PLC Delivery Workbench Self-test

日期：2026-07-24

结论：`harness_status=pass`，真实交付状态为 `0 pass / 1 blocked / 2 fail`。桌面工作台核心交互通过；command palette 的增强 `field:value` 浏览器断言仍未闭合。

## 结论边界

交付项目是编译器进展接口。工作台把每个项目的源码、编译阶段、验证报告、运行记录、接线证据和人工 hold 放在同一个 Cursor/VS Code 式工作区中。系统没有独立的 compiler monitor 页面。

工作台状态与真实项目状态分开计算：

- `harness_status=pass` 表示本轮脚本构造了独立 specimen，重新构建并执行了编译、四类验证、scenario、`project-check` 和 validator。
- `delivery_status=blocked` 表示项目逻辑通过当前门禁，但物理接线、HIL 或人工责任仍缺证据。
- `delivery_status=fail` 表示声明的 scenario 或 project-check 真实 exit 1。产品缺口保持为 fail，不能改写成 blocked。

## 执行链

```text
canonical delivery-projects/
        |
        v
copy config + source + review -> run-specific specimens/
        |
        v
build current rust_plc -> materialize project artifacts
        |
        +--> compile / Parser / AST / Semantic / IR
        +--> Safety / Liveness / Timing / Causality
        +--> scenario-validate -> project-check
        +--> wiring / point checks / human holds
        |
        v
validator -> corpus result -> repeatability comparison
        |
        v
Workbench Explorer / Editor / Problems / Tests / Inspector
```

## 三项目结果

| 项目 | 当前交付状态 | 本轮证据 | 直接 blocker |
|---|---|---|---|
| `module.axis_move_blocking_baseline` | `blocked` | materializer、validator、compile、scenario、project-check 均通过；307 checks | HIL、硬件 timing、接线点检和人工 hold 尚未完成 |
| `station.dual_slot_shuttle_press_cell` | `fail` | 11 个 current-run events，553 checks，9/9 scenario 通过 | `process_model_check` exit 1，`OP-003`，`press_process.mark_carrier_pressed.op62` 的 transform-carrier admission/refinement 尚未完整建模 |
| `line.three_station_assembly` | `fail` | 4 个 current-run events，345 checks | normal/timeout scenario 与 project-check exit 1；`sensor_push_ext == true` 属于 scenario runtime 尚未支持的 semantic guard |

最终 corpus：

- `20260724-156000`：exit 0，`harness_status=pass`。
- `20260724-157000`：exit 0，`repeatability=pass`，differences 0。
- 3/3 materializer、3/3 validator、3/3 project harness 通过。
- `1205 checks / 0 errors / 19 current-run agent events`。
- `15` 条机器异常，`14` 条已验证修正。
- 每个项目的 `input_set_sha256` 覆盖 `delivery-project.config.json`、`source/` 和 `review/`；`source_set_sha256` 单独保留。
- `fresh_checkout=not_proven`。本轮证明 repository-local specimen ownership 和 repeatability，没有声称执行过 clean clone。

证据：`out/delivery-project-corpus/20260724-157000/corpus-result.json`。

## Subagent 完整度

| Subagent | 完成内容 | 完整度判断 | 主线程分析与修正 |
|---|---|---|---|
| Definition agent | 产出 v2 三项目定义，固定 module/station/line、状态轴和验收边界 | 通过 | v1 曾在实现后才冻结，且把 harness pass 与 delivery readiness 混合；主线程冻结 v2 并恢复 `harness_status`/`delivery_status` 分离 |
| Corpus execution agent | 每轮复制三个 specimen，重建 compiler，执行 materializer、validator、repeatability | 通过 | 自审发现 source-only digest 漏掉 config；补齐 input-set digest，并修正 registry hard-code、PowerShell 参数和 exit 1 投影 |
| Backend agent | delivery registry、auth、physical evidence、signature、release gate、deep-link API | 通过 | 中间态出现 Rust borrow 编译错误，主线程用 `cargo check` 发现并修复；最终 `59 passed` |
| Frontend agent | IDE shell、Explorer、editor groups、Inspector、Bottom Panel、point observation、artifact viewer | 基本通过 | 初始 API projection、correction ledger、签名和 stale backend 都有缺口；主线程补齐 typed projection 并重建服务 |
| 主线程最终审计 | 统一门禁、真实服务、浏览器截图、报告和 PRD | 未完全闭合 | command palette `field:value` 在 Puppeteer 中显示 DOM 输入变化但结果仍为未过滤的 241 项；已记录为残余风险，没有宣称增强断言通过 |

## 异常与修正记录

### 过程异常

| ID | 观察到的异常 | 修正 | 结果 |
|---|---|---|---|
| `ANOM-001` | `impeccable` 项目内脚本路径不存在 | 改用全局安装路径 `C:\Users\liangyuhang\.agents\skills\impeccable\scripts\context.mjs` | setup 完成，路径异常保留记录 |
| `ANOM-002` | corpus subagent 两次越权重定向 backend/frontend sibling | 停止交叉修改，恢复 definition/execution/backend/frontend 边界 | 后续修改按文件范围执行 |
| `ANOM-003` | Definition v1 在实现后才冻结，候选项目与 canonical 项目不一致 | 冻结 v2 三项目定义并补独立状态轴 | 验收对象稳定 |
| `ANOM-004` | 旧 validator 依赖 ignored `out/complex_selftest` 残留 | 改为项目本地 config/source/review + run-specific output | fresh data 依赖消除 |
| `ANOM-005` | 旧 runner 只验证静态 JSON，却声称 full-flow repeatability | runner 重新构建并调用 materializer、scenario、project-check、validator | 156/157 两轮通过 |
| `ANOM-006` | line 项目真实 exit 1 被物化为 blocked/0 fail | 保留 scenario/project-check fail 与 gap code | 交付结论保持 fail |
| `ANOM-007` | canonical Agent run 出现 7 次公开语法编译试错 | 记录 retry_count 和 long-search signal | 这是 skill/prompt 缺口，不被隐藏 |
| `ANOM-008` | PowerShell `Resolve-Path`、native stderr、空 artifact 参数和 `${field}` 插值多次失败 | 使用 workspace-contained path、局部 ErrorActionPreference、稳定参数契约和显式插值 | parser、JSON、runner 通过 |
| `ANOM-009` | backend 中间态 Rust borrow 错误 | 主线程编译探针定位并修复 | web-server tests 通过 |
| `ANOM-010` | 浏览器最初连接旧二进制，projection API 返回 404 | 停止旧 PID，重新 build 并启动服务 | live API 通过 |
| `ANOM-011` | Windows 旧 `rustplc-web.exe` 锁住 Cargo 输出文件 | 停止已确认 PID `67940` 后重建 | build 通过 |
| `ANOM-012` | validator 硬编码 canonical registry，拒绝 run-specific specimen | 接受仓库内显式 `RegistryRoot` | 3/3 validator 通过 |
| `ANOM-013` | input digest 初始只覆盖 `source/`，漏掉会改变 verdict 的 config | 增加 config/source/review 的 `input_set_sha256`，保留 source digest | 157 repeatability 差异 0 |
| `ANOM-014` | 临时兼容性命令再次触发 `$project:` drive-qualified interpolation | 改用 `${project}` | 记录为同类错误复发，最终解析通过 |
| `ANOM-015` | command palette 全量过滤在 869 条索引上触发高渲染负载 | 全量搜索只渲染前 80 条，增加命中总数提示 | lint/build 通过 |
| `ANOM-016` | point-check 浏览器自测写入 synthetic blocked observations/uploads | 每条 note 明确标注“not physical acceptance”，release projection 仍 blocked | 没有制造物理通过证据 |
| `ANOM-017` | `field:value` harness 中 DOM input 值更新，但结果仍是 241 个未过滤 option | 尝试等待 palette 卸载、逐键输入、`onInput`、限制可见结果、禁用 Chrome cache、唯一 URL、重启 8080 | 仍未闭合，保留为产品/harness 残余风险 |
| `ANOM-018` | 首次提交命令的 PowerShell 引号转义把 commit subject 写入反斜杠和尾引号 | 在推送前检查 `git log -1 --format=fuller`，使用单引号参数执行 `git commit --amend` | 提交内容未变化，标题已修正后再推送 |

## 代码与验证门禁

| Gate | 结果 | 证据 |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | 当前工作树 Rust 格式通过 |
| `cargo test -p web-server` | PASS | 59 passed，包含 auth、角色、evidence stale、trace containment、release gate |
| `npm run lint` | PASS | ESLint 无错误 |
| `npm run build` | PASS | Vite production build 通过；Monaco/Ant Design chunk >500 kB 为性能 warning |
| Corpus 156/157 | PASS | 3/3 materializer、validator、harness pass；repeatability pass |
| Browser core interaction | PASS | 1440x900、1920x1080；project coverage、split/tab move、keyboard/pointer resize、Problems/Tests grouping、signature dialog、artifact deep-link、无 viewport overflow |
| Browser physical evidence | PASS | point observation dialog、photo upload、append-only observation、projection refresh 连续成功；状态保持 blocked |
| Browser palette `field:value` enhancement | NOT CLOSED | DOM 输入值正确，过滤结果未收敛；未将该项写成 PASS |

## 真实边界

- `unattended_verdict` 仍为 `not_proven`。当前记录缺少足以证明无人类源码编辑的文件级前后 digest 归因链，也没有持久模型标识。
- clean clone 未执行，`fresh_checkout=not_proven`。
- synthetic point observation 是 UI/API 自测证据，不能替代人类接线、仪表测量、HIL 或安全签署。
- electronic signature 是内部 engineering attestation，不声明满足特定法规签名标准。
- 远程或生产环境仍需要组织身份系统、密码哈希和密钥管理。
- Monaco/Ant Design chunk 较大，属于后续加载性能优化项。
- command palette 的 `field:value` 过滤需要在真实 Chrome 执行路径中继续定位。当前已证明普通打开、执行、搜索框输入、核心项目/证据导航流程可用；增强过滤项保持未完成。

## 主要工件

- `tasks/prd-autonomous-plc-delivery-workbench.md`
- `scripts/run_delivery_project_corpus.ps1`
- `scripts/materialize_delivery_project_fixture.ps1`
- `scripts/validate_delivery_project_fixture.ps1`
- `scripts/verify_workbench_browser.mjs`
- `delivery-projects/module.axis-move-blocking-baseline/`
- `delivery-projects/station.dual-slot-shuttle-press-cell/`
- `delivery-projects/line.three-station-assembly/`
- `crates/web-server/src/delivery.rs`
- `crates/web-server/src/physical_evidence.rs`
- `crates/web-server/src/signatures.rs`
- `web-ui/src/components/workbench/WorkbenchShell.tsx`
- `web-ui/src/components/workbench/CommandPalette.tsx`
- `web-ui/artifacts/workbench-*.png`
