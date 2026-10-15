/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // 地色：略带蓝的近黑（不是中性纯黑，长时间读代码更稳）
        bg: {
          DEFAULT: '#0b0d12',
          card: '#11141b',
          elev: '#171b24',
        },
        // 交互色只有一个，只出现在"光"该在的地方：焦点、选中、活动态
        primary: {
          DEFAULT: '#5ee0f5',
          soft: 'rgba(94,224,245,.09)',
          dim: 'rgba(94,224,245,.35)',
        },
        border: {
          DEFAULT: 'rgba(255,255,255,.07)',
          dim: 'rgba(255,255,255,.045)',
          hover: 'rgba(255,255,255,.15)',
          active: 'rgba(94,224,245,.45)',
        },
        text: {
          DEFAULT: '#e9edf5',
          2: '#aab4c6',
          3: '#727e93',
          4: '#4d5767',
        },
        // 光谱：只用于数据身份编码（agent / 模型 / 实体类别），不做装饰
        spectrum: {
          violet: '#8b7cff',
          cyan: '#35d5e8',
          amber: '#f0a332',
          rose: '#f2557f',
          green: '#3ddc97',
          azure: '#4d8dff',
          lime: '#a8d94a',
          coral: '#ff7a5c',
          slate: '#7c89a3',
        },
      },
      fontFamily: {
        sans: [
          '-apple-system',
          'BlinkMacSystemFont',
          'Segoe UI Variable Display',
          'Segoe UI',
          'Inter',
          'system-ui',
          'PingFang SC',
          'Microsoft YaHei',
          'sans-serif',
        ],
        mono: [
          'SF Mono',
          'Cascadia Mono',
          'JetBrains Mono',
          'Consolas',
          'ui-monospace',
          'monospace',
        ],
      },
      fontSize: {
        micro: ['10px', { lineHeight: '14px', letterSpacing: '.02em' }],
        label: ['11px', { lineHeight: '15px', letterSpacing: '.03em' }],
        note: ['12px', { lineHeight: '17px' }],
        body: ['13px', { lineHeight: '20px' }],
        lead: ['14px', { lineHeight: '22px' }],
        title: ['17px', { lineHeight: '24px' }],
        display: ['26px', { lineHeight: '30px' }],
      },
    },
  },
  plugins: [require('@tailwindcss/typography')],
};
