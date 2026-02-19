/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        // Dark theme colors
        'app-bg': '#1a1a1a',
        'panel-bg': '#2d2d2d',
        'canvas-bg': '#1e1e1e',
        'text-primary': '#e0e0e0',
        'text-secondary': '#a0a0a0',
        'accent-primary': '#00bcd4',
        'status-pass': '#52c41a',
        'status-warning': '#faad14',
        'status-fail': '#f5222d',
        'status-info': '#1890ff',
        'status-running': '#722ed1',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'Fira Code', 'monospace'],
      },
      spacing: {
        // 4px grid system
        '1': '4px',
        '2': '8px',
        '3': '12px',
        '4': '16px',
        '5': '20px',
        '6': '24px',
        '7': '28px',
        '8': '32px',
      },
    },
  },
  plugins: [],
}
