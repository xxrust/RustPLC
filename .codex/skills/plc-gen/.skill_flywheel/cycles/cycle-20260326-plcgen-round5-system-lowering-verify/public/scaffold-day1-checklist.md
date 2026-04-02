# Scaffold Day-1 Checklist

回答一个“请基于 `.system.md` 生成 RustPLC 项目或 `plc/main.plc`”的请求时，默认至少检查：

## 输入检查

- 是否已经有确认版 `.system.md`
- 这是新项目还是现有 `.plc` 修复
- 用户使用的是 installed binary 还是 source workspace
- 是否存在仍会改变 `.plc` 结构的未决 contract

## 生成检查

- 没有把并发语义误写成单执行指针
- 如果 `.system.md` 已给出 task 名称，是否保留这些 task 名
- 模式矩阵是否被降成独立 service/supervisor 结构，而不是混进自动主循环
- warning 路径与 fault 路径是否被错误合并
- 计数器 / streak / retry / rate 是否提升为 `[topology] variable`
- 共享资源是否用 `semantic_resource` / `claim`，而不是只写口头顺序
- blocking 行为显式落到 `wait/delay/timeout/axis.move_*`
- manual wait 是否显式 `allow_indefinite_wait: true`
- 非 manual wait 是否给出 timeout 逃生路径
- axis move 是否带 `timeout`、`on_reject`、`on_motion_fault`、`on_safety_fault`

## 输出检查

最终回答至少应包含：

- 结果摘要
- 生成或修复后的 `plc/main.plc` 或项目交付状态
- assumptions
- 真实 launcher / 命令
- validation 状态

## 遇到陌生用户时的默认顺序

先回答：

1. 该用哪套 launcher
2. scaffold 后先改哪些文件
3. 这份 `.system.md` 是否足够直接生成 `.plc`
4. 最小验证链是什么

不要让用户自己从顶层 `--help` 猜命令。
