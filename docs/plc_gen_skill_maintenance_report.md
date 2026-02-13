# plc-gen Skill Maintenance Report

范围：`.codex/skills/plc-gen/SKILL.md`（LLM 工作流说明）+ 其可运行自测夹具。

## 发现的主要问题（优化前）

1) 缺少可运行的回归测试
- `SKILL.md` 描述了许多规则（repeat/race/timeout/causality 等），但没有与之绑定的、可 `cargo test` 自动验证的样例集。
- 结果：规则改动/编译器演进后，skill 可能“看起来合理但产物不再可验证”。

2) 关键流程缺少“最小提问清单”
- 阶段一强调“多轮确认”，但没有列出最小必须追问项，容易在信息不足时凭空补齐。

3) I/O 分配在信息缺失时容易“编造”
- 工程落地必须贴合接线表；如果工程师没提供，skill 需要明确占位策略与标注方式。

4) 因果链（causality）规则表述过于绝对
- 原文倾向于“每个 wait 引用传感器都需要 causality 链”，但当前编译器允许从拓扑图推断 action→sensor 可达路径。
- 绝对化表述会导致不必要的对话负担，也可能与仓库内现有示例不一致。

5) 缺少“可验证任务骨架”
- 许多失败并非语法问题，而是 liveness（wait 无 timeout / task 无 on_complete / 没有 fault handler）。
- skill 缺少统一模板，导致生成产物随机性较高。

## 已做的修复（本次）

- 为 plc-gen skill 增加自测机制
  - 新增 fixtures：`.codex/skills/plc-gen/fixtures/valid/*.plc`（10 个覆盖用例）
  - 新增测试：`tests/plc_gen_skill_fixtures.rs`（逐个编译 + semantic + verify_all）

- 改进 `SKILL.md` 的“可执行性”
  - 增加“维护说明（自测）”章节，明确修改 skill 后应同步更新 fixture
  - 在阶段一补充“最小提问清单”，减少凭空假设
  - 增加 I/O 未知时的占位约定，避免编造真实接线
  - 将 causality 的表述调整为“最佳实践 + 解释推断行为”
  - 增加“可验证任务骨架（模板）”，默认引导生成 ready/cycle/fault_handler 三段式结构

## 当前仍未覆盖/建议的下一步

- 负向用例：为常见失败模式补充 `fixtures/invalid/*` 并断言报错信息（如：wait 无 timeout、missing on_complete、parallel 跨设备导致因果误报等）。
- skill 与编译器版本绑定：若未来 DSL 语法演进，建议在 `SKILL.md` 明确最小支持版本/变更点，并把迁移样例加入 fixtures。
- 真实工艺库：将团队历史项目的工艺（脱敏）转成“对话记录 + 最终 .plc + 验证结果”，形成更贴近生产的覆盖面。

