# Scaffold Day-1 Validation Order

对一个“已确认 `.system.md` -> 新项目交付”的 Day-1 路径，顺序固定为：

1. 判断 launcher
2. scaffold 项目
3. 先确认或落盘 `plc/main.system.md`
4. 生成或修复 `plc/main.plc`
5. 如有需要，调整 `scenarios/nominal/normal.yaml`
6. 运行 `scenario-validate`
7. 运行 `scenario-doctor`
8. 项目级请求再运行 `no-board-gate`
9. 需要 ST 时再运行 `gen-st`

## 最低验证门槛

至少给出：

- `scenario-validate`
- `scenario-doctor`

没有真实工具运行结果时，不要把状态写成 `validated`。

推荐状态词只有这四个：

- `validated`
- `validated with warnings`
- `blocked by missing contract`
- `failed validation`

## 文件关注顺序

对陌生用户先强调这些文件：

1. `plc/main.system.md`
2. `plc/main.plc`
3. `scenarios/nominal/normal.yaml`
4. `rustplc.project.toml`
5. `config/io_map.toml`

不要一上来把注意力带到 `out/` 或板级目录。
