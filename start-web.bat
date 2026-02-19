@echo off
chcp 65001 >nul
REM RustPLC Web 启动脚本 (Windows)

echo 🚀 启动 RustPLC Web 服务器...

REM 检查前端构建产物
if not exist "web-ui\dist" (
    echo 📦 前端未构建，正在构建...
    cd web-ui
    call npm run build
    cd ..
)

REM 启动后端服务器
echo 🌐 启动后端服务器 (http://localhost:8080)...
cargo run -p web-server
