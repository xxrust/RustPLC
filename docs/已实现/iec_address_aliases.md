# IEC 地址别名（最小子集）与 RustPLC 逻辑通道映射

日期：2026-02-18

本文件定义 RustPLC 工具链层对 IEC 61131-3 地址的一小部分解析规则，用于：

- 从 OpenPLC / IEC 工程习惯迁移到 RustPLC 时的别名映射；
- 支持 `io-map-normalize` 把含 IEC key 的 `io_map.toml` 规范化为 `di/do/ai/ao` key。

注意：该能力是“工具链别名”，不引入 IEC 内存模型语义到 DSL/runtime 核心。

---

## 支持的语法

仅支持以下最小子集（大小写不敏感，允许前后空白）：

- `%IXn.m`：输入位（Input Bit）
- `%QXn.m`：输出位（Output Bit）
- `%IWn`：输入字（Input Word）
- `%QWn`：输出字（Output Word）

其中：

- `n` 是非负整数
- `m` 是位索引，范围 `0..=7`

---

## 映射规则（IEC -> RustPLC logical id）

### 1) Bit 地址：`%IXn.m` / `%QXn.m`

IEC bit 地址用 `n.m` 表示“第 n 个字节里的第 m 位”。

RustPLC 逻辑通道 id 计算：

```
id = n * 8 + m
```

映射：

- `%IXn.m` -> `di{id}`
- `%QXn.m` -> `do{id}`

例子：

- `%IX0.0` -> `di0`
- `%IX1.0` -> `di8`
- `%QX2.3` -> `do19`

### 2) Word 地址：`%IWn` / `%QWn`

RustPLC 逻辑通道 id：

```
id = n
```

映射：

- `%IWn` -> `ai{id}`
- `%QWn` -> `ao{id}`

例子：

- `%IW0` -> `ai0`
- `%QW12` -> `ao12`

---

## 错误与边界

- 不支持的地址（例如 `%MW0`）会报错
- bit 地址缺少 `.m` 会报错
- bit 索引 `m` 超出 `0..=7` 会报错

