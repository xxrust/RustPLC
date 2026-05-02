# Developer Bootstrap Pack

从零到第一次验证通过，5 分钟。

---

## 创建项目

```bash
cargo run --release -- new my_plc_project
cd my_plc_project
```

生成的目录结构：

```
my_plc_project/
├── rustplc.project.toml          # 项目配置
├── plc/
│   ├── main.system.md            # 需求描述（AI 读取此文件生成 .plc）
│   └── main.plc                  # 控制程序
├── scenarios/
│   ├── nominal/
│   │   └── normal.yaml           # 正常场景
│   └── faults/                   # 故障场景
├── config/
│   ├── io_map.toml               # I/O 映射
│   └── retain.toml               # 保持变量
├── docs/
│   └── project-layout.md         # 项目结构说明
├── .github/workflows/
│   └── no_board_gate.yml         # CI 门禁
├── .vscode/
│   ├── tasks.json                # VS Code 任务
│   ├── settings.json             # 编辑器设置
│   ├── extensions.json           # 推荐扩展
│   └── plc.code-snippets         # 代码片段
└── .gitignore
```

---

## 第一次验证

```bash
# 编译并验证
cargo run --release -- plc/main.plc --no-print-ir

# 校验场景
cargo run --release -- scenario-validate plc/main.plc \
  --scenario scenarios/nominal/normal.yaml --output human

# 无板门禁
cargo run --release -- no-board-gate plc/main.plc \
  --scenario scenarios/nominal/normal.yaml \
  --out-dir out/gate/no_board/normal --output human
```

---

## VS Code 开箱即用

项目脚手架自带 VS Code 配置：

**语法高亮**：`*.plc` 文件映射到 `ini` 语法（无需安装额外扩展）

**任务快捷键**（`Ctrl+Shift+B`）：
- `RustPLC: scenario-init (normal)` — 生成场景骨架
- `RustPLC: scenario-validate` — 校验场景
- `RustPLC: sim-plc` — SIL 仿真
- `RustPLC: no-board-gate` — 无板门禁
- `RustPLC: gen-st` — 生成 ST 代码
- `RustPLC: build-rp2040` — 构建 RP2040 固件

**代码片段**：
- `plc-skeleton` — 完整 .plc 骨架
- `plc-wait-timeout` — wait + timeout 模式

---

## AI 辅助生成

如果你使用 Claude Code 或其他支持 MCP 的 AI 工具：

1. 编辑 `plc/main.system.md`，用自然语言描述你的控制需求
2. AI 读取 system.md，通过 MCP 调用 `get_rustplc_skill_guide` 获取 DSL 指南
3. AI 生成 .plc 文件
4. AI 调用 `validate_plc` 验证，根据错误自动修复
5. 验证通过后，你审查并批准

---

## 下一步

| 想做什么 | 看哪里 |
|---------|--------|
| 理解 DSL 语法 | README 的 "DSL 一览" 章节 |
| 看复杂示例 | `examples/dual_axis_platform.plc` |
| 部署到硬件 | [RP2040 运动控制](RP2040-Motion-Minimal-Example.md) |
| 理解验证引擎 | [AGENTS.md](../../AGENTS.md) 验证引擎章节 |
| 场景工程 | [场景资产化](Scenario-Assetization-Coverage-Feedback.md) |
| 设备库扩展 | [设备库](Device-Library.md) |

---

## 边界说明

- 项目的正式需求入口是 `plc/main.system.md`
- 公开示例中的系统契约应优先使用 `examples/project_scaffold_demo/plc/main.system.md` 这类项目内入口；不要再在 `examples/` 根目录堆放游离的 `*.system.md`
- 项目级布局契约见 `docs/已实现/generated_project_layout_spec.md`
