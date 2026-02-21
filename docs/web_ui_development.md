# RustPLC Web UI - 开发文档

## 项目概述

RustPLC Web UI 是一个基于 React + TypeScript 的现代化 Web 界面，用于 RustPLC 工业控制系统的可视化操作、监控和诊断。

## 架构设计

### 前端技术栈
- **React 19** - 声明式 UI 框架
- **TypeScript** - 类型安全
- **Ant Design 6** - 企业级 UI 组件库
- **Zustand** - 轻量级状态管理
- **TanStack Query** - 服务端状态管理
- **Axios** - HTTP 客户端
- **React Router v7** - 客户端路由
- **Vite** - 快速构建工具

### 后端技术栈
- **Axum** - 高性能异步 Web 框架
- **Tokio** - 异步运行时
- **Tower** - 中间件生态
- **Serde** - 序列化/反序列化

## 快速开始

### 1. 开发模式（前后端分离）

**启动前端开发服务器：**
```bash
cd web-ui
npm install
npm run dev
```
访问 http://localhost:5173

**启动后端 API 服务器：**
```bash
cargo run -p web-server
```
API 地址：http://localhost:8080/api

### 2. 生产模式（集成部署）

**构建并启动：**
```bash
# Linux/Mac
./start-web.sh

# Windows
start-web.bat
```

或手动执行：
```bash
# 1. 构建前端
cd web-ui && npm run build && cd ..

# 2. 启动后端（自动服务前端静态文件）
cargo run -p web-server --release
```

访问 http://localhost:8080

## 项目结构

```
rust_plc/
├── web-ui/                    # 前端项目
│   ├── src/
│   │   ├── components/        # 可复用组件
│   │   ├── pages/            # 页面组件
│   │   │   └── Dashboard.tsx # 总览看板
│   │   ├── layouts/          # 布局组件
│   │   │   └── MainLayout.tsx
│   │   ├── services/         # API 服务层
│   │   │   └── api.ts        # API 客户端
│   │   ├── stores/           # Zustand 状态管理
│   │   │   └── appStore.ts   # 全局应用状态
│   │   ├── types/            # TypeScript 类型定义
│   │   │   └── index.ts      # 后端契约类型
│   │   ├── App.tsx           # 应用根组件
│   │   └── main.tsx          # React 入口
│   ├── dist/                 # 构建产物
│   └── package.json
│
└── crates/
    └── web-server/           # 后端服务器
        ├── src/
        │   └── main.rs       # Axum 服务器
        └── Cargo.toml
```

## 核心功能模块

### 1. 总览看板 (Dashboard)
- 运行模式切换（No-Board / HIL / Live）
- 实时告警统计
- 最近运行记录
- 快速操作入口

### 2. 拓扑编辑器 (Topology Editor)
- 组件库管理
- 拓扑图可视化编辑
- 连接关系配置
- 拓扑验证

### 3. 场景管理器 (Scenario Manager)
- 场景 YAML 编辑
- 事件时间线配置
- 故障注入配置
- 场景验证

### 4. 运行监控 (Runtime Monitor)
- 触发 No-Board Gate
- 实时运行状态
- Trace 数据查看
- 诊断报告展示

### 5. Tick 回放 (Playback)
- 历史 Trace 回放
- 逐 Tick 步进
- 信号波形展示
- 状态快照对比

### 6. 诊断中心 (Diagnostics)
- 告警列表（按严重程度分类）
- 诊断候选项展示
- 证据链追溯
- 修复建议

### 7. 审计日志 (Audit Log)
- 操作记录追踪
- 用户行为审计
- 关键操作回溯

## API 契约

### 后端 API 端点

```
GET  /api/topology/:id          # 获取拓扑
POST /api/topology/validate     # 验证拓扑
PUT  /api/topology/:id          # 保存拓扑

GET  /api/scenario/:id          # 获取场景
POST /api/scenario/validate     # 验证场景
PUT  /api/scenario/:id          # 保存场景

POST /api/run/no-board-gate     # 触发 No-Board 运行
GET  /api/run/:id/status        # 获取运行状态
GET  /api/run/list              # 列出运行记录

GET  /api/trace/:id             # 获取 Trace 数据
GET  /api/trace/:id/range       # 获取 Tick 范围数据

GET  /api/diagnosis/:id         # 获取诊断报告
GET  /api/timing/:id            # 获取时序报告

GET  /api/alarms                # 获取告警列表
POST /api/alarms/:id/ack        # 确认告警
```

### 数据类型

所有类型定义位于 `web-ui/src/types/index.ts`，与后端 JSON 工件保持一致：

- `ComponentTopology` - 组件拓扑
- `ComponentScenario` - 场景配置
- `RunStatus` - 运行状态
- `TraceData` - Trace 数据
- `DiagnosisReport` - 诊断报告
- `AlarmEvent` - 告警事件
- `TimingReport` - 时序报告

### 标签字段契约（`tags_schema_version = 1`）

`/api/topology/:id` 与 `/api/topology/parse-plc` 的返回值统一遵循以下 tags 契约：

- 顶层返回 `tags_schema_version`（当前固定为 `1`）。
- 每个 `components[].params.tags` 都是固定 shape（缺失字段会补空数组）：

```json
{
  "functional_group": [],
  "danger_level": [],
  "location_group": []
}
```

- `location_group` 支持层级路径（例如 `line_a/cell_2/station_7`），前端 store 允许按层级前缀检索（如 `line_a/cell_2` 命中该工位）。

## 状态管理

### Zustand Store (`appStore.ts`)

```typescript
{
  runMode: 'no_board' | 'hil_board' | 'runtime_live',
  currentUser: { id, name, role },
  currentProject: string | null,
  hasUnsavedChanges: boolean,
  alarmCount: { info, warning, critical }
}
```

### React Query

用于服务端数据获取和缓存：
- 自动重试
- 缓存管理
- 乐观更新
- 后台刷新

## 开发指南

### 添加新页面

1. 在 `src/pages/` 创建页面组件
2. 在 `src/App.tsx` 添加路由
3. 在 `src/layouts/MainLayout.tsx` 添加菜单项

### 添加新 API

1. 在 `src/services/api.ts` 添加 API 函数
2. 在 `src/types/index.ts` 定义类型
3. 使用 React Query 调用：

```typescript
const { data, isLoading } = useQuery({
  queryKey: ['key'],
  queryFn: () => api.method(),
});
```

### 添加全局状态

在 `src/stores/appStore.ts` 中扩展 store：

```typescript
interface AppState {
  newState: string;
  setNewState: (value: string) => void;
}
```

## 环境变量

### 开发环境 (`.env.development`)
```
VITE_API_BASE_URL=http://localhost:8080/api
```

### 生产环境 (`.env.production`)
```
VITE_API_BASE_URL=/api
```

## 构建与部署

### 开发构建
```bash
cd web-ui && npm run build
```

### 生产构建
```bash
cd web-ui && npm run build
cargo build -p web-server --release
```

### Docker 部署（待实现）
```dockerfile
FROM rust:1.75 as builder
# ... 构建后端

FROM node:20 as frontend
# ... 构建前端

FROM debian:bookworm-slim
# ... 运行时镜像
```

## 安全考虑

- JWT Token 认证（待实现）
- CORS 配置
- CSRF 防护
- 输入验证
- 审计日志

## 性能优化

- 代码分割（动态 import）
- 懒加载路由
- React Query 缓存
- Vite 构建优化
- Gzip 压缩

## 下一步计划

- [ ] 实现拓扑编辑器可视化
- [ ] 实现场景编辑器
- [ ] 实现 Tick 回放器
- [ ] WebSocket 实时推送
- [ ] 用户认证与授权
- [ ] 角色权限管理
- [ ] 审计日志持久化
- [ ] 国际化支持

## 参考文档

- [功能规格说明](../docs/web_ui_functional_spec.md)
- [React 文档](https://react.dev)
- [Ant Design 文档](https://ant.design)
- [Axum 文档](https://docs.rs/axum)
