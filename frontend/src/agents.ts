/** Agent 身份的视觉映射（UI 常量 — 非接口契约）。
 *
 * 颜色来自 spectrum 调色板，只用于数据身份编码（区分是哪个 agent），
 * 顺序按安装量/常见度排，取色时相邻项刻意拉开色相，避免列表中混淆。
 */

export const AGENT_COLORS: Record<string, string> = {
  opencode: '#8b7cff',
  'claude-code': '#f0a332',
  codex: '#35d5e8',
  cursor: '#f2557f',
  'gemini-cli': '#3ddc97',
  qoder: '#4d8dff',
  trae: '#a8d94a',
  'kimi-code': '#ff7a5c',
  kilocode: '#e879f9',
  openclaw: '#fb923c',
  unknown: '#7c89a3',
};

export const AGENT_LABELS: Record<string, string> = {
  opencode: 'OpenCode',
  'claude-code': 'Claude Code',
  codex: 'Codex',
  cursor: 'Cursor',
  'gemini-cli': 'Gemini CLI',
  qoder: 'Qoder',
  trae: 'Trae',
  'kimi-code': 'Kimi Code',
  kilocode: 'KiloCode',
  openclaw: 'OpenClaw',
  unknown: '未知',
};

/** 有序取色表，供没有稳定 agent 归属的数据（模型名等）分配颜色。 */
const IDENTITY_CYCLE = [
  '#8b7cff',
  '#35d5e8',
  '#f0a332',
  '#3ddc97',
  '#f2557f',
  '#4d8dff',
  '#a8d94a',
  '#ff7a5c',
];

/* colorFor 在 agents.ts 里按"顺序来自 stats 返回"分配，故用简单确定性哈希，
   同一个名字在任意刷新间拿到同一颜色（避免每次重排都换色）。 */
export function colorForName(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) | 0;
  }
  return IDENTITY_CYCLE[Math.abs(h) % IDENTITY_CYCLE.length];
}

export function agentColor(agentType: string): string {
  return AGENT_COLORS[agentType] ?? colorForName(agentType);
}

export function agentLabel(agentType: string): string {
  return AGENT_LABELS[agentType] || agentType;
}
