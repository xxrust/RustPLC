# 痛点记录

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


## 结果

单代理 `weak-blind` fallback 结果表明：

- 仅靠真实 `plc-gen` 与导出的 `public/` 工件面，已经可以直接回答复杂项目中的 `public brief` 应包含什么。
- 回答时不再需要翻 `references/` 才能解释 architect / implementer / reviewer 的 one-shot 交接关系。
- `test_plc_gen_target_config.py` 与 cycle 初始化都通过，说明该工件已经进入稳定导出路径。

## 假设观察

本轮观察支持当前假设：此前的主要问题确实是 `public-surface-gap`，而不是还需要继续往 `plc-gen` 本体里堆规则。

## 痛点

1. 步骤：弱盲执行者尝试回答复杂项目多 agent 编排前置条件
   观察到的阻塞：`public brief` 的结构虽然已在 skill / references 中出现，但没有作为专门 public artifact 暴露
   缺少的工件或说明：`complex-project-public-brief.md`
   影响：调用者看不到源码时，执行者仍可能被迫越界去翻 `references/`，或者重新猜 brief 结构

2. 步骤：校验 `plc-gen` 的 flywheel 配置是否能稳定导出新 contract
   观察到的阻塞：原目标测试只覆盖 Day-1 与 lowering / scenario 公开工件，没有把 complex-project public brief 作为回归点
   缺少的工件或说明：target-config 对 `complex-project-public-brief.md` 的断言
   影响：即使本轮补出 public artifact，后续也可能被静默移除，导致 skill 再次退回“规则只存在于正文里”
