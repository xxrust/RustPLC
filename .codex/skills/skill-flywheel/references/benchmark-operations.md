# Benchmark 交接协议

## 目标

这份文档只回答一个问题：

当 benchmark 引入 `benchmark-proposer`、`benchmark-curator`、`runner-judge`、`flywheel-optimizer` 四类角色时，它们之间通过哪些文件交接，而不是靠会话记忆。

## 最小通信面

推荐只使用下面这些稳定工件：

1. suite 级

- `manifest.json`
- `governance/curator-notes.md`
- `governance/proposals.jsonl`
- `summaries/latest-summary.json`
- `summaries/latest-summary.md`

2. case 级

- `case.json`
- `public/prompt.md`
- `public/inputs/*`
- `hidden/rubric.json`
- `hidden/oracle.json`
- `hidden/notes.md`
- `evaluation/result.json`

## 谁写什么

### `benchmark-proposer`

只负责：

- 整理候选任务
- 生成 case 草稿
- 把候选理由写入 `governance/proposals.jsonl`

不负责：

- freeze case
- 修改 hidden oracle 以适配当前 skill
- 宣布通过或阶段验收

### `benchmark-curator`

只负责：

- 审核候选 case
- 把 case 状态从 `draft` 变成 `frozen`
- 必要时 retire case
- 在 `governance/curator-notes.md` 记录理由

### `runner-judge`

只负责：

- 读取 `case.json`
- 读取 `hidden/rubric.json` 与 `hidden/oracle.json`
- 执行被测 skill
- 把结果写入 `evaluation/result.json`

### `flywheel-optimizer`

只负责：

- 读取 `summaries/latest-summary.*`
- 必要时读 `evaluation/result.json` 的摘要字段
- 根据失败模式修改 skill / public surface / code

不应读取：

- 完整 hidden oracle
- 当前冻结逻辑的内部理由

## `evaluation/result.json` 约定

推荐至少包含：

- `case_id`
- `split`
- `run_label`
- `skill_revision`
- `status`
- `verdict`
- `summary`
- `blocker_classification`
- `metrics`
- `evidence_paths`
- `evaluated_at_utc`

其中：

- `status` 表示运行是否完成，例如 `not_run` / `completed` / `error`
- `verdict` 表示评测结论，例如 `pass` / `fail` / `blocked`

## `summaries/latest-summary.json` 约定

推荐至少包含：

- suite 名称
- 生成时间
- 总 case 数
- pass / fail / blocked / error / not_run 统计
- split 级统计
- top blockers
- stable failure patterns

这个文件是 flywheel 默认应读取的主入口，而不是逐 case 扫描全部 hidden 工件。

## 单轮推荐顺序

1. proposer 生成候选 case，并把意图记入 `proposals.jsonl`
2. curator 冻结一批 `draft -> frozen`
3. judge 逐 case 写 `evaluation/result.json`
4. aggregate 脚本更新 `summaries/latest-summary.*`
5. flywheel 只根据 summary 和失败摘要决定下一轮最小修复

## 反模式

不要这样做：

- optimizer 一边改 skill，一边改 frozen case
- judge 把 hidden oracle 原文回填到 flywheel 的输入里
- curator 不写理由，只靠聊天记忆决定 freeze / retire
- flywheel 直接把单个 case 的偶发失败升级成全局结论
