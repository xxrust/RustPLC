# RustPLC 安全与架构审计改动总结

审计日期：2026-07-23

工作区：`E:\personal_project\rust_plc`

本次改动把 Web 控制面、semantic 前门、runtime 所有权、并发验证和依赖安全收敛到可执行、可验证、可持续门禁的工程状态。

## 机制链

```text
External user / Web UI / CLI
              |
              v
  +-----------------------------+
  | Web security front door     |
  | localhost default           |
  | Bearer auth + CORS allowlist|
  | path / size / time limits   |
  +--------------+--------------+
                 |
                 v
Parser -> AST -> Shared Semantic Compile -> Canonical IR
                                      |          |
                         +------------+          +-------------+
                         v                                     v
                 Verification                         Runtime Bridge
          safety/liveness/timing/causality        checked lowering + arena
                         |                                     |
                         +----------------+--------------------+
                                          v
                                     runtime-core
                                 task-local tick execution
                                          |
                              +-----------+-----------+
                              v                       v
                           I/O HAL                 Codegen / Sim

Persistence boundaries:
workspace inputs -> canonical containment check -> bounded artifacts
run requests      -> semaphore + timeout          -> bounded run records
```

## 已完成

- Web 默认仅监听 localhost；远程访问要求 Bearer Token 与 CORS 白名单。
- 路径 containment、UUID run ID、并发、超时、文件、输出和记录限制已经落地。
- raw I/O、重复设备、非有限数值、表达式预算和 panic 路径已经进入 semantic front-door。
- CLI 与 Web 统一使用 `compile_semantic_program_with_library`。
- runtime bridge 使用 `Bump + self_cell` 管理程序生命周期，已移除 bridge 中的 `Box::leak`。
- runtime 与 verification 统一 graph-based task roots、extern WCET、carrier/workpiece 和 pending action 语义。
- Rust 与 npm 已知漏洞依赖已经升级，npm 官方审计为 0 漏洞。
- CI 增加 RustSec、npm audit、前端生产构建和 `--all-targets` workspace 测试。

## 残余风险收敛

- Web 架构：已完成第一轮职责拆分。`main.rs` 从约 5377 行降到约 3102 行，协作通道、artifact 存储、运行服务、安全配置和测试分别进入独立模块；HTTP handler 聚合仍可继续细分。
- CLI 架构：`utilities.rs` 从约 3718 行降到约 1615 行，文档/示例索引与 KEYENCE 生成器已恢复独立命令域。
- 前端质量：`npm run lint` 已达到 0 errors / 0 warnings。
- 嵌入式供应链：升级 `defmt`、`defmt-rtt` 与 `panic-probe` 后移除 `proc-macro-error2`；RustSec 仍报告 4 个来自 Cortex-M、STM32 HAL 与 PIO 上游的停止维护警告，当前没有已知漏洞或 unsound 项。
- 前端性能：Monaco 改为 editor API 与单一通用 worker，编辑器 chunk 从约 4.46 MB 降到约 2.66 MB，并继续保持路由级懒加载。

## 验证结果

```text
cargo fmt --all                                            PASS
cargo check --workspace --all-targets                     PASS
cargo test --workspace --all-targets                      PASS
git diff --check                                           PASS
cargo audit --no-fetch --stale --deny unsound --deny yanked PASS
npm audit --registry=https://registry.npmjs.org            0 vulnerabilities
npm run build                                               PASS
npm run lint                                                0 errors / 0 warnings
```

本报告只归纳审计相关修复。当前工作树还包含用户原有改动、新模块和文档。
