import { Component, createElement } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { SessionSummary, Stats, GraphData, GraphNode } from "./types";
import { api } from "./lib/api";
import { DashboardView } from "./components/DashboardView";
import { Sidebar } from "./components/Sidebar";
import { TimelineView } from "./components/TimelineView";
import { GraphView } from "./components/GraphView";

type Tab = "dashboard" | "session" | "graph";

interface ErrorBoundaryState {
  error: Error | null;
}

export class ErrorBoundary extends Component<{}, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };
  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }
  componentDidCatch(error: Error, info: any) {
    console.error("[Lens] 渲染异常:", error, info?.componentStack);
  }
  render() {
    if (this.state.error) {
      return createElement("div", { class: "min-h-screen flex items-center justify-center bg-bg text-text p-6" },
        createElement("div", { class: "text-center space-y-3" },
          createElement("h1", { class: "text-xl font-bold text-red-400" }, "界面发生错误"),
          createElement("p", { class: "text-sm text-text-3 font-mono" }, this.state.error.message),
          createElement("button", { class: "px-4 py-2 rounded bg-primary text-white", onClick: () => this.setState({ error: null }) }, "重试")
        )
      );
    }
    return this.props.children;
  }
}

export function App() {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [tab, setTab] = useState<Tab>("dashboard");
  const [selectedSession, setSelectedSession] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [graphData, setGraphData] = useState<GraphData | null>(null);
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const failCount = useRef(0);
  const intervalId = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      if (typeof document !== "undefined" && document.hidden) return;
      try {
        const data = await api.searchSessions(searchQuery);
        if (!cancelled) {
          setSessions(data);
          setError(null);
          failCount.current = 0;
          setLoading(false);
        }
        // Also fetch stats
        const s = await api.getStats();
        if (!cancelled && s) setStats(s);
      } catch (e: any) {
        if (!cancelled) {
          failCount.current += 1;
          setError(e.message || String(e));
        }
      }
    }
    load();
    function scheduleNext() {
      const backoff = Math.min(3000 * Math.pow(2, failCount.current - 1), 30000);
      intervalId.current = window.setTimeout(() => { load().then(scheduleNext); }, backoff);
    }
    scheduleNext();
    return () => {
      cancelled = true;
      if (intervalId.current !== null) clearTimeout(intervalId.current);
    };
  }, [searchQuery]);

  function selectSession(id: string) {
    setSelectedSession(id);
    setTab("session");
  }

  return (
    <div class="flex h-screen bg-bg text-text">
        <Sidebar
          sessions={sessions}
          stats={stats}
          onSelect={selectSession}
          selectedId={selectedSession}
          searchQuery={searchQuery}
          onSearch={setSearchQuery}
        />
      <main class="flex-1 overflow-y-auto p-4 md:p-6">
        <div class="flex gap-2 mb-4">
          <TabBtn active={tab === "dashboard"} onClick={() => setTab("dashboard")}>仪表盘</TabBtn>
          <TabBtn active={tab === "graph"} onClick={() => setTab("graph")}>知识图谱</TabBtn>
          {selectedSession && (
            <TabBtn active={tab === "session"} onClick={() => setTab("session")}>会话详情</TabBtn>
          )}
        </div>
        {error && (
          <div class="mb-3 px-3 py-2 rounded bg-red-900/30 border border-red-800/60 text-sm text-red-300">
            数据加载失败: {error}
          </div>
        )}
        {loading && sessions.length === 0 && (
          <div class="text-center text-text-3 py-12">加载中...</div>
        )}
        {tab === "dashboard" && <DashboardView sessions={sessions} stats={stats} />}
        {tab === "session" && selectedSession && <TimelineView sessionId={selectedSession} />}
        {tab === "graph" && (
          <GraphView
            data={graphData}
            onDataChange={setGraphData}
            onSelectNode={setSelectedNode}
          />
        )}
      </main>
    </div>
  );
}

function TabBtn({ active, onClick, children }: { active: boolean; onClick: () => void; children: preact.ComponentChildren }) {
  return (
    <button onClick={onClick}
      class={`px-4 py-1.5 rounded text-sm font-medium transition-colors ${active ? "bg-primary text-white" : "bg-bg-card text-text-3 hover:text-text-2 hover:bg-bg-elev"}`}>
      {children}
    </button>
  );
}
