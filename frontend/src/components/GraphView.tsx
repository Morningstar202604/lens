/**
 * GraphView — production-grade knowledge visualisation.
 *
 * Layer A: Visual foundation  — dark void, radial-gradient orbs, SVG glow filters, curved flowing edges
 * Layer B: Information weight — radius / glow / font size proportional to node weight, pulse ring on high-weight nodes
 * Layer C: Interaction        — zoom (d3-zoom), hover-focus, click-select, drag-reposition
 * Layer D: Navigation         — search bar, kind-filter chips, physics toggle
 */
import { useEffect, useRef, useState, useCallback } from 'preact/hooks';
import { select } from 'd3-selection';
import 'd3-transition';
import { zoom as d3Zoom, zoomIdentity as d3ZoomIdentity } from 'd3-zoom';
import { forceSimulation, forceLink, forceManyBody, forceCenter, forceCollide, forceX, forceY } from 'd3-force';
import { drag as d3Drag } from 'd3-drag';
import type { GraphData, GraphNode, GraphEdge } from '../types';
import { api } from '../lib/api';
import { GraphPanel } from './GraphPanel';

// Internal simulation node (extends GraphNode with d3 position fields)
interface SimNode extends GraphNode { x?: number; y?: number; fx?: number | null; fy?: number | null; vx?: number; vy?: number; index?: number }
interface SimLink { source: SimNode | string; target: SimNode | string; relation: string; weight: number }

// ── Colour system ──────────────────────────────────────────────────────────
const PALETTE: Record<string, { base: string; glow: string; label: string; sigil: string }> = {
  character: { base: '#c9a961', glow: '#f0d48a', label: '角色', sigil: '◈' },
  location:  { base: '#22c55e', glow: '#86efac', label: '地点', sigil: '◉' },
  family:    { base: '#a78bfa', glow: '#ddd6fe', label: '家族', sigil: '◆' },
  event:     { base: '#ef4444', glow: '#fca5a5', label: '事件', sigil: '✧' },
  skill:     { base: '#f97316', glow: '#fed7aa', label: '技能', sigil: '⟐' },
  artifact:  { base: '#06b6d4', glow: '#a5f3fc', label: '神器', sigil: '◇' },
  theme:     { base: '#ec4899', glow: '#fbcfe8', label: '主题', sigil: '❖' },
  file:      { base: '#6366f5', glow: '#a5b4fc', label: '文件', sigil: '▤' },
  function:  { base: '#06b6d4', glow: '#a5f3fc', label: '函数', sigil: 'ƒ' },
  topic:     { base: '#f59b0b', glow: '#fde68a', label: '话题', sigil: '✦' },
};
const UNKNOWN = { base: '#6b7280', glow: '#9ca3af', label: '其他', sigil: '●' };

function palette(kind: string) { return PALETTE[kind] ?? UNKNOWN; }

interface Props {
  data: GraphData | null;
  onDataChange: (d: GraphData | null) => void;
  onSelectNode: (n: GraphNode | null) => void;
}

export function GraphView({ data, onDataChange, onSelectNode }: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const gRef = useRef<SVGGElement | null>(null);
  const simRef = useRef<any>(null);
  const zoomBehavior = useRef<any>(null);

  const [query, setQuery] = useState('');
  const [activeKinds, setActiveKinds] = useState<Set<string>>(new Set());
  const [physicsOn, setPhysicsOn] = useState(true);
  const [visible, setVisible] = useState<{ nodes: GraphNode[]; edges: GraphEdge[] }>({ nodes: [], edges: [] });
  const [selNode, setSelNode] = useState<GraphNode | null>(null);
  const [loading, setLoading] = useState(false);

  const visibleRef = useRef(visible);
  const activeKindsRef = useRef(activeKinds);
  const physicsRef = useRef(physicsOn);
  visibleRef.current = visible;
  activeKindsRef.current = activeKinds;
  physicsRef.current = physicsOn;

  // ── Fetch data ───────────────────────────────────────────────────────────
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api.getGlobalGraph().then(d => {
      if (cancelled) return;
      onDataChange(d);
      setActiveKinds(new Set(d.nodes.map(n => n.kind)));
      setVisible({ nodes: d.nodes, edges: d.edges });
      setLoading(false);
    }).catch(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  // ── Render ────────────────────────────────────────────────────────────
  const renderGraph = useCallback(() => {
    if (!svgRef.current || !gRef.current) return;
    const svg = select(svgRef.current);
    const g = select(gRef.current);
    const W = svgRef.current.clientWidth;
    const H = svgRef.current.clientHeight;

    const v = visibleRef.current;
    const kinds = activeKindsRef.current;
    const fNodes: SimNode[] = v.nodes.filter(n => kinds.has(n.kind)).map(n => ({ ...n }));
    const nodeIds = new Set(fNodes.map(n => n.id));
    const fLinks: SimLink[] = v.edges.filter(e => nodeIds.has(e.source) && nodeIds.has(e.target)).map(e => ({ source: e.source, target: e.target, relation: e.relation, weight: e.weight }));

    simRef.current?.stop();

    // Simulation
    const sim = forceSimulation(fNodes)
      .force('link', forceLink<SimNode, SimLink>(fLinks).id((d: SimNode) => d.id).distance(90).strength(0.35))
      .force('charge', forceManyBody().strength(-260))
      .force('center', forceCenter(W / 2, H / 2).strength(0.04))
      .force('collide', forceCollide<SimNode>().radius((d: SimNode) => nodeRadius(d) + 10))
      .force('x', forceX(W / 2).strength(0.02))
      .force('y', forceY(H / 2).strength(0.02));
    simRef.current = sim;
    if (!physicsRef.current) sim.stop();

    // Edge colour lookup with fallback
    const nodeKindMap = new Map(fNodes.map(n => [n.id, n.kind]));

    // ── Edges ─────────────────────────────────
    const edgeLayer = g.select<SVGGElement>('.edges');
    const edges = edgeLayer.selectAll<SVGPathElement, SimLink>('path').data(fLinks, (d: SimLink) => `${d.source}-${d.target}-${d.relation}`);
    edges.exit().remove();
    const edgesEnter = edges.enter().append('path').attr('fill', 'none').attr('stroke-linecap', 'round');
    edgesEnter.merge(edges as any)
      .attr('stroke', (d: SimLink) => palette(nodeKindMap.get(typeof d.source === 'string' ? d.source : d.source.id) ?? '').glow)
      .attr('stroke-opacity', (d: SimLink) => 0.12 + 0.15 * Math.min(d.weight / 10, 1))
      .attr('stroke-width', (d: SimLink) => 0.8 + Math.min(d.weight / 4, 2.5));

    // ── Edge labels ───────────────────────────
    const labelLayer = g.select<SVGGElement>('.edge-labels');
    const labels = labelLayer.selectAll<SVGTextElement, SimLink>('text').data(fLinks, (d: SimLink) => `${d.source}-${d.target}-${d.relation}`);
    labels.exit().remove();
    labels.enter().append('text')
      .attr('text-anchor', 'middle').attr('font-size', '9px').attr('fill', '#9ca3af')
      .attr('opacity', 0).attr('pointer-events', 'none')
      .merge(labels as any).text((d: SimLink) => d.relation);

    // ── Nodes ─────────────────────────────────
    const nodeLayer = g.select<SVGGElement>('.nodes');
    const nodesSel = nodeLayer.selectAll<SVGGElement, SimNode>('g.node-group').data(fNodes, (d: SimNode) => d.id);
    nodesSel.exit().remove();

    const nodesEnter = nodesSel.enter().append('g').attr('class', 'node-group').style('cursor', 'pointer');
    nodesEnter.filter((d: SimNode) => d.weight >= 8).append('circle').attr('class', 'pulse-ring').attr('fill', 'none').attr('stroke-width', 1.5).attr('opacity', 0.4);
    nodesEnter.append('circle').attr('class', 'orb').attr('stroke-width', 1.5).attr('stroke-opacity', 0.8);
    nodesEnter.append('text').attr('class', 'sigil').attr('text-anchor', 'middle').attr('dominant-baseline', 'central').attr('fill', 'rgba(0,0,0,0.4)').attr('pointer-events', 'none');
    nodesEnter.append('text').attr('class', 'node-label').attr('text-anchor', 'middle').attr('pointer-events', 'none');

    const allNodes = nodesEnter.merge(nodesSel as any);
    allNodes.select<SVGCircleElement>('.orb').attr('r', (d: SimNode) => nodeRadius(d)).attr('fill', (d: SimNode) => `url(#grad-${d.kind})`).attr('stroke', (d: SimNode) => palette(d.kind).base).attr('filter', (d: SimNode) => `url(#glow-${d.kind})`);
    allNodes.select<SVGCircleElement>('.pulse-ring').attr('r', (d: SimNode) => nodeRadius(d) + 6).attr('stroke', (d: SimNode) => palette(d.kind).glow);
    allNodes.select<SVGTextElement>('.sigil').attr('font-size', (d: SimNode) => nodeRadius(d) * 0.9).text((d: SimNode) => palette(d.kind).sigil);
    allNodes.select<SVGTextElement>('.node-label').attr('dy', (d: SimNode) => nodeRadius(d) + 14).attr('font-size', (d: SimNode) => d.weight > 7 ? 13 : d.weight > 4 ? 11 : 9).attr('font-weight', (d: SimNode) => d.weight > 7 ? 600 : 400).attr('fill', '#d1d5db').attr('opacity', 0.75).text((d: SimNode) => d.label.length > 6 ? d.label.slice(0, 5) + '…' : d.label);

    // Node interactions
    allNodes
      .on('mouseenter', function (_e: MouseEvent, d: SimNode) { hlNode(d); })
      .on('mouseleave', function () { clearHL(); })
      .on('click', function (_e: MouseEvent, d: SimNode) { doSelect(d); })
      .on('dblclick', function (_e: MouseEvent, d: SimNode) { centerNode(d); })
      .call(drag(sim));

    sim.on('tick', () => {
      edgeLayer.selectAll<SVGPathElement, SimLink>('path').attr('d', (d: SimLink) => {
        const s = d.source as SimNode, t = d.target as SimNode;
        const dx = (t.x ?? 0) - (s.x ?? 0), dy = (t.y ?? 0) - (s.y ?? 0);
        const dr = Math.sqrt(dx*dx + dy*dy) * 2;
        return `M${s.x ?? 0},${s.y ?? 0}A${dr},${dr} 0 0,1 ${t.x ?? 0},${t.y ?? 0}`;
      });
      labelLayer.selectAll<SVGTextElement, SimLink>('text')
        .attr('x', (d: SimLink) => (((d.source as SimNode).x ?? 0) + ((d.target as SimNode).x ?? 0)) / 2)
        .attr('y', (d: SimLink) => (((d.source as SimNode).y ?? 0) + ((d.target as SimNode).y ?? 0)) / 2 - 4);
      nodeLayer.selectAll<SVGGElement, SimNode>('g').attr('transform', (d: SimNode) => `translate(${d.x ?? 0},${d.y ?? 0})`);
    });

    function hlNode(n: SimNode) {
      const nbr = new Set([n.id]);
      fLinks.forEach(l => {
        const sid = typeof l.source === 'string' ? l.source : l.source.id;
        const tid = typeof l.target === 'string' ? l.target : l.target.id;
        if (sid === n.id) nbr.add(tid);
        if (tid === n.id) nbr.add(sid);
      });
      nodeLayer.selectAll<SVGGElement, SimNode>('g').style('opacity', (d: SimNode) => nbr.has(d.id) ? 1 : 0.12);
      edgeLayer.selectAll<SVGPathElement, SimLink>('path').style('opacity', (d: SimLink) => {
        const sid = typeof d.source === 'string' ? d.source : d.source.id;
        const tid = typeof d.target === 'string' ? d.target : d.target.id;
        return (sid === n.id || tid === n.id) ? 0.7 : 0.03;
      }).attr('stroke-width', (d: SimLink) => {
        const sid = typeof d.source === 'string' ? d.source : d.source.id;
        const tid = typeof d.target === 'string' ? d.target : d.target.id;
        return (sid === n.id || tid === n.id) ? 2 + d.weight * 0.5 : 0.5;
      });
      labelLayer.selectAll<SVGTextElement, SimLink>('text').attr('opacity', (d: SimLink) => {
        const sid = typeof d.source === 'string' ? d.source : d.source.id;
        const tid = typeof d.target === 'string' ? d.target : d.target.id;
        return (sid === n.id || tid === n.id) ? 0.8 : 0;
      });
    }

    function clearHL() {
      nodeLayer.selectAll<SVGGElement, SimNode>('g').style('opacity', 1);
      edgeLayer.selectAll<SVGPathElement, SimLink>('path').style('opacity', null).attr('stroke-width', null);
      labelLayer.selectAll<SVGTextElement, SimLink>('text').attr('opacity', 0);
    }

    function doSelect(n: SimNode) {
      const gn: GraphNode = { id: n.id, label: n.label, kind: n.kind, weight: n.weight };
      setSelNode(gn);
      onSelectNode(gn);
      hlNode(n);
    }

    function centerNode(n: SimNode) {
      if (n.x == null || n.y == null || !svgRef.current || !zoomBehavior.current) return;
      const w = svgRef.current.clientWidth, h = svgRef.current.clientHeight;
      const t = d3ZoomIdentity.translate(w / 2, h / 2).scale(1.4).translate(-(n.x ?? 0), -(n.y ?? 0));
      zoomBehavior.current.transform(select(svgRef.current), t);
    }

    function drag(sim: any) {
      return d3Drag<SVGGElement, SimNode>()
        .on('start', (event: any, d: SimNode) => { if (!event.active) sim.alphaTarget(0.3).restart(); d.fx = d.x; d.fy = d.y; })
        .on('drag', (event: any, d: SimNode) => { d.fx = event.x; d.fy = event.y; })
        .on('end', (event: any, d: SimNode) => { if (!event.active) sim.alphaTarget(0); d.fx = null; d.fy = null; });
    }
  }, [visible, activeKinds]);

  // ── Initialize SVG ───────────────────────────────────────────────────────
  useEffect(() => {
    if (!svgRef.current || !wrapRef.current) return;
    const svg = select(svgRef.current);
    svg.selectAll('*').remove();

    const defsEl = svg.append('defs');
    Object.entries(PALETTE).forEach(([kind, p]) => {
      const grad = defsEl.append('radialGradient').attr('id', `grad-${kind}`).attr('cx', '35%').attr('cy', '30%');
      grad.append('stop').attr('offset', '0%').attr('stop-color', p.glow);
      grad.append('stop').attr('offset', '100%').attr('stop-color', p.base);
      const f = defsEl.append('filter').attr('id', `glow-${kind}`).attr('x', '-50%').attr('y', '-50%').attr('width', '200%').attr('height', '200%');
      f.append('feGaussianBlur').attr('stdDeviation', '4').attr('result', 'blur');
      const m = f.append('feMerge');
      m.append('feMergeNode').attr('in', 'blur');
      m.append('feMergeNode').attr('in', 'SourceGraphic');
    });

    defsEl.append('marker').attr('id', 'arrow').attr('viewBox', '0 -5 10 10').attr('refX', 22).attr('refY', 0)
      .attr('markerWidth', 7).attr('markerHeight', 7).attr('orient', 'auto')
      .append('path').attr('d', 'M0,-4L8,0L0,4').attr('fill', 'rgba(255,255,255,.25)');

    const g = svg.append('g').attr('class', 'graph-root');
    gRef.current = g.node();
    g.append('g').attr('class', 'edges');
    g.append('g').attr('class', 'edge-labels');
    g.append('g').attr('class', 'nodes');

    const z = d3Zoom<SVGSVGElement, unknown>().scaleExtent([0.15, 4]).on('zoom', (e) => { g.attr('transform', e.transform.toString()); });
    zoomBehavior.current = z;
    svg.call(z);
    svg.on('click', (e) => { if ((e.target as Element).tagName === 'svg') { setSelNode(null); onSelectNode(null); } });

    renderGraph();
    return () => { simRef.current?.stop(); };
  }, []);

  useEffect(() => { renderGraph(); }, [visible, activeKinds, renderGraph]);

  // ── Search ──────────────────────────────────────────────────────────────
  function handleSearch(q: string) {
    setQuery(q);
    if (!q.trim() || !gRef.current) { select(gRef.current).selectAll('.nodes g').style('opacity', 1); return; }
    const v = visibleRef.current;
    const matched = v.nodes.filter(n => n.label.toLowerCase().includes(q.toLowerCase()) || n.id.toLowerCase().includes(q.toLowerCase()));
    if (matched.length === 1 && gRef.current && svgRef.current && zoomBehavior.current) {
      const g = select(gRef.current);
      g.selectAll<SVGGElement, SimNode>('.nodes g').style('opacity', (d: SimNode) => d.id === matched[0].id ? 1 : 0.08);
    }
  }

  function toggleKind(kind: string) {
    setActiveKinds(prev => { const n = new Set(prev); if (n.has(kind)) n.delete(kind); else n.add(kind); return n; });
  }

  function togglePhysics() {
    setPhysicsOn(prev => { const n = !prev; if (n) simRef.current?.alphaTarget(0.3).restart(); else simRef.current?.stop(); return n; });
  }

  function resetView() {
    if (!svgRef.current || !zoomBehavior.current) return;
    zoomBehavior.current.transform(select(svgRef.current), d3ZoomIdentity);
  }
  function zoomIn() { if (svgRef.current && zoomBehavior.current) zoomBehavior.current.scaleBy(select(svgRef.current), 1.5); }
  function zoomOut() { if (svgRef.current && zoomBehavior.current) zoomBehavior.current.scaleBy(select(svgRef.current), 0.67); }

  const allKinds = new Set(data?.nodes.map(n => n.kind) ?? []);

  return (
    <div class="flex flex-col h-[calc(100vh-6rem)]">
      {/* Top bar */}
      <div class="flex items-center gap-4 px-4 py-3 border-b border-border">
        <div class="relative flex-1 max-w-md">
          <span class="absolute left-3 top-1/2 -translate-y-1/2 text-text-3 text-sm">⌖</span>
          <input value={query} onInput={e => handleSearch((e.target as HTMLInputElement).value)} placeholder="Search entities..."
            class="w-full pl-9 pr-3 py-1.5 text-sm bg-bg border border-border rounded-full text-text placeholder:text-text-4 focus:outline-none focus:border-primary/50 transition-colors" />
        </div>
        <span class="text-xs text-text-3 tabular-nums">{visible.nodes.length} nodes</span>
        <span class="text-xs text-text-3 tabular-nums">{visible.edges.length} edges</span>
      </div>

      <div ref={wrapRef} class="flex-1 relative overflow-hidden" style={{ background: 'radial-gradient(ellipse at 50% 40%, #0f1428 0%, #080a14 70%, #060710 100%)' }}>
        <div class="absolute inset-0 pointer-events-none opacity-[0.02]" style={{ backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")` }} />
        <svg ref={svgRef} class="absolute inset-0 w-full h-full" />

        {loading && (
          <div class="absolute inset-0 flex flex-col items-center justify-center z-10">
            <div class="w-10 h-10 border-2 border-primary/30 border-t-primary rounded-full animate-spin" />
            <p class="mt-3 text-xs text-text-3 tracking-widest uppercase">Invoking Graph</p>
          </div>
        )}
        {!loading && visible.nodes.length < 2 && (
          <div class="absolute inset-0 flex flex-col items-center justify-center z-10 text-text-4">
            <div class="text-4xl mb-3 opacity-30">✦</div>
            <div class="text-sm">Insufficient data</div>
          </div>
        )}

        {/* Bottom-left controls */}
        <div class="absolute bottom-4 left-4 flex flex-col gap-2 z-10">
          <button onClick={zoomIn} class="w-9 h-9 rounded-lg border border-border/40 bg-bg/80 backdrop-blur-sm text-text-3 hover:text-text hover:border-primary/40 flex items-center justify-center text-base transition-all">⊕</button>
          <button onClick={zoomOut} class="w-9 h-9 rounded-lg border border-border/40 bg-bg/80 backdrop-blur-sm text-text-3 hover:text-text hover:border-primary/40 flex items-center justify-center text-base transition-all">⊖</button>
          <button onClick={resetView} class="w-9 h-9 rounded-lg border border-border/40 bg-bg/80 backdrop-blur-sm text-text-3 hover:text-text hover:border-primary/40 flex items-center justify-center text-base transition-all">⟲</button>
          <button onClick={togglePhysics} class="w-9 h-9 rounded-lg border border-border/40 bg-bg/80 backdrop-blur-sm text-text-3 hover:text-text hover:border-primary/40 flex items-center justify-center text-base transition-all">
            <span class={physicsOn ? 'text-primary' : 'text-text-4'}>⚡</span>
          </button>
        </div>

        {/* Bottom-right kind filter */}
        <div class="absolute bottom-4 right-[340px] flex flex-col items-end gap-1.5 z-10">
          {Array.from(allKinds).map(k => {
            const p = palette(k);
            const active = activeKinds.has(k);
            return (
              <button key={k} onClick={() => toggleKind(k)}
                class="flex items-center gap-2 px-3 py-1 rounded-full text-xs transition-all border"
                style={{ background: active ? `${p.base}22` : 'rgba(15,18,32,0.7)', borderColor: active ? `${p.base}55` : 'rgba(255,255,255,0.06)', color: active ? p.glow : '#6b7280' }}>
                <span>{p.sigil}</span><span>{p.label}</span>
              </button>
            );
          })}
        </div>

        {selNode && (
          <GraphPanel node={selNode} data={data!}
            onClose={() => { setSelNode(null); onSelectNode(null); if (gRef.current) { select(gRef.current).selectAll('.nodes g').style('opacity', 1).selectAll('.edges path').style('opacity', null).attr('stroke-width', null); } }}
            onJump={(id) => { const n = data?.nodes.find(x => x.id === id); if (n) { setSelNode(n); onSelectNode(n); if (svgRef.current && zoomBehavior.current) centerNodeById(n.id); } }}
          />
        )}
      </div>
    </div>
  );
}

function nodeRadius(d: SimNode): number { return Math.max(7, Math.min(24, 5 + d.weight * 1.6)); }

function centerNodeById(id: string) { /* reposition on jump */ }
