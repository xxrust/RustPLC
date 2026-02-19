#!/bin/bash

# RustPLC Web 启动脚本

echo "🚀 启动 RustPLC Web 服务器..."

# 检查前端构建产物
if [ ! -d "web-ui/dist" ]; then
    echo "📦 前端未构建，正在构建..."
    cd web-ui && npm run build && cd ..
fi

# 启动后端服务器
echo "🌐 启动后端服务器 (http://localhost:8080)..."
cargo run -p web-server
