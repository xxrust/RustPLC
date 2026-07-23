# Scaffold Day-1 Validation Order

对一个“已确认 `.system.md` -> 新项目交付”的 Day-1 路径，顺序固定为：

1. 判断 launcher
2. 判断 source shape
3. 对 complex delivery 先 scaffold structured-fragments 项目
4. 先确认或落盘 root `plc/main.system.md`
5. 立刻替换 delivery asset 下的 scaffold 占位 docs，例如 `plc/deliveries/station/<slug>/docs/station.system.md`
6. 先写 confirmed-system lowering 摘要，再生成或修复 delivery asset `main.bundle.toml` 与 fragments
7. 修复 `scenarios/nominal/normal.yaml`
8. 先跑一次真实 trace，确认 scenario 不只驱动启动按钮，而且确实驱动了本周期依赖的普通 field inputs / sensors
9. complex delivery 默认同时处理 `*.intent_alignment.contract.json`，如果仍是占位状态则明确报 blocker
10. 在 delivery asset source entry 上运行 `project-check`
11. 需要 ST 时再运行 `gen-st`

## 最低验证门槛

至少给出：

- 对 project-scale 请求：`project-check`
- 对未走完整项目链的局部 repair：`scenario-validate` 与 `scenario-doctor`

没有真实工具运行结果时，不要把状态写成 `validated`。
如果 `*.intent_alignment.contract.json` 仍含 `replace_me_after_authoring` 或 `replace_after_intent_doctor`，也不要写成 `validated`。
如果 trace 还没覆盖到 cycle start / cycle complete 边界，也不要提前冻结 sidecar。

推荐状态词只有这四个：

- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `blocked by toolchain limitation`
- `failed validation`

## 文件关注顺序

对陌生用户先强调这些文件：

1. delivery asset `docs/*.system.md`
2. delivery asset `plc/main.bundle.toml`
3. delivery asset `scenarios/nominal/normal.yaml`
4. delivery asset `docs/*.intent_alignment.contract.json`
5. root `plc/main.system.md`

不要一上来把注意力带到 `out/` 或板级目录，也不要停在 scaffold 默认占位文案。
