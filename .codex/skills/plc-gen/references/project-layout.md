# plc-gen Project Layout

本文告诉 skill：对一个 RustPLC 项目，先让用户看哪些文件，后看哪些文件。

## scaffold 默认布局

对 scaffold 项目，优先让用户关注这些文件：

- `plc/main.system.md`
  已确认的 system contract，决定 task、blocking、fault 与资源边界
- `plc/main.plc`
  scaffold 默认的 DSL source entry
- `scenarios/nominal/normal.yaml`
  scaffold 已创建好的 nominal scenario
- `rustplc.project.toml`
  项目 manifest 与默认入口约定
- `config/io_map.toml`
  板级 I/O 映射
- `config/retain.toml`
  retain / persistence 基线

## DSL source set 视角

RustPLC DSL 交付关注的是 source set，source entry 由项目布局决定。

常见 source set 形态有两种：

### 单文件 source set

- 一个 `.plc` 文件承载 `[topology]`、`[constraints]`、`[tasks]`
- 在 scaffold 默认布局里，这个入口通常是 `plc/main.plc`

### 多文件 source set

- 一个 `.bundle.toml` 作为 DSL source entry
- `topology`、`constraints`、`tasks` 分别落在不同 fragments
- 编译、验证和 scenario 工具链统一从 bundle entry 进入

## 多文件布局关注点

如果项目采用 `.bundle.toml` + fragments，优先让用户关注：

- `<name>.bundle.toml`
  bundle source entry
- `fragments/topology.plcfrag` 或等价 topology fragment
- `fragments/constraints.plcfrag` 或等价 constraints fragment
- `fragments/tasks.plcfrag` 或等价 tasks fragment
- 配套 scenario 与项目级验证命令

## 可告知用户存在，但不应优先手改的目录

- `out/ir/`
- `out/sim/`
- `out/gate/`
- `out/project_check/`
- `out/codegen/`
- `out/rp2040/`
- `out/release/`

这些都是生成产物目录，不是第一批手改目标。

## 正确的编辑顺序

对 scaffold 默认布局，优先顺序固定为：

1. `plc/main.system.md`
2. DSL source entry
3. `scenarios/nominal/normal.yaml`
4. 验证命令
5. codegen / build / release

对 bundle 布局，优先顺序固定为：

1. system contract 或等价需求源
2. `.bundle.toml`
3. `topology` / `constraints` / `tasks` fragments
4. scenario
5. 验证命令
6. codegen / build / release

## VS Code 与附带文件

scaffold 还会生成这些辅助文件：
- `.vscode/*`
- `.github/workflows/no_board_gate.yml`
- `docs/project-layout.md`
- `README.md`

这些文件用于 Day-1 启动与项目导航，不是主语义源。如果用户问“先改哪里”，先回答 system contract、DSL source entry 与 scenario。

## 运行方式提醒

这个 scaffold 不是 Cargo 项目。

因此：
- 如果用户使用已安装的 `rust_plc`，可以进入 scaffold 目录继续工作
- 如果用户使用 `cargo run --release --bin rust_plc -- ...`，就必须留在 RustPLC 源码仓根目录，对 scaffold 使用路径参数
