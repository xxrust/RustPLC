# 根因分析

任务：
# PLC Complex Project Public Brief

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `.system.md`，而且任务同时涉及多文件 bundle 倾向、scenario/gate、并可能要求 intent-alignment 时，主 agent 必须先准备什么 `public brief`，才能把任务 one-shot 地拆给 architect、多个 implementer 和 reviewer？

观察点：

- 是否先说清 `public brief`，而不是直接开始角色分工
- 是否能列出 brief 的最低字段
- 是否能区分 authored artifacts 与 toolchain artifacts
- 是否能说明 architect / implementer / reviewer 各自基于 brief 交付什么
- 如果 brief 不足，是否会要求补 brief，而不是默默越界读源码

如果盲测执行者必须阅读 `references/` 或仓库普通文件才能回答，就应记为 `public-surface-gap`，而不是默默越界。


## 假设判断

支持。

## 结论

1. 痛点：复杂项目的 `public brief` 规则没有成为显式 public contract
   分类：`public-surface-gap`
   原因：`plc-gen` 已经引入 one-shot 与多 agent 编排，但“主 agent 先准备 public brief”这条规则仍主要停留在正文与 reference 层，没有公开到 `.skill_flywheel/public/`
   最小修复：新增 `public/complex-project-public-brief.md`，并把它加入 `public_surface.json` 与 profile 中的默认公开工件面

2. 痛点：新 contract 缺少自动化回归守卫
   分类：`code-gap`
   原因：`skill-flywheel` 的目标测试此前没有把 complex-project public brief 视为稳定公开面的一部分
   最小修复：在 `test_plc_gen_target_config.py` 中增加 `artifact_paths` 与 cycle `public/` 导出的双重断言
