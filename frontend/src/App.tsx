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
    <div class="flex h-screen bg-bg text-text font-sans text-body">
        <Sidebar
          sessions={sessions}
          stats={stats}
          onSelect={selectSession}
          selectedId={selectedSession}
          searchQuery={searchQuery}
          onSearch={setSearchQuery}
        />
      <main class="flex-1 min-w-0 flex flex-col">
        <header class="flex items-center gap-1 px-6 pt-4 pb-3 flex-shrink-0">
          <TabBtn active={tab === "dashboard"} onClick={() => setTab("dashboard")}>仪表盘</TabBtn>
          <TabBtn active={tab === "graph"} onClick={() => setTab("graph")}>知识图谱</TabBtn>
          {selectedSession && (
            <TabBtn active={tab === "session"} onClick={() => setTab("session")}>会话详情</TabBtn>
          )}
        </header>
        {error && (
          <div class="mx-6 mb-2 px-3 py-2 rounded-md bg-red-500/8 border border-red-500/25 text-note text-red-300">
            数据加载失败：{error}
          </div>
        )}
        <div class="flex-1 min-h-0">
          {loading && sessions.length === 0 && (
            <div class="flex items-center justify-center h-full text-text-4 text-note">加载中…</div>
          )}
          {tab === "dashboard" && <DashboardView sessions={sessions} stats={stats} onSelect={selectSession} />}
          {tab === "session" && selectedSession && <TimelineView sessionId={selectedSession} />}
          {tab === "graph" && (
            <GraphView
              data={graphData}
              onDataChange={setGraphData}
              onSelectNode={setSelectedNode}
            />
          )}
        </div>
      </main>
    </div>
  );
}

function TabBtn({ active, onClick, children }: { active: boolean; onClick: () => void; children: preact.ComponentChildren }) {
  return (
    <button
      onClick={onClick}
      class={`px-2.5 py-1 rounded-md text-note transition-colors ${
        active
          ? "bg-bg-card text-text border border-border"
          : "text-text-3 hover:text-text-2 border border-transparent"
      }`}
    >
      {children}
    </button>
  );
}
