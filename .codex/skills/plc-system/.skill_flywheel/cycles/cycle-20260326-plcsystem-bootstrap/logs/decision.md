# 本轮决策

## 研究问题

把 `plc-system` 的本地 flywheel 配置升级到当前协议后，是否能只靠 task-specific 导出工件稳定约束 System Day-1 的建议稿、问题数量、并发表达和 handoff 边界？

## 假设状态

partially-supported

## 关键证据

- 已为 `plc-system` 新增本地 `.skill_flywheel/` 配置，包括 `program.md`、`profile.md`、`public_surface.json`、`experiments.jsonl`、任务模板和 Day-1 public 工件。
- 新增 `test_plc_system_target_config.py` 后，`python -m unittest discover -s .codex/skills/skill-flywheel/scripts -p "test_*.py" -v` 20/20 通过。
- 真实运行 `python .codex/skills/skill-flywheel/scripts/init_public_surface.py --target-skill-path ...plc-system --task-file system-day1.md` 成功创建了 `cycle-20260326-plcsystem-bootstrap`，证明当前脚本可以稳定消费这套局部配置。
- 当前证据仍是 weak-blind bootstrap：证明了协议兼容和 public surface 成形，但还没有在具体模糊工艺需求上做 clean-room 回答一致性验证。

## 本轮最小动作

- 把 `plc-system/.skill_flywheel/` 升级到当前 `skill-flywheel` 协议。
- 把 System Day-1 的 workflow、required sections、concurrency、handoff 和 checklist 收敛成 task-specific public 工件。
- 补一条自动测试，锁定 `plc-system` 目标配置不会退回旧 schema 或漏导出关键工件。

## 结论分类

bootstrap-validated

## 决策摘要

`plc-system` 已具备可运行的 flywheel 本地配置和最小 Day-1 public surface；bootstrap 与导出链路已经验证通过，但真实模糊需求上的回答一致性还需要下一轮验证。

## 是否进入下一轮

是

## 下一轮研究问题

在一个真实但模糊的工业控制需求上，当前导出的 public surface 是否能稳定约束回答为：先给 `.system.md` 建议稿、最多 3 个阻塞问题、正确的并发 / blocking 语义，以及只有在 handoff gate 满足后才允许交给 `plc-gen`？
