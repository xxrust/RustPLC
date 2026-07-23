# Scaffold Day-1 Checklist

回答一个“请基于 `.system.md` 生成 RustPLC 项目或 delivery asset source set”的请求时，默认至少检查：

## 输入检查

- 是否已经有确认版 `.system.md`
- 这是新项目、现有 bundle 修复，还是现有 `.plc` 修复
- 用户使用的是 installed binary 还是 source workspace
- 这是小范围 repair，还是 complex delivery
- 是否存在仍会改变 `.plc` 结构的未决 contract
- complex delivery 是否默认需要 intent sidecar
- 多工位整线描述是否应优先冻结为 `line` 而不是 `station`
- “单机运行/装配岛”这类部署词是否被错误拿来覆盖多工位整线语义

## 生成检查

- complex delivery 是否已切到 `.bundle.toml` + fragments，而不是默认停在单文件 `plc/main.plc`
- scaffold 后是否已替换 root `plc/main.system.md` 与 delivery asset `docs/*.system.md` 的占位内容
- authoritative source entry 是否已经明确到 delivery asset `main.bundle.toml`
- delivery asset docs / bundle / sidecar 是否仍包含 `Default Starter Flow`、`starter`、`replace_me_after_authoring`、`replace_after_intent_doctor`
- delivery layer 是否与 authoritative source 的粒度一致，例如整线描述没有误降成 `station`
- 没有把并发语义误写成单执行指针
- 如果 `.system.md` 已给出 task 名称，是否保留这些 task 名
- 模式矩阵是否被降成独立 service/supervisor 结构，而不是混进自动主循环
- warning 路径与 fault 路径是否被错误合并
- 计数器 / streak / retry / rate 是否提升为 `[topology] variable`
- 共享资源是否用 `semantic_resource` / `claim`，而不是只写口头顺序
- blocking 行为显式落到 `wait/delay/timeout/axis.move_*`
- 如果用户要求当前 scenario 工具链可直接跑，是否存在可能触发 `unsupported guard expression` 的复合 `wait` guard
- 如果业务语义允许，复合 readiness 是否已优先改写为顺序单条件 `wait`
- manual wait 是否显式 `allow_indefinite_wait: true`
- 非 manual wait 是否给出 timeout 逃生路径
- axis move 是否带 `timeout`、`on_reject`、`on_motion_fault`、`on_safety_fault`
- complex delivery 的 `*.intent_alignment.contract.json` 是否仍然是 scaffold placeholder
- 是否真的运行了 `project-check`

## 输出检查

最终回答至少应包含：

- 结果摘要
- 生成或修复后的 bundle / fragments / 项目交付状态
- assumptions
- 真实 launcher / 命令
- validation 状态
- authored artifacts 与 toolchain artifacts 的分离
- 如果被 blocker 卡住，是否明确指出是 placeholder contract、未冻结 contract，还是工具链限制
- 是否明确说明 root scaffold 文件只是 bridge，而不是唯一交付面

## 遇到陌生用户时的默认顺序

先回答：

1. 该用哪套 launcher
2. complex delivery 该选哪种 source shape
3. scaffold 后先改哪些 delivery asset 文件
4. 这份 `.system.md` 是否足够直接生成 bundle / fragments
5. 最小验证链是什么

不要让用户自己从顶层 `--help` 猜命令。
