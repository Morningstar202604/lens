import { useEffect, useRef } from 'preact/hooks';
import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCenter,
  forceCollide,
  type Simulation,
} from 'd3-force';
import type { GraphData, GraphNode, GraphEdge } from '../types';

interface Props {
  data: GraphData | null;
  loading: boolean;
}

const KIND_COLORS: Record<string, string> = {
  file: '#6366f5',
  function: '#06b6d4',
  topic: '#f59b0b',
};

export function GraphView({ data, loading }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!containerRef.current || !data || data.nodes.length < 2) return;

    const container = containerRef.current;
    const rect = container.getBoundingClientRect();
    const width = rect.width;
    const height = rect.height;

    container.innerHTML = '';

    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('width', String(width));
    svg.setAttribute('height', String(height));
    svg.style.display = 'block';
    container.appendChild(svg);

    const simNodes: GraphNode[] = data.nodes.map((n) => ({ ...n }));
    const simEdges: GraphEdge[] = data.edges.map((e) => ({ ...e }));

    const linkGroup = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    svg.appendChild(linkGroup);

    const linkEls: { line: SVGLineElement; source: any; target: any }[] = [];
    simEdges.forEach((e) => {
      const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
      line.setAttribute('stroke', 'rgba(255,255,255,0.15)');
      line.setAttribute('stroke-width', '1');
      linkGroup.appendChild(line);
      linkEls.push({ line, source: e.source, target: e.target });
    });

    const nodeGroup = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    svg.appendChild(nodeGroup);

    const nodeEls: { node: GraphNode & { x?: number; y?: number; fx?: number | null; fy?: number | null }; el: SVGGElement; circle: SVGCircleElement }[] = [];
    simNodes.forEach((n) => {
      const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
      g.style.cursor = 'grab';

      const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
      const r = Math.sqrt(n.weight) * 5 + 5;
      circle.setAttribute('r', String(r));
      circle.setAttribute('fill', KIND_COLORS[n.kind] || '#6b7280');
      circle.setAttribute('opacity', '0.85');
      g.appendChild(circle);

      const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
      text.textContent = n.label;
      text.setAttribute('x', String(r + 4));
      text.setAttribute('y', '4');
      text.setAttribute('fill', '#c8cae0');
      text.setAttribute('font-size', '11');
      g.appendChild(text);

      nodeGroup.appendChild(g);
      nodeEls.push({ node: n as any, el: g, circle });
    });

    // Simulation
    const simulation: Simulation<GraphNode, GraphEdge> = forceSimulation(simNodes as any)
      .force(
        'link',
        forceLink(simEdges as any)
          .id((d: any) => d.id)
          .distance(120)
          .strength(0.5),
      )
      .force('charge', forceManyBody().strength(-200))
      .force('center', forceCenter(width / 2, height / 2))
      .force(
        'collision',
        forceCollide<GraphNode>().radius((d: any) => Math.sqrt(d.weight) * 5 + 8),
      ) as any;

    // Resolve link source/target references
    linkEls.forEach((l) => {
      const src = simNodes.find((n) => n.id === (l.source as any).id || n.id === l.source);
      const tgt = simNodes.find((n) => n.id === (l.target as any).id || n.id === l.target);
      l.source = src;
      l.target = tgt;
    });

    simulation.on('tick', () => {
      linkEls.forEach((l) => {
        if (l.source?.x != null && l.target?.x != null) {
          l.line.setAttribute('x1', String(l.source.x));
          l.line.setAttribute('y1', String(l.source.y));
          l.line.setAttribute('x2', String(l.target.x));
          l.line.setAttribute('y2', String(l.target.y));
        }
      });
      nodeEls.forEach((n: any) => {
        if (n.node.x != null && n.node.y != null) {
          n.el.setAttribute('transform', `translate(${n.node.x},${n.node.y})`);
        }
      });
    });

    // Simple drag
    let draggedNode: any = null;
    let startX = 0;
    let startY = 0;

    nodeEls.forEach((n: any) => {
      n.el.addEventListener('mousedown', (e: MouseEvent) => {
        e.preventDefault();
        draggedNode = n.node;
        startX = e.clientX;
        startY = e.clientY;
        draggedNode.fx = draggedNode.x;
        draggedNode.fy = draggedNode.y;
        simulation.alphaTarget(0.3).restart();
        n.el.style.cursor = 'grabbing';

        const onMove = (ev: MouseEvent) => {
          const dx = ev.clientX - startX;
          const dy = ev.clientY - startY;
          draggedNode.fx = (draggedNode.x || 0) + dx;
          draggedNode.fy = (draggedNode.y || 0) + dy;
        };

        const onUp = () => {
          draggedNode.fx = null;
          draggedNode.fy = null;
          draggedNode = null;
          n.el.style.cursor = 'grab';
          simulation.alphaTarget(0);
          window.removeEventListener('mousemove', onMove);
          window.removeEventListener('mouseup', onUp);
        };

        window.addEventListener('mousemove', onMove);
        window.addEventListener('mouseup', onUp);
      });
    });

    return () => {
      simulation.stop();
    };
  }, [data]);

  if (loading) {
    return <div class="flex items-center justify-center h-full text-text-4">加载中...</div>;
  }

  if (!data || data.nodes.length < 2) {
    return (
      <div class="flex items-center justify-center h-full text-text-4 text-sm">
        <div class="text-center">
          <div class="text-2xl mb-2">🕸️</div>
          <div>图谱数据不足</div>
          <div class="text-xs mt-1 text-text-4">
            需要至少 2 个节点才能生成关系图
          </div>
        </div>
      </div>
    );
  }

  return (
    <div class="h-full flex flex-col">
      <div class="flex items-center gap-4 px-6 py-3 border-b border-border bg-bg-elev">
        <span class="text-xs font-semibold text-text-4 uppercase tracking-wider">
          实体关系图谱
        </span>
        <span class="text-xs text-text-4">
          {data.nodes.length} 节点 · {data.edges.length} 关系
        </span>
        <span class="text-xs text-text-4 ml-auto">拖拽节点调整布局</span>
      </div>
      <div ref={containerRef} class="flex-1 relative overflow-hidden bg-bg" />
    </div>
  );
}
