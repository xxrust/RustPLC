`skill-flywheel` 的自测目标不是验证某个业务功能，而是验证它自己作为研究编排 skill 是否闭环。

当前自测约束：

- 盲测执行者默认只能读取本目录下的 `SKILL.md` 与本轮导出的 `public/`
- 仓库内的 `references/`、`agents/`、`scripts/` 仍视为受保护路径，除非通过导出工件间接暴露
- 本轮优先验证“外层 shell runner 是否能按 fresh-process 方式推进至少 5 轮外层迭代”，而不是只做单轮 smoke
- 盲测执行者必须能只靠导出的工件判断 `runner_state.json`、`progress.txt`、`runner_logs/` 和 stop condition 是否与当前任务一致

推荐修复顺序：

1. 先补与 `autonomous-self-improve` 对齐的显式辅助工件和检查清单
2. 再补 shell runner / state / progress 的真实闭环缺口
3. 最后才改 `SKILL.md` 或 agent 模板
