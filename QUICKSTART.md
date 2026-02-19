# RustPLC Web UI - 快速启动指南

## ✅ 服务器已启动

RustPLC Web 服务器正在运行：
- **地址**: http://localhost:8080
- **API**: http://localhost:8080/api

## 🌐 访问方式

在浏览器中打开：
```
http://localhost:8080
```

## 📡 可用的 API 端点

### 运行相关
- `GET /api/run/list` - 列出运行记录
- `GET /api/run/:id/status` - 获取运行状态
- `POST /api/run/no-board-gate` - 触发 No-Board 运行

### 拓扑相关
- `GET /api/topology/:id` - 获取拓扑
- `POST /api/topology/validate` - 验证拓扑

### 场景相关
- `GET /api/scenario/:id` - 获取场景
- `POST /api/scenario/validate` - 验证场景

### 诊断相关
- `GET /api/diagnosis/:id` - 获取诊断报告
- `GET /api/alarms` - 获取告警列表

### Trace 相关
- `GET /api/trace/:id` - 获取 Trace 数据

## 🛑 停止服务器

按 `Ctrl+C` 停止服务器

## 🔄 重新启动

### Windows
```bash
start-web.bat
```

### Linux/Mac
```bash
./start-web.sh
```

### 手动启动
```bash
# 从项目根目录
cargo run -p web-server
```

## 📝 注意事项

1. **首次启动**: 确保前端已构建（`cd web-ui && npm run build`）
2. **端口占用**: 如果 8080 端口被占用，修改 `crates/web-server/src/main.rs` 中的端口号
3. **开发模式**: 前端开发时可以运行 `cd web-ui && npm run dev`，会在 5173 端口启动热重载服务器

## 🎯 功能页面

- **总览看板** - `/` - 运行状态、告警统计
- **拓扑编辑** - `/topology` - 组件拓扑配置
- **场景管理** - `/scenario` - 场景编辑
- **运行监控** - `/run` - 触发运行、查看状态
- **Tick 回放** - `/replay` - 历史回放
- **诊断中心** - `/diagnosis` - 告警和诊断
- **审计日志** - `/audit` - 操作审计

## 🐛 故障排查

### 页面无法访问
1. 检查服务器是否启动：`curl http://localhost:8080/api/run/list`
2. 检查前端是否构建：`ls web-ui/dist/`
3. 查看服务器日志

### API 返回 404
- 确保 URL 以 `/api` 开头
- 检查端点路径是否正确

### 前端显示空白
1. 打开浏览器开发者工具（F12）
2. 查看 Console 是否有错误
3. 检查 Network 标签页，确认资源加载成功
