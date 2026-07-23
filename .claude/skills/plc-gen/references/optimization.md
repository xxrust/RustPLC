# plc-gen Optimization

当用户提到“优化”“提速”“候选方案”“更短节拍”时，必须读取本文件。

## 当前真实能力

RustPLC 当前提供的是 library 级 optimization pipeline，不是 CLI subcommand。

公开入口：

```rust
use rust_plc::optimization::optimize_plc_source;
```

主入口返回：
- ranked candidate 列表
- timing summary
- legality verdict
- emitted optimized PLC source

## 当前支持的 rewrite kind

现在只支持这 5 类保守 rewrite：
- `SwapAdjacentSteps`
- `ParallelizeAdjacentSteps`
- `RemoveRedundantWaitStep`
- `MergeAdjacentDelaySteps`
- `ReplaceTimeoutRecoveryTarget`

这些 rewrite 都围绕 task AST、相邻 step、现有 timing / verification 复用来做。

## 必须明确告诉用户的边界

不要承诺以下能力，因为当前并不存在：
- `rust_plc optimize ...` 之类的 CLI
- topology 或 `[constraints]` 的自动改写
- 全局调度搜索
- optimization 专用 legality / verification 规则
- 自动应用到整个源文件的“大规模重构器”

当前 emitted source 只重组 `[tasks]`，保留原有非 task 前缀。

## 什么时候主动提 optimization

只有以下场景才主动提：
- 用户明确要求优化现有 PLC
- 用户要候选方案排序
- 用户要比较多个保守 rewrite 的 timing / legality

普通“生成一个可用 PLC”请求，不要强行把 optimization 当主路径。

## 回答 optimization 请求时的正确说法

必须把结论说清楚：
- 这是 library API，不是 CLI
- 当前支持的 rewrite kind 只有 5 类
- legality 复用现有 semantic + verification pipeline
- timing 复用现有 timing engine

## 最小示例

```rust
use rust_plc::optimization::optimize_plc_source;

let source = std::fs::read_to_string("examples/two_cylinder.plc")?;
let candidates = optimize_plc_source(&source)?;

for candidate in candidates.iter().take(3) {
    println!(
        "{} legal={} nominal_ms={} rewrite={}",
        candidate.id,
        candidate.legality.is_legal,
        candidate.timing.global_nominal_ms,
        candidate.rewrite.summary
    );
}
```
