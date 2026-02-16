<p align="center">
  <h1 align="center">RustPLC</h1>
  <p align="center">
    <strong>Formally Verified Compiler for Industrial Control Systems</strong><br>
    Don't program devices — declare physical facts and intent, let the compiler prove it's safe.
  </p>
  <p align="center">
    <a href="README.md">中文</a> | <strong>English</strong>
  </p>
</p>

---

## Understand RustPLC in 30 Seconds

```mermaid
flowchart TD
    A["Engineer describes<br>process in plain language"] --> B["AI (Claude / Codex)<br>generates .plc via<br>plc-gen skill"]
    B --> C["RustPLC compiler<br>four verification engines"]
    C --> D{"Verification<br>passed?"}
    D -- "Yes" --> E["JSON IR output<br>for codegen / simulation"]
    D -- "No" --> F["Precise error report<br>line number + fix suggestion"]
    F --> B
```

**Traditional**: Engineer writes ladder logic → manual safety review → collisions/deadlocks/timeouts found during commissioning

**RustPLC**: Engineer describes process → AI generates declarative DSL → compiler mathematically proves safety → all issues caught at compile time

## Quick Start

```bash
git clone https://github.com/xxrust/RustPLC.git
cd RustPLC
cargo build --release
cargo run --release -- examples/two_cylinder.plc
```

```
Verification passed:
  - Safety: Complete proof (depth 4) — conflicts_with satisfied
  - Liveness: Passed — no deadlock risk
  - Timing: Passed
  - Causality: Passed — all signal chains connected
```

## AI-Assisted Generation Example

Describe your process in plain language in Claude Code, and the AI generates a verified `.plc` file:

**You**:

> I have an assembly station: two conveyors deliver workpieces, then push cylinders push them to center, a press cylinder assembles, then an eject cylinder pushes the product out. The press must not act unless both pushers are extended. Eject must not act while press is extended.

**AI infers topology & safety constraints**:

```
Left conveyor:   Y0 → motor_left → sensor_left_arrive → X0
Left push cyl:   Y2 → valve_push_L → cyl_push_L → sensor_push_L_ext → X2
Press cylinder:  Y4 → valve_press → cyl_press → sensor_press_ext → X6
...

Safety constraints:
  cyl_press.extended requires cyl_push_L.extended
  cyl_press.extended requires cyl_push_R.extended
  cyl_eject.extended conflicts_with cyl_press.extended
```

**AI generates `.plc` and auto-verifies**:

```
Verification passed:
  - Safety: Complete proof (depth 14) — requires/conflicts_with satisfied
  - Liveness: Passed — no deadlock risk
  - Timing: Passed
  - Causality: Passed — all signal chains connected
```

Full file: [`examples/assembly_station.plc`](examples/assembly_station.plc)

## Four Verification Engines

| Engine | Checks | Method |
|--------|--------|--------|
| **Safety** | Mutual exclusion (`conflicts_with`), dependencies (`requires`) | Bounded Model Checking + k-induction |
| **Liveness** | Deadlock / livelock (unguarded waits, zero-outdegree states) | Tarjan SCC + reachability |
| **Timing** | Timing envelope (`must_complete_within` / `worst_case`) | Worst-case critical path |
| **Causality** | Signal chain integrity (can signals propagate along topology?) | Topology BFS |

All four engines run in parallel — one compilation exposes all issues. On failure, precise diagnostics:

```
ERROR [safety] Safety constraint violated
  Location: task cycle.step together
  Cause: cyl_A.extended and cyl_B.extended both true in parallel branches
  Suggestion: Make conflicting actions sequential

ERROR [liveness] Potential deadlock
  Location: task main.step_wait
  Cause: wait condition has no timeout branch
  Suggestion: Add timeout: <duration> -> goto <recovery task>
```

## From Verification to Deployment

After `.plc` verification passes, proceed to simulation and board-level deployment:

```mermaid
flowchart LR
    A[".plc verified"] --> B["SIL Simulation<br>fault injection / waveform / regression"]
    A --> C["RP2040 Deployment<br>cross-compile / flash / trace diff"]
    B --> D["trace-diff<br>SIL vs board comparison gate"]
    C --> D
```

```bash
# SIL simulation
cargo run --release -- sim-plc examples/assembly_station.plc \
  --scenario scenarios/normal.yaml --out trace.jsonl

# RP2040 firmware build
cargo run --release -- build-rp2040 examples/assembly_station.plc \
  --out out/rp2040 --io-map out/rp2040/io_map.toml --emit-uf2 out/firmware.uf2
```

## 📚 Documentation

For in-depth content, see the **[Wiki](https://github.com/xxrust/RustPLC/wiki)**:

| Page | Content |
|------|---------|
| [Quick Start](https://github.com/xxrust/RustPLC/wiki/Quick-Start) | 5-minute setup: install, build, run |
| [DSL Language Reference](https://github.com/xxrust/RustPLC/wiki/DSL-Language-Reference) | Full syntax reference: topology, constraints, control logic |
| [Architecture](https://github.com/xxrust/RustPLC/wiki/Architecture) | Compilation pipeline, module structure, IR design |
| [Verification Engines](https://github.com/xxrust/RustPLC/wiki/Verification-Engines) | Engine internals and mathematical foundations |
| [SIL Simulation](https://github.com/xxrust/RustPLC/wiki/SIL-Simulation) | Simulation loop: scenarios, fault injection, batch regression |
| [RP2040 Deployment](https://github.com/xxrust/RustPLC/wiki/RP2040-Deployment) | Cross-compilation, I/O mapping, flashing, trace comparison |
| [Examples Gallery](https://github.com/xxrust/RustPLC/wiki/Examples-Gallery) | Example files with industrial scenario walkthroughs |
| [AI Assisted Generation](https://github.com/xxrust/RustPLC/wiki/AI-Assisted-Generation) | Full AI dialogue workflow for generating `.plc` files |
| [Contributing](https://github.com/xxrust/RustPLC/wiki/Contributing) | Development guide, testing, code structure |

## Roadmap

- [x] DSL design and parser
- [x] Four formal verification engines (Safety / Liveness / Timing / Causality)
- [x] Structured error reporting (line numbers + fix suggestions)
- [x] DSL v2: delay / repeat / wait AND|OR / if-else / goto task.step / custom states
- [x] AI-assisted generation (plc-gen skill)
- [x] Analog I/O (analog_input / analog_output / set_analog / threshold comparison)
- [x] SIL simulation loop (SimIO / Plant / fault injection / waveform export / batch regression)
- [x] Code generation + RP2040 build/flash (build-rp2040 / flash-rp2040)
- [x] Board-level observability & SIL comparison (trace-parse / trace-diff)
- [ ] Hardware abstraction layer (EtherCAT / Modbus / more GPIO boards)
- [ ] PID control
- [ ] Multi-controller coordination
- [ ] Graphical DSL editor

## License

MIT

---

<p align="center">
  <sub>Written in Rust, so it won't panic. Well, at least not on the production line.</sub>
</p>
