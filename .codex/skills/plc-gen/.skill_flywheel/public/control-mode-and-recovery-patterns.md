# Control Mode And Recovery Patterns

如果 `.system.md` 同时出现自动、手动、单步、维护四类模式，默认采用下面的建模偏好。

## 模式建模

- 自动主流程：保留为正式生产 task。
- 手动 / 单步 / 维护：优先放进单独的 service task，而不是把自动节拍搅乱。
- 模式互斥：用明确 safety 约束表达，不靠口头约定。

## 恢复建模

- warning 路径：
  - 典型语义是“挂起当前相关流程”
  - 修复后操作员按刷新按钮
  - `wait refresh_button == true` 常配 `allow_indefinite_wait: true`
  - 完成后回原 task 的等待入口

- fault 路径：
  - 典型语义是“停机 / 清料 / 告警”
  - 先关闭执行器或回安全位
  - 再进入人工确认或刷新等待
  - 不要假装 fault 会自动无条件恢复

## 何时需要 supervisor

如果 system contract 同时包含：

- 上电初始化
- 正常停机清料
- 模式切换入口
- 统一 stop 请求

优先增加 supervisor task 管理这些全局入口，而不是把 stop / init 条件散落到每个生产 task 里。
