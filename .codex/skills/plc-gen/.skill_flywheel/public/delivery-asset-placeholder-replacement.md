# Delivery Asset Placeholder Replacement

这个工件只回答一个问题：

> scaffold 之后，哪些文件必须从占位态变成真实 authored asset，哪些占位字符串还在就绝对不能收工？

## root 与 delivery asset 的关系

- root `plc/main.system.md` 是项目级 bridge / 索引
- 对 complex delivery，真正的 authored asset 在 `plc/deliveries/<layer>/<slug>/docs/*.md`、`plc/deliveries/<layer>/<slug>/plc/main.bundle.toml`、`plc/deliveries/<layer>/<slug>/scenarios/nominal/normal.yaml`

不要只替换 root scaffold 文档。
也不要只把 confirmed facts 写进 root `plc/main.system.md` 却不改 delivery asset docs。

## 必须替换的 delivery asset 文件

至少检查并 authoring：

- `docs/<layer>.system.md`
- `docs/<layer>.architecture.md`
- `docs/<layer>.verification.md`
- `docs/<layer>.intent_alignment.contract.json`
- `plc/main.bundle.toml`
- `scenarios/nominal/normal.yaml`

## 这些占位字符串还在，就不能叫完成

以下任一仍然存在，都说明交付还停在 scaffold：

- `Default Starter Flow`
- `starter`
- `Replace with the authored`
- `replace_me_after_authoring`
- `replace_after_intent_doctor`
- non-lowercase or mismatched `source_digest.value`

## 外部 confirmed `.system.md` 的正确落盘顺序

1. 先确认 authoritative source 没有乱码；含中文等非 ASCII 时显式按 UTF-8 读取
2. 再把 confirmed facts 写进 delivery asset `docs/*.system.md`
3. 再同步到 `docs/*.architecture.md` / `docs/*.verification.md`
4. 再 authoring `plc/main.bundle.toml` 与 fragments
5. 再修 scenario
6. 最后 authoring intent sidecar，或显式报告 blocker

## 正确的 stop rule

如果执行结果是：

- delivery asset docs 仍是 `Default Starter Flow`
- sidecar 仍是 `starter intent contract`
- source digest 仍是 `replace_me_after_authoring`
- source digest 不是绑定源文件的 lowercase SHA-256 hex

那么正确结论是：

- `blocked by missing contract`
- 或 `implementation-mistake`

而不是 `generated`、`validated with warnings`，更不是 `validated`。
