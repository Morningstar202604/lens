/**
 * GraphPanel — 选中节点的详情面板。
 *
 * 结构：类别标识 + 名称 + 权重 + 按关系分组的关联列表（可点击跳转）。
 */
import type { GraphNode, GraphData, GraphEdge } from '../types';
import { kindPalette as palette } from '../graph-palette';

interface Props {
  node: GraphNode;
  data: GraphData;
  onClose: () => void;
  onJump: (id: string) => void;
}

const RELATION_LABELS: Record<string, string> = {
  contains: '包含',
  references: '引用',
  related_to: '相关',
  co_occurs: '共现',
};

export function GraphPanel({ node, data, onClose, onJump }: Props) {
  const p = palette(node.kind);

  const rels: { edge: GraphEdge; outgoing: boolean; otherId: string }[] = [];
  for (const e of data.edges) {
    if (e.source === node.id) rels.push({ edge: e, outgoing: true, otherId: e.target });
    else if (e.target === node.id) rels.push({ edge: e, outgoing: false, otherId: e.source });
  }
  rels.sort((a, b) => (b.edge.weight || 1) - (a.edge.weight || 1));

  const groups: Record<string, typeof rels> = {};
  for (const r of rels) {
    (groups[r.edge.relation] ||= []).push(r);
  }

  return (
    <div class="absolute top-0 right-0 w-[300px] h-full z-20 flex flex-col bg-bg-card border-l border-border">
      <div class="flex items-start gap-2.5 px-4 py-3.5 border-b border-border-dim">
        <span
          class="w-7 h-7 rounded-md flex items-center justify-center text-note shrink-0 mt-0.5"
          style={{ background: `${p.base}14`, border: `1px solid ${p.base}30`, color: p.base }}
        >
          {p.sigil}
        </span>
        <div class="min-w-0 flex-1">
          <div class="text-micro" style={{ color: p.base }}>
            {p.label}
          </div>
          <h3 class="text-body font-medium text-text break-all leading-snug">{node.label}</h3>
          <div class="text-micro font-mono text-text-4 mt-1">
            权重 {(node.weight || 0).toFixed(1)} · {rels.length} 条关联
          </div>
        </div>
        <button
          onClick={onClose}
          aria-label="关闭"
          class="w-6 h-6 rounded text-text-4 hover:text-text hover:bg-white/[0.06] flex items-center justify-center transition-colors shrink-0"
        >
          ✕
        </button>
      </div>

      <div class="flex-1 overflow-y-auto scroll-thin px-2 py-2">
        {Object.entries(groups).map(([type, group]) => (
          <div key={type} class="mb-3">
            <div class="px-2 text-micro text-text-4 mb-1">
              {RELATION_LABELS[type] ?? type}
              <span class="font-mono ml-1.5">{group.length}</span>
            </div>
            {group.slice(0, 12).map((r, idx) => {
              const other = data.nodes.find((n) => n.id === r.otherId);
              const op = palette(other?.kind ?? '');
              return (
                <button
                  key={idx}
                  onClick={() => onJump(r.otherId)}
                  class="w-full flex items-center gap-2 px-2 py-1.5 rounded text-left hover:bg-bg-elev transition-colors"
                >
                  <span class="text-micro shrink-0" style={{ color: op.base }}>
                    {op.sigil}
                  </span>
                  <span class="flex-1 text-note text-text-2 truncate">{other?.label ?? r.otherId}</span>
                  <span class="text-micro font-mono text-text-4 shrink-0">
                    {r.outgoing ? '→' : '←'}
                  </span>
                </button>
              );
            })}
            {group.length > 12 && (
              <div class="px-2 text-micro text-text-4">还有 {group.length - 12} 条</div>
            )}
          </div>
        ))}

        {rels.length === 0 && (
          <p class="text-note text-text-4 text-center py-8">该节点暂无关联</p>
        )}
      </div>
    </div>
  );
}
