# plc-gen Project Layout

当调用方询问 scaffold 后应该先修改哪些文件时，使用本文件。

## Scaffold Command

```bash
<run> new my_plc_project
```

## 调用方首先应关注的文件

- `plc/main.system.md`
  已确认的 system contract
- `plc/main.plc`
  可执行 RustPLC DSL
- `scenarios/nominal/normal.yaml`
  scaffold 已创建好的 nominal validation scenario
- `config/io_map.toml`
  deployment I/O mapping
- `config/retain.toml`
  retain baseline
- `rustplc.project.toml`
  manifest 与默认路径 contract

## Edit Order

对于全新项目：

1. 先确认 `plc/main.system.md`
2. 再编写或修复 `plc/main.plc`
3. 然后更新 `scenarios/nominal/normal.yaml`
4. 接着运行验证命令
5. 最后才生成 codegen 或 deployment artifact

## Output Folders

以下目录都视为可重建的生成产物：

- `out/ir/`
- `out/sim/`
- `out/gate/`
- `out/codegen/`
- `out/rp2040/`
- `out/release/`
