/**
 * GraphView — 知识图谱。
 *
 * 视觉编码（三类实体各有独立形状，缩到最小也能分辨类别）：
 *   文件   ▢ 圆角方形
 *   函数   ● 实心圆点
 *   话题   ⬢ 六边形
 *
 * 尺寸用对数刻度映射权重，还原 0.5~190 的真实跨度（线性刻度下 90% 节点会
 * 撞到尺寸上限、变得一样大，看起来就"只有一种样式"）。
 *
 * 密度分层（主干/标准/全部）控制实体与关系数量：默认"标准"，
 * 避免一打开就是几百个点加几千条边的一团麻。
 *
 * 交互：悬停高亮邻域；点击固定聚焦（只留该节点邻域）；拖拽、缩放、双击居中。
 */
import { useEffect, useRef, useState, useCallback } from 'preact/hooks';
import { select } from 'd3-selection';
import 'd3-transition';
import { zoom as d3Zoom, zoomIdentity as d3ZoomIdentity } from 'd3-zoom';
import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCenter,
  forceCollide,
  forceX,
  forceY,
} from 'd3-force';
import { drag as d3Drag } from 'd3-drag';
import type { GraphData, GraphNode, GraphEdge } from '../types';
import { api } from '../lib/api';
import { GraphPanel } from './GraphPanel';
import { kindPalette as palette } from '../graph-palette';

interface SimNode extends GraphNode {
  x?: number;
  y?: number;
  fx?: number | null;
  fy?: number | null;
  vx?: number;
  vy?: number;
  r?: number;
}
interface SimLink {
  source: SimNode | string;
  target: SimNode | string;
  relation: string;
  weight: number;
}

interface Props {
  data: GraphData | null;
  onDataChange: (d: GraphData | null) => void;
  onSelectNode: (n: GraphNode | null) => void;
}

type Density = 'core' | 'standard' | 'all';

const DENSITY_CONFIG: Record<Density, { nodes: number; edges: number; label: string; hint: string }> = {
  core: { nodes: 60, edges: 130, label: '主干', hint: '只看权重最高的 60 个实体' },
  standard: { nodes: 150, edges: 400, label: '标准', hint: '权重最高的 150 个实体' },
  all: { nodes: Infinity, edges: 1100, label: '全部', hint: '显示全部实体（会比较密）' },
};

/** 半径范围（对数刻度）。 */
const R_MIN = 4;
const R_MAX = 26;

/** 权重 → 半径。对数刻度，避免大权重把其余节点压成同一尺寸。 */
function radiusFor(weight: number, maxWeight: number): number {
  const w = Math.max(weight, 0.1);
  const denom = Math.log1p(Math.max(maxWeight, 1));
  if (denom <= 0) return R_MIN;
  return R_MIN + (R_MAX - R_MIN) * (Math.log1p(w) / denom);
}

type EdgeClass = 'structural' | 'topic' | 'cooccur';

function edgeClass(relation: string): EdgeClass {
  if (relation === 'references' || relation === 'contains') return 'structural';
  if (relation === 'related_to') return 'topic';
  return 'cooccur';
}

/** 标签压缩：保留有信息量的尾部（文件名/函数名）。 */
/** 标签：优先显示文件名/函数名尾部（有区分度的那部分），过长才截断。
 *  图谱里节点密集，标签越短越不容易互相压字。 */
function shortLabel(label: string, max = 20): string {
  if (label.length <= max) return label;
  const parts = label.split('/');
  const tail = parts[parts.length - 1];
  if (tail.length <= max - 1) return `…/${tail}`;
  return `${tail.slice(0, max - 1)}…`;
}

export function GraphView({ data, onDataChange, onSelectNode }: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const gRef = useRef<SVGGElement | null>(null);
  const simRef = useRef<ReturnType<typeof forceSimulation<SimNode>> | null>(null);
  const centerRef = useRef<((id: string) => void) | null>(null);
  const zoomBehavior = useRef<any>(null);

  const [query, setQuery] = useState('');
  const [density, setDensity] = useState<Density>('core');
  const [activeKinds, setActiveKinds] = useState<Set<string>>(new Set());
  const [physicsOn, setPhysicsOn] = useState(true);
  const [loading, setLoading] = useState(false);
  const [selNode, setSelNode] = useState<GraphNode | null>(null);
  const [hoverNode, setHoverNode] = useState<GraphNode | null>(null);
  const [visible, setVisible] = useState<{ nodes: GraphNode[]; edges: GraphEdge[] }>({
    nodes: [],
    edges: [],
  });

  const rawRef = useRef<GraphData>({ nodes: [], edges: [] });
  const densityRef = useRef(density);
  const kindsRef = useRef(activeKinds);
  const physicsRef = useRef(physicsOn);
  const focusRef = useRef<string | null>(null);
  densityRef.current = density;
  kindsRef.current = activeKinds;
  physicsRef.current = physicsOn;

  // ── 取数 ─────────────────────────────────────────────────────
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .getGlobalGraph()
      .then((d) => {
        if (cancelled) return;
        rawRef.current = d;
        onDataChange(d);
        setActiveKinds(new Set(d.nodes.map((n) => n.kind)));
        setLoading(false);
      })
      .catch(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, []);

  /** 按密度与类别过滤出当前屏幕上的节点与边。 */
  const filterByDensity = useCallback(
    (d: Density, kinds: Set<string>): { nodes: GraphNode[]; edges: GraphEdge[] } => {
      const raw = rawRef.current;
      const cfg = DENSITY_CONFIG[d];

      const pool = raw.nodes.filter((n) => kinds.has(n.kind));
      const sorted = [...pool].sort((a, b) => b.weight - a.weight);
      const nodes = Number.isFinite(cfg.nodes) ? sorted.slice(0, cfg.nodes) : sorted;
      const ids = new Set(nodes.map((n) => n.id));

      // 共现边是弱关系（同一轮里一起出现），优先级压低，避免挤掉结构性关系
      const edges = raw.edges
        .filter((e) => ids.has(e.source) && ids.has(e.target))
        .map((e) => ({ e, rank: edgeClass(e.relation) === 'cooccur' ? e.weight * 0.25 : e.weight }))
        .sort((a, b) => b.rank - a.rank)
        .slice(0, cfg.edges)
        .map((s) => s.e);

      return { nodes, edges };
    },
    [],
  );

  useEffect(() => {
    if (loading) return;
    setVisible(filterByDensity(density, activeKinds));
  }, [density, activeKinds, loading, filterByDensity]);

  // ── 渲染 ─────────────────────────────────────────────────────
  const renderGraph = useCallback(() => {
    if (!svgRef.current || !gRef.current) return;
    const svg = select(svgRef.current);
    const g = select(gRef.current);
    const W = svgRef.current.clientWidth;
    const H = svgRef.current.clientHeight;

    const v = visible;
    if (v.nodes.length === 0) return;

    const maxW = Math.max(...v.nodes.map((n) => n.weight), 1);
    const labelCut = labelThreshold(v.nodes);

    const fNodes: SimNode[] = v.nodes.map((n) => ({ ...n, r: radiusFor(n.weight, maxW) }));
    const byId = new Map(fNodes.map((n) => [n.id, n]));
    const fLinks: SimLink[] = v.edges
      .filter((e) => byId.has(e.source) && byId.has(e.target))
      .map((e) => ({ source: e.source, target: e.target, relation: e.relation, weight: e.weight }));

    simRef.current?.stop();

    // 斥力与碰撞半径按画布面积缩放：小窗口里塞太多节点必然叠字。
    const area = Math.max(W * H, 1);
    const spread = Math.min(1.8, Math.max(0.7, area / (1_100_000 * Math.max(fNodes.length / 120, 0.6))));
    const sim = forceSimulation(fNodes)
      .force('link', forceLink<SimNode, SimLink>(fLinks).id((d) => d.id).distance(78 * spread).strength(0.22))
      .force('charge', forceManyBody().strength((d) => -90 * spread - ((d as SimNode).r ?? 4) * 8))
      .force('center', forceCenter(W / 2, H / 2).strength(0.035))
      // 碰撞半径含标签高度，避免标签互相压字
      .force('collide', forceCollide<SimNode>().radius((d) => (d.r ?? 4) + 16).strength(0.9))
      .force('x', forceX(W / 2).strength(0.012))
      .force('y', forceY(H / 2).strength(0.012));
    simRef.current = sim;
    if (physicsRef.current) sim.alpha(0.85);
    else sim.stop();

    // ── 边：三类关系分别绘制，样式不同 ──
    const edgeLayer = g.select<SVGGElement>('.edges');
    const classes: EdgeClass[] = ['cooccur', 'topic', 'structural'];
    for (const cls of classes) {
      const dataForCls = fLinks.filter((l) => edgeClass(l.relation) === cls);
      const merged = edgeLayer
        .selectAll<SVGPathElement, SimLink>(`path.edge-${cls}`)
        .data(dataForCls, (d: SimLink) => `${idOf(d.source)}→${idOf(d.target)}`)
        .join(
          (enter) =>
            enter
              .append('path')
              .attr('class', `edge-${cls}`)
              .attr('fill', 'none')
              .attr('stroke-linecap', 'round')
              // 边不参与命中：否则会盖住节点标签，点节点变成点边
              .attr('pointer-events', 'none'),
          (update) => update,
          (exit) => exit.remove(),
        );
      merged
        .attr('stroke', EDGE_STYLE[cls].stroke)
        .attr('stroke-width', (d: SimLink) => EDGE_STYLE[cls].width(d.weight))
        .attr('stroke-opacity', (d: SimLink) => EDGE_STYLE[cls].opacity(d.weight))
        .attr('stroke-dasharray', EDGE_STYLE[cls].dash);
    }

    // ── 节点 ──
    const nodeLayer = g.select<SVGGElement>('.nodes');
    const nodesSel = nodeLayer
      .selectAll<SVGGElement, SimNode>('g.node-group')
      .data(fNodes, (d: SimNode) => d.id)
      .join((enter) => {
        const grp = enter.append('g').attr('class', 'node-group').style('cursor', 'pointer');
        grp.append('circle').attr('class', 'hit').attr('fill', 'transparent');
        grp.each(function (d) {
          const node = select(this);
          const p = palette(d.kind);
          if (d.kind === 'file') {
            node
              .append('rect')
              .attr('class', 'shape')
              .attr('stroke', p.base)
              .attr('stroke-width', 1.4)
              .attr('fill', p.base)
              .attr('fill-opacity', 0.16);
          } else if (d.kind === 'topic') {
            node
              .append('polygon')
              .attr('class', 'shape')
              .attr('stroke', p.base)
              .attr('stroke-width', 1.4)
              .attr('fill', p.base)
              .attr('fill-opacity', 0.22);
          } else {
            node
              .append('circle')
              .attr('class', 'shape')
              .attr('stroke', p.base)
              .attr('stroke-width', 1.2)
              .attr('fill', p.base)
              .attr('fill-opacity', 0.32);
          }
        });
        grp
          .append('text')
          .attr('class', 'node-label')
          .attr('text-anchor', 'middle')
          .attr('pointer-events', 'none');
        return grp;
      });

    // 形状几何
    nodesSel.select<SVGRectElement>('rect.shape').each(function (d) {
      const r = d.r ?? 4;
      const s = r * 1.8;
      select(this)
        .attr('x', -s / 2)
        .attr('y', -s / 2)
        .attr('width', s)
        .attr('height', s)
        .attr('rx', Math.max(2, s * 0.2));
    });
    nodesSel.select<SVGPolygonElement>('polygon.shape').attr('points', (d) => hexPoints((d.r ?? 4) * 1.15));
    nodesSel.select<SVGCircleElement>('circle.shape').attr('r', (d) => d.r ?? 4);
    nodesSel.select<SVGCircleElement>('circle.hit').attr('r', (d) => (d.r ?? 4) + 12);

    // ── 标签：只有高权重节点常驻显示，其余靠悬停/聚焦 ──
    nodesSel
      .select<SVGTextElement>('text.node-label')
      .attr('dy', (d) => (d.r ?? 4) + 12)
      .attr('font-size', (d) => ((d.r ?? 4) > 16 ? 11 : 10))
      .attr('font-weight', (d) => ((d.r ?? 4) > 16 ? 600 : 400))
      .attr('fill', (d) => palette(d.kind).glow)
      .attr('paint-order', 'stroke')
      .attr('stroke', 'rgba(11,13,18,.92)')
      .attr('stroke-width', 3)
      .attr('stroke-linejoin', 'round')
      .text((d) => shortLabel(d.label));

    /** 标签防叠：按权重从高到低贪心放置，冲突的标签隐藏。
     *  中心枢纽节点的标签必然最多，不处理就会叠成一坨黑块。 */
    function declutterLabels() {
      const placed: { x1: number; x2: number; y1: number; y2: number }[] = [];
      const ordered = [...fNodes].sort((a, b) => b.weight - a.weight);
      const hide = new Set<string>();
      for (const n of ordered) {
        if (n.x == null || n.y == null) continue;
        const label = shortLabel(n.label);
        const w = label.length * 6.2; // 估算文本宽度（10~11px 字号）
        const y = (n.y ?? 0) + (n.r ?? 4) + 6;
        const box = { x1: (n.x ?? 0) - w / 2, x2: (n.x ?? 0) + w / 2, y1: y, y2: y + 12 };
        const overlaps = placed.some(
          (p) => !(box.x2 < p.x1 || box.x1 > p.x2 || box.y2 < p.y1 || box.y1 > p.y2),
        );
        if (overlaps) hide.add(n.id);
        else placed.push(box);
      }
      nodeLayer
        .selectAll<SVGTextElement, SimNode>('text.node-label')
        .classed('label-dim', (d: SimNode | undefined) => !d || hide.has(d.id));
    }

    // ── 聚焦 ──
    function applyFocus(focusId: string | null, sticky: boolean) {
      const nLayer = nodeLayer.selectAll<SVGGElement, SimNode>('g.node-group');
      const eLayer = edgeLayer.selectAll<SVGPathElement, SimLink>('path');
      const lLayer = nodeLayer.selectAll<SVGTextElement, SimNode>('text.node-label');

      if (!focusId) {
        nLayer.style('opacity', 1);
        eLayer.style('opacity', null).attr('stroke-width', null);
        // 先按阈值粗筛；坐标就绪后由 declutterLabels 精确去叠
        lLayer.classed('label-dim', (d: SimNode | undefined) => (d?.weight ?? 0) < labelCut);
        if (fNodes.every((n) => n.x != null)) declutterLabels();
        return;
      }

      const nbr = new Set<string>([focusId]);
      for (const l of fLinks) {
        const s = idOf(l.source);
        const t = idOf(l.target);
        if (s === focusId) nbr.add(t);
        if (t === focusId) nbr.add(s);
      }
      nLayer.style('opacity', (d: SimNode) => (nbr.has(d.id) ? 1 : sticky ? 0.06 : 0.12));
      eLayer
        .style('opacity', (d: SimLink) =>
          idOf(d.source) === focusId || idOf(d.target) === focusId ? 0.95 : sticky ? 0.03 : 0.06,
        )
        .attr('stroke-width', (d: SimLink) =>
          idOf(d.source) === focusId || idOf(d.target) === focusId
            ? 1.4 + Math.min(d.weight, 12) * 0.22
            : null,
        );
      // 邻域内的标签全部显示——这是聚焦真正的价值
      lLayer.classed('label-dim', (d: SimNode | undefined) => !!d && !nbr.has(d.id) && d.weight < labelCut);
    }

    nodesSel
      .on('mouseenter', function (_e: MouseEvent, d: SimNode) {
        setHoverNode(d);
        if (!focusRef.current) applyFocus(d.id, false);
      })
      .on('mouseleave', function () {
        setHoverNode(null);
        applyFocus(focusRef.current, !!focusRef.current);
      })
      .on('click', function (e: MouseEvent, d: SimNode) {
        e.stopPropagation();
        const turningOn = focusRef.current !== d.id;
        focusRef.current = turningOn ? d.id : null;
        setSelNode(turningOn ? d : null);
        onSelectNode(turningOn ? d : null);
        applyFocus(focusRef.current, !!focusRef.current);
      })
      .on('dblclick', function (_e: MouseEvent, d: SimNode) {
        centerNode(d);
      })
      .call(drag(sim));

    centerRef.current = (id: string) => {
      const node = fNodes.find((x) => x.id === id);
      if (node) centerNode(node);
    };
    function centerNode(n: SimNode) {
      if (n.x == null || n.y == null || !svgRef.current || !zoomBehavior.current) return;
      const w = svgRef.current.clientWidth;
      const h = svgRef.current.clientHeight;
      const t = d3ZoomIdentity.translate(w / 2, h / 2).scale(1.5).translate(-(n.x ?? 0), -(n.y ?? 0));
      zoomBehavior.current.transform(select(svgRef.current), t);
    }

    function drag(s: any) {
      return d3Drag<SVGGElement, SimNode>()
        .on('start', (event: any, d: SimNode) => {
          if (!event.active) s.alphaTarget(0.25).restart();
          d.fx = d.x;
          d.fy = d.y;
        })
        .on('drag', (event: any, d: SimNode) => {
          d.fx = event.x;
          d.fy = event.y;
        })
        .on('end', (event: any, d: SimNode) => {
          if (!event.active) s.alphaTarget(0);
          d.fx = null;
          d.fy = null;
        });
    }

    // 布局稳定后做一次标签去叠（此时坐标已定）
    sim.on('end', () => {
      if (!focusRef.current) declutterLabels();
    });

    sim.on('tick', () => {
      edgeLayer.selectAll<SVGPathElement, SimLink>('path').attr('d', (d: SimLink) => {
        const s = d.source as SimNode;
        const t = d.target as SimNode;
        return `M${s.x ?? 0},${s.y ?? 0}L${t.x ?? 0},${t.y ?? 0}`;
      });
      nodeLayer
        .selectAll<SVGGElement, SimNode>('g.node-group')
        .attr('transform', (d: SimNode) => `translate(${d.x ?? 0},${d.y ?? 0})`);
    });

    applyFocus(focusRef.current, !!focusRef.current);
  }, [visible]);

  // ── 初始化画布 ──
  useEffect(() => {
    if (!svgRef.current || !wrapRef.current) return;
    const svg = select(svgRef.current);
    svg.selectAll('*').remove();

    const g = svg.append('g').attr('class', 'graph-root');
    gRef.current = g.node();
    g.append('g').attr('class', 'edges');
    g.append('g').attr('class', 'nodes');

    const z = d3Zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.15, 4])
      .on('zoom', (e) => g.attr('transform', e.transform.toString()));
    zoomBehavior.current = z;
    svg.call(z);
    // 点空白处退出聚焦
    svg.on('click', () => {
      if (focusRef.current) {
        focusRef.current = null;
        setSelNode(null);
        onSelectNode(null);
        if (gRef.current) {
          const nodeLayer = select(gRef.current).selectAll<SVGGElement, SimNode>('g.node-group');
          nodeLayer.style('opacity', 1);
          select(gRef.current).selectAll<SVGPathElement, SimLink>('path').style('opacity', null).attr('stroke-width', null);
          nodeLayer.selectAll('text.node-label').classed('label-dim', true);
        }
      }
    });

    return () => {
      simRef.current?.stop();
    };
  }, []);

  useEffect(() => {
    renderGraph();
  }, [visible, renderGraph]);

  useEffect(() => {
    const onResize = () => renderGraph();
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [renderGraph]);

  function clearFocus() {
    focusRef.current = null;
    setSelNode(null);
    onSelectNode(null);
  }

  function handleSearch(q: string) {
    setQuery(q);
    if (!q.trim()) {
      clearFocus();
      renderGraph();
      return;
    }
    const hit = visible.nodes.find(
      (n) => n.label.toLowerCase().includes(q.toLowerCase()) || n.id.toLowerCase().includes(q.toLowerCase()),
    );
    if (hit) {
      focusRef.current = hit.id;
      setSelNode(hit);
      onSelectNode(hit);
      centerRef.current?.(hit.id);
      renderGraph();
    }
  }

  function toggleKind(kind: string) {
    setActiveKinds((prev) => {
      const n = new Set(prev);
      if (n.has(kind)) n.delete(kind);
      else n.add(kind);
      return n;
    });
    clearFocus();
  }

  function togglePhysics() {
    setPhysicsOn((prev) => {
      const next = !prev;
      if (next) simRef.current?.alphaTarget(0.25).restart();
      else simRef.current?.stop();
      return next;
    });
  }

  function resetView() {
    if (!svgRef.current || !zoomBehavior.current) return;
    clearFocus();
    zoomBehavior.current.transform(select(svgRef.current), d3ZoomIdentity);
    renderGraph();
  }
  function zoomIn() {
    if (svgRef.current && zoomBehavior.current) zoomBehavior.current.scaleBy(select(svgRef.current), 1.5);
  }
  function zoomOut() {
    if (svgRef.current && zoomBehavior.current) zoomBehavior.current.scaleBy(select(svgRef.current), 0.67);
  }

  const allKinds = [...new Set(rawRef.current.nodes.map((n) => n.kind))];
  const focusActive = focusRef.current != null;

  return (
    <div class="flex flex-col h-[calc(100vh-7.5rem)]">
      {/* 工具条 */}
      <div class="flex items-center gap-3 px-5 py-2.5 border-b border-border-dim flex-wrap">
        <div class="relative w-52">
          <span class="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-4 text-note leading-none">⌕</span>
          <input
            value={query}
            onInput={(e) => handleSearch((e.target as HTMLInputElement).value)}
            placeholder="搜索并聚焦实体"
            class="w-full pl-8 pr-3 py-1.5 text-note bg-bg-card border border-border rounded-md text-text placeholder:text-text-4 focus:outline-none focus:border-primary/50 transition-colors"
          />
        </div>

        <div class="flex items-center rounded-md border border-border overflow-hidden">
          {(Object.keys(DENSITY_CONFIG) as Density[]).map((d) => (
            <button
              key={d}
              onClick={() => {
                setDensity(d);
                clearFocus();
              }}
              title={DENSITY_CONFIG[d].hint}
              class={`px-2.5 py-1.5 text-micro transition-colors ${
                density === d ? 'bg-primary-soft text-primary' : 'text-text-3 hover:text-text-2 hover:bg-bg-card'
              }`}
            >
              {DENSITY_CONFIG[d].label}
            </button>
          ))}
        </div>

        <div class="flex items-center gap-3 text-micro font-mono text-text-3 tabular-nums">
          <span>{visible.nodes.length} 实体</span>
          <span>{visible.edges.length} 关系</span>
        </div>

        {/* 图例：形状即类别，点击开关 */}
        <div class="flex items-center gap-2 ml-auto flex-wrap">
          {allKinds.map((k) => {
            const p = palette(k);
            const active = activeKinds.has(k);
            return (
              <button
                key={k}
                onClick={() => toggleKind(k)}
                aria-pressed={active}
                title={`${active ? '隐藏' : '显示'}${p.label}`}
                class={`flex items-center gap-1.5 px-2 py-1 rounded text-micro border transition-colors ${
                  active ? 'border-border' : 'border-transparent'
                }`}
                style={{ color: active ? p.base : '#4d5767' }}
              >
                <KindGlyph kind={k} size={10} color={active ? p.base : '#4d5767'} />
                <span>{p.label}</span>
              </button>
            );
          })}
          {focusActive && (
            <button onClick={resetView} class="text-micro text-primary hover:text-primary/80 transition-colors ml-1">
              退出聚焦
            </button>
          )}
        </div>
      </div>

      <div ref={wrapRef} class="flex-1 relative overflow-hidden bg-bg">
        <svg ref={svgRef} class="absolute inset-0 w-full h-full" />

        {loading && (
          <div class="absolute inset-0 flex flex-col items-center justify-center z-10 gap-2.5">
            <div class="w-6 h-6 border-2 border-primary/25 border-t-primary rounded-full animate-spin" />
            <p class="text-micro text-text-4">正在构建图谱…</p>
          </div>
        )}

        {!loading && visible.nodes.length < 3 && (
          <div class="absolute inset-0 flex flex-col items-center justify-center z-10 text-text-4 gap-1.5">
            <span class="text-2xl opacity-30">✦</span>
            <p class="text-note">图谱里还没有足够的实体</p>
            <p class="text-micro">先在会话详情里看几轮对话，实体从工具调用与文件路径里抽取</p>
          </div>
        )}

        <div class="absolute bottom-4 left-4 flex flex-col gap-1.5 z-10">
          <CtrlBtn onClick={zoomIn} label="放大">＋</CtrlBtn>
          <CtrlBtn onClick={zoomOut} label="缩小">－</CtrlBtn>
          <CtrlBtn onClick={resetView} label="重置视图">⟲</CtrlBtn>
          <CtrlBtn onClick={togglePhysics} label={physicsOn ? '停止布局' : '启动力导向布局'} active={physicsOn}>
            ⚡
          </CtrlBtn>
        </div>

        {hoverNode && !selNode && (
          <div class="absolute bottom-4 left-16 z-10 max-w-md pointer-events-none">
            <div class="bg-bg-card/95 backdrop-blur border border-border rounded-md px-3 py-2">
              <div class="flex items-center gap-2">
                <KindGlyph kind={hoverNode.kind} size={10} color={palette(hoverNode.kind).base} />
                <span class="text-note text-text break-all">{hoverNode.label}</span>
              </div>
              <div class="text-micro font-mono text-text-4 mt-0.5">
                {palette(hoverNode.kind).label} · 权重 {hoverNode.weight.toFixed(1)}
              </div>
            </div>
          </div>
        )}

        {selNode && (
          <GraphPanel
            node={selNode}
            data={{ nodes: visible.nodes, edges: visible.edges }}
            onClose={() => {
              clearFocus();
              renderGraph();
            }}
            onJump={(id) => {
              const n = rawRef.current.nodes.find((x) => x.id === id);
              if (n) {
                focusRef.current = n.id;
                setSelNode(n);
                onSelectNode(n);
                centerRef.current?.(n.id);
                renderGraph();
              }
            }}
          />
        )}
      </div>
    </div>
  );
}

/** 三类关系的线条样式。 */
const EDGE_STYLE: Record<
  EdgeClass,
  { stroke: string; width: (w: number) => number; opacity: (w: number) => number; dash: string | null }
> = {
  structural: {
    stroke: 'rgba(150,168,196,.55)',
    width: (w) => 0.6 + Math.min(w, 14) * 0.13,
    opacity: (w) => 0.22 + Math.min(w / 30, 0.45),
    dash: null,
  },
  topic: {
    stroke: '#f0a332',
    width: (w) => 0.5 + Math.min(w, 12) * 0.1,
    opacity: (w) => 0.16 + Math.min(w / 40, 0.3),
    dash: '3 3',
  },
  cooccur: {
    stroke: 'rgba(120,135,160,.45)',
    width: () => 0.5,
    opacity: () => 0.09,
    dash: '1 5',
  },
};

function idOf(v: SimNode | string): string {
  return typeof v === 'string' ? v : v.id;
}

/** 标签阈值：常驻标签只给权重最高的约四分之一（上限 24 个）。 */
function labelThreshold(nodes: GraphNode[]): number {
  if (nodes.length === 0) return Infinity;
  const sorted: GraphNode[] = [...nodes].sort((a, b) => b.weight - a.weight);
  // 常驻标签数量与节点规模挂钩，上限 14：标签多了必然互相压字，
  // 真正需要看哪个实体时用搜索或悬停，比全挤在屏幕上更有效。
  const count = Math.min(14, Math.max(5, Math.round(Math.sqrt(sorted.length) * 1.1)));
  return sorted[Math.min(count, sorted.length - 1)].weight;
}

/** 正六边形顶点（以原点为中心）。 */
function hexPoints(r: number): string {
  return Array.from({ length: 6 }, (_, i) => {
    const a = (Math.PI / 3) * i - Math.PI / 2;
    return `${(r * Math.cos(a)).toFixed(2)},${(r * Math.sin(a)).toFixed(2)}`;
  }).join(' ');
}

/** 图例形状小标（与画布内形状一致）。 */
function KindGlyph({ kind, size, color }: { kind: string; size: number; color: string }) {
  if (kind === 'file') {
    return (
      <span style={{ display: 'inline-block', width: size, height: size, borderRadius: 2, background: color }} />
    );
  }
  if (kind === 'topic') {
    return (
      <svg width={size} height={size} viewBox="-10 -10 20 20" style={{ display: 'block' }}>
        <polygon points={hexPoints(9)} fill={color} />
      </svg>
    );
  }
  return (
    <span style={{ display: 'inline-block', width: size, height: size, borderRadius: '50%', background: color }} />
  );
}

/** 画布控件按钮。 */
function CtrlBtn({
  onClick,
  label,
  active,
  children,
}: {
  onClick: () => void;
  label: string;
  active?: boolean;
  children: preact.ComponentChildren;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      aria-label={label}
      class={`w-8 h-8 rounded-md border bg-bg-card/90 backdrop-blur flex items-center justify-center text-note transition-colors ${
        active ? 'border-primary/40 text-primary' : 'border-border text-text-3 hover:text-text hover:border-border-hover'
      }`}
    >
      {children}
    </button>
  );
}
