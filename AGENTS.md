# Agent Notes

- When introducing a new `TransitionAction` variant, update all dependent layers together: semantic lowering (`src/semantic/mod.rs`), runtime bridging (`src/runtime_bridge.rs`), ST codegen (`src/codegen/st.rs`), and safety/diagnostic matchers (`src/verification/safety.rs`).
- Extern-function support spans AST + IR: keep `topology.extern_functions`/call bindings aligned across `src/ast/mod.rs`, `src/ir/mod.rs`, and semantic topology/state-machine lowering tests.
- Extern `action: call` return bindings are type-checked against `[topology] variable` declarations in semantic analysis; declare bound variables up front and keep extern signature tests line-aware.
