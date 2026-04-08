# plc-gen Public Brief Template

本文定义主 agent 在复杂项目里必须先准备的 `public brief`。

目的只有一个：
让看不到源码的子 agent 也能在固定边界内工作，而不是重新猜需求或越权读仓库。

## 1. brief 最低结构

### 任务目标

- 用户想交付什么
- 是修复、生成、重构，还是项目级交付
- 最终判定成功的条件

### 当前 source shape

- 单文件 `.plc`
- 还是 `.bundle.toml` + fragments
- 是否已经存在 scaffold 项目布局

### 已冻结的 system / lowering facts

- 已确认的 task 划分
- blocking / timeout / wait / delay / axis.move_* 事实
- 已确认的 topology-closed device action 语义
- mode / supervisor / warning / fault 分流
- shared resource / interlock / counter / retry 等结构

这里只写已经冻结的事实，不写猜测。

### 当前已有文件

- 当前已存在哪些关键文件
- 这些文件目前承担什么角色
- 哪些文件这轮允许修改

### 期望写入物

- 这轮应由 skill 写入哪些文件
- 哪些文件只是可选写入
- 哪些东西绝不能当作写入物，因为它们属于工具链产物

### authored artifact 范围

- 是否需要 scenario
- 是否需要可选 `*.intent_alignment.contract.json`
- 是否需要 canonical fixture / golden path 资产

### 不可改变的边界

- 不允许破坏的 source boundary
- 不允许重命名的 task / file / artifact
- 不允许擅自补全的未冻结 contract

### blocker / assumptions

- 当前明确 blocker
- 当前允许的 assumptions
- 哪些 assumptions 一旦变化会改变 source shape

### 成功判据

- 什么叫“实现者已收敛”
- 什么叫“reviewer 可出场”
- 最终允许交付需要满足什么

## 2. brief 使用规则

- brief 由主 agent 生成
- 子 agent 默认只消费 brief，不直接消费仓库源码
- brief 不足时，子 agent 应报告缺口
- 缺口由主 agent 补 brief，而不是把源码阅读权限下放成默认行为

## 3. 禁止事项

- 不要把“去看源码就知道”写进 brief
- 不要把命令清单当作 brief 主体
- 不要把未冻结猜测包装成已确认事实
- 不要省略“不允许改变的边界”

## 4. 最小示例

```md
任务目标：
- 修复现有双气缸项目，使其通过 project-level gate

当前 source shape：
- 已存在 scaffold 项目
- DSL entry 为 `plc/main.plc`

已冻结的 system / lowering facts：
- 两个 task：`main`、`fault_recovery`
- `axis.move_*` 仍按 blocking 长时动作处理

当前已有文件：
- `plc/main.system.md`
- `plc/main.plc`
- `scenarios/nominal/normal.yaml`

期望写入物：
- 允许修改 `plc/main.plc`
- 允许修改 `scenarios/nominal/normal.yaml`
- 本轮不生成 intent sidecar

不可改变的边界：
- 不得把单文件拆成 bundle
- 不得重命名 `main` task

blocker / assumptions：
- 允许继续使用现有 device 名称
- 不允许新增未确认 sensor

成功判据：
- 实现者提交范围内已收敛
- reviewer 复核后允许交付
```
