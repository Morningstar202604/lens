/** 图谱实体类视觉映射（代码域;演示域配色见 demo/novel_graph.html 自带样式）。 */

export interface KindPalette {
  base: string;
  glow: string;
  label: string;
  sigil: string;
}

export const GRAPH_PALETTE: Record<string, KindPalette> = {
  file: { base: '#6366f5', glow: '#a5b4fc', label: '文件', sigil: '▤' },
  function: { base: '#06b6d4', glow: '#a5f3fc', label: '函数', sigil: 'ƒ' },
  topic: { base: '#f59b0b', glow: '#fde68a', label: '话题', sigil: '✦' },
};

export const UNKNOWN_KIND: KindPalette = { base: '#6b7280', glow: '#9ca3af', label: '其他', sigil: '●' };

export function kindPalette(kind: string): KindPalette {
  return GRAPH_PALETTE[kind] ?? UNKNOWN_KIND;
}
