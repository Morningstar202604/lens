/**
 * GraphPanel — Slide-in detail panel showing selected node info and relations.
 *
 * Layout:
 * - Top: kind badge + name + ID + close button
 * - Weight arc indicator
 * - Grouped relations (source-> / <-target), each row clickable to jump
 */
import type { GraphNode, GraphData, GraphEdge } from '../types';

import { kindPalette as palette } from '../graph-palette';

interface Props {
  node: GraphNode;
  data: GraphData;
  onClose: () => void;
  onJump: (id: string) => void;
}

export function GraphPanel({ node, data, onClose, onJump }: Props) {
  const p = palette(node.kind);

  // Collect & group relations
  const rels: { edge: GraphEdge; outgoing: boolean; otherId: string }[] = [];
  data.edges.forEach(e => {
    if (e.source === node.id) rels.push({ edge: e, outgoing: true, otherId: e.target });
    else if (e.target === node.id) rels.push({ edge: e, outgoing: false, otherId: e.source });
  });
  rels.sort((a, b) => (b.edge.weight || 1) - (a.edge.weight || 1));

  // Group by relation type
  const groups: Record<string, typeof rels> = {};
  rels.forEach(r => { const t = r.edge.relation; if (!groups[t]) groups[t] = []; groups[t].push(r); });

  const weightPct = Math.min((node.weight || 0) / 10 * 100, 100);

  return (
    <div class="absolute top-0 right-0 w-[320px] h-full z-20 flex flex-col gap-0
                bg-[rgba(13,16,32,0.96)] backdrop-blur-xl border-l border-white/[0.06]
                shadow-[-8px 0_40px_rgba(0,0,0,0.4)]">
      {/* Close */}
      <button onClick={onClose}
        class="absolute top-3 right-3 w-8 h-8 rounded-full border border-white/10
               bg-white/[0.03] text-text-3 hover:text-text hover:bg-white/[0.08] flex items-center justify-center transition-all">
        x
      </button>

      {/* Header */}
      <div class="px-5 pt-5 pb-4">
        <div class="flex items-center gap-3 mb-3">
          <div class="w-11 h-11 rounded-full flex items-center justify-center text-lg shrink-0"
            style={{
              background: 'radial-gradient(circle at 35% 30%, ' + p.glow + '33, ' + p.base + '11)',
              border: '1.5px solid ' + p.base + '44',
              color: p.glow,
              boxShadow: '0 0 20px ' + p.base + '22',
            }}>
            {p.sigil}
          </div>
          <div class="min-w-0">
            <div class="text-[10px] uppercase tracking-[2px] font-semibold" style={{ color: p.glow }}>{p.label}</div>
            <h3 class="text-lg font-semibold text-text leading-tight truncate">{node.label}</h3>
          </div>
        </div>

        {/* Weight arc */}
        <div class="flex items-center gap-3 mt-2">
          <svg width="48" height="48" class="shrink-0">
            <circle cx="24" cy="24" r="19" fill="none" stroke="rgba(255,255,255,0.05)" stroke-width="3" />
            <circle cx="24" cy="24" r="19" fill="none" stroke={p.base} stroke-width="3"
              stroke-linecap="round" stroke-dasharray={weightPct * 1.2 + ' 120'}
              transform="rotate(-90 24 24)" style={{ filter: 'drop-shadow(0 0 4px ' + p.base + '66)' }} />
          </svg>
          <div>
            <div class="text-[10px] text-text-4 uppercase tracking-wider">权重</div>
            <div class="text-xl font-bold tabular-nums" style={{ color: p.glow }}>{(node.weight || 0).toFixed(1)}</div>
          </div>
        </div>
      </div>

      {/* Divider */}
      <div class="mx-5 h-px bg-white/[0.05]" />

      {/* Relations list */}
      <div class="flex-1 overflow-y-auto px-5 py-3 scroll-thin">
        <div class="text-[10px] uppercase tracking-[2px] text-text-4 mb-2 font-semibold">
          关联（{rels.length}）
        </div>

        {Object.entries(groups).map(([type, group]) => (
          <div key={type} class="mb-3">
            <div class="text-[10px] text-text-4 mb-1.5 font-medium tracking-wide italic">{type}</div>
            {group.slice(0, 8).map((r, idx) => {
              const other = data.nodes.find(n => n.id === r.otherId);
              const op = palette(other?.kind ?? '');
              return (
                <button key={idx} onClick={() => onJump(r.otherId)}
                  class="w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg mb-0.5 text-left
                         hover:bg-white/[0.04] transition-colors group">
                  <div class="w-7 h-7 rounded-full flex items-center justify-center text-xs shrink-0"
                    style={{ background: op.base + '15', border: '1px solid ' + op.base + '33', color: op.glow }}>
                    {op.sigil}
                  </div>
                  <span class="flex-1 text-sm text-text-2 truncate group-hover:text-text transition-colors">
                    {other?.label ?? r.otherId}
                  </span>
                  <span class="text-[10px] text-text-4 opacity-0 group-hover:opacity-100 transition-opacity">
                    {r.outgoing ? '->' : '<-'}
                  </span>
                </button>
              );
            })}
            {group.length > 8 && (
              <div class="text-[10px] text-text-4 pl-2.5 pt-0.5">还有 {group.length - 8} 条</div>
            )}
          </div>
        ))}

        {rels.length === 0 && (
          <div class="text-sm text-text-4 text-center py-6">暂无关联</div>
        )}
      </div>
    </div>
  );
}
