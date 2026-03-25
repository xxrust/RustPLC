# plc-gen Project Layout

本文件告诉 skill：对一个新项目，先让用户看哪些文件，后看哪些文件。

## scaffold 后的关键文件

必须优先让用户关注这些文件：

- `plc/main.system.md`
  已确认的 system contract，决定 task、blocking、fault 与资源边界。
- `plc/main.plc`
  真正要生成或修复的 RustPLC DSL。
- `scenarios/nominal/normal.yaml`
  scaffold 已创建好的 nominal scenario。
- `rustplc.project.toml`
  项目 manifest 与默认路径约定。
- `config/io_map.toml`
  板级 I/O 映射。
- `config/retain.toml`
  retain / persistence 基线。

## 可告诉用户存在，但不要优先让他手改的文件

- `out/ir/`
- `out/sim/`
- `out/gate/`
- `out/codegen/`
- `out/rp2040/`
- `out/release/`

这些都是生成产物目录，不是第一批手改目标。

## 正确的编辑顺序

对新项目，优先顺序固定为：

1. `plc/main.system.md`
2. `plc/main.plc`
3. `scenarios/nominal/normal.yaml`
4. 验证命令
5. codegen / build / release

不要一上来就把注意力带到 `out/` 或板级构建。

## VS Code 与附带文件

scaffold 还会生成 `.vscode/*`、`.github/workflows/*`、`docs/project-layout.md` 等辅助文件。
这些是方便用户启动，不是主语义源。
如果用户问“先改哪里”，不要先回答这些辅助文件。

## 运行方式提醒

这个 scaffold 不是 Cargo 项目。

因此：

- 如果用户用已安装的 `rust_plc`，可以进入 scaffold 目录继续工作
- 如果用户用 `cargo run --release --bin rust_plc -- ...`，就必须留在 RustPLC 源码仓根目录，对 scaffold 使用路径参数
