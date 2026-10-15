/** 图谱实体类别的视觉映射。
 *
 * 颜色取自 spectrum 调色板，sigil 用几何符号直接编码类别，
 * 使节点在缩放后仍可辨类（不依赖颜色，色盲友好）。
 */

export interface KindPalette {
  base: string;
  glow: string;
  label: string;
  sigil: string;
}

export const GRAPH_PALETTE: Record<string, KindPalette> = {
  file: { base: '#5ee0f5', glow: '#b8f2fb', label: '文件', sigil: '▤' },
  function: { base: '#8b7cff', glow: '#cfc7ff', label: '函数', sigil: 'ƒ' },
  topic: { base: '#f0a332', glow: '#fbd9a4', label: '话题', sigil: '✦' },
};

export const UNKNOWN_KIND: KindPalette = {
  base: '#7c89a3',
  glow: '#b9c2d1',
  label: '其他',
  sigil: '●',
};

export function kindPalette(kind: string): KindPalette {
  return GRAPH_PALETTE[kind] ?? UNKNOWN_KIND;
}
