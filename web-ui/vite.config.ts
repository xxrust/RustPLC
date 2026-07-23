import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const devProxyTarget = process.env.VITE_DEV_PROXY_TARGET || 'http://127.0.0.1:8080';
const devWsProxyTarget = devProxyTarget.replace(/^http/, 'ws');

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) {
            return undefined;
          }
          if (id.includes('monaco-editor') || id.includes('@monaco-editor')) {
            return 'monaco-editor';
          }
          if (id.includes('@xyflow')) {
            return 'xyflow';
          }
          if (id.includes('antd') || id.includes('@ant-design')) {
            return 'antd';
          }
          if (id.includes('react') || id.includes('scheduler')) {
            return 'react';
          }
          return undefined;
        },
      },
    },
  },
  server: {
    proxy: {
      '/api': {
        target: devProxyTarget,
        changeOrigin: true,
      },
      '/ws': {
        target: devWsProxyTarget,
        ws: true,
      },
    },
  },
})
