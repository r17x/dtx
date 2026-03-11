/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./crates/dtx-web/templates/**/*.html",
    "./static/js/**/*.js",
  ],
  theme: {
    extend: {
      colors: {
        'dtx': {
          'bg': '#0d1117',
          'panel': '#161b22',
          'card': '#1a1f26',
          'elevated': '#21262d',
          'border': '#2d333b',
        },
        'status': {
          'running': '#22c55e',
          'starting': '#f59e0b',
          'stopped': '#6b7280',
          'error': '#ec4899',
          'active': '#22d3ee',
          'ephemeral': '#a855f7',
        }
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Fira Code', 'SF Mono', 'Consolas', 'monospace'],
        sans: ['Inter', '-apple-system', 'BlinkMacSystemFont', 'Segoe UI', 'sans-serif'],
      },
      animation: {
        'pulse-slow': 'pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'pulse-fast': 'pulse 0.5s cubic-bezier(0.4, 0, 0.6, 1) infinite',
      }
    }
  },
  plugins: [],
}
