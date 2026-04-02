你正在执行一轮研究盲测。

研究程序：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\context\program.md`
运行实例：`run-01`
实例输出 Markdown：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\logs\runs\run-01.md`
实例输出 JSON：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\logs\runs\run-01.json`
使用真实目标 skill：`E:\personal_project\rust_plc\.codex\skills\plc-gen` 来完成这个真实任务：

# PLC Scaffold Day-1

任务目标：

仅基于真实 `plc-gen` skill 和导出的公开工件面，回答下面这个问题：

> 当用户已经给出一份确认版 `plc/main.system.md`，而且这份 contract 的复杂度接近 `wafer_loader.system.md`，应该如何先指导一个不接触 RustPLC 源码的新手完成 Day-1 scaffold、`plc/main.plc` 生成或修复，以及最小验证？

观察点：

- 是否先判断 launcher，而不是直接甩一串 `cargo run` 命令
- 是否把 `.system.md` 当作主输入，而不是重新发散成大问卷
- 是否先给出一轮从 `.system.md` 到 `.plc` 的 lowering 摘要
- 是否能说清 scaffold 后先改哪些文件
- 是否能把未冻结 contract 留在 assumptions / blockers，而不是默默补齐
- 是否能正确处理并发 task、模式矩阵、warning/fault 分流和计数器
- 是否能给出最小验证链，并正确区分 `validated` 与“只是推荐命令”

如果盲测执行者必须阅读 `references/`、`docs/` 或 RustPLC 源码才能回答，就应记为 `public-surface-gap` 或 `skill-gap`，而不是默默越界。


如果这轮存在多个 blind-runner，请只记录你这个实例的观察，不要提前对齐其他实例的结论。

你只允许读取：

- `E:\personal_project\rust_plc\.codex\skills\plc-gen`
- `E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\public`

不要读取目标 skill 之外的仓库文件，包括 README、docs、examples、src、crates 或其他受保护路径。只有显式导出到 `E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\public` 的辅助工件才可读取。

输出要求：

1. 给出你的任务结果。
2. 记录每个阻塞点或低效点。
3. 写清你希望得到的精确缺失项：工件、命令、示例或说明。
4. 明确指出你的观察是支持、削弱，还是无法判断当前研究假设。

把实例观察保存到：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\logs\runs\run-01.md`。
如果需要机器可读版本，同时写入：`E:\personal_project\rust_plc\.codex\skills\plc-gen\.skill_flywheel\cycles\cycle-20260326-plcgen-round4-system-lowering\logs\runs\run-01.json`。
