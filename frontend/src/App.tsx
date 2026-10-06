import { useState, useEffect, useCallback } from 'preact/hooks';
import type { SessionSummary, SessionDetail, Stats, GraphData } from './types';
import { api } from './lib/api';
import { useApi } from './hooks/useApi';
import { Sidebar } from './components/Sidebar';
import { TimelineView } from './components/TimelineView';
import { DashboardView } from './components/DashboardView';
import { GraphView } from './components/GraphView';

type Tab = 'timeline' | 'dashboard' | 'graph';

export function App() {
  const [tab, setTab] = useState<Tab>('timeline');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [graph, setGraph] = useState<GraphData | null>(null);
  const [graphLoading, setGraphLoading] = useState(false);

  // Fetch sessions + stats on mount and on search change
  const loadSessions = useCallback(async () => {
    try {
      const [s, st] = await Promise.all([
        searchQuery ? api.searchSessions(searchQuery) : api.listSessions(),
        api.getStats(),
      ]);
      setSessions(s);
      setStats(st);
    } catch (e) {
      console.error('Failed to load sessions:', e);
    }
  }, [searchQuery]);

  useEffect(() => {
    loadSessions();
    // Poll every 5s for live updates
    const id = setInterval(loadSessions, 5000);
    return () => clearInterval(id);
  }, [loadSessions]);

  // Load session detail when selected
  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      setGraph(null);
      return;
    }
    setDetailLoading(true);
    api
      .getSession(selectedId)
      .then(setDetail)
      .catch((e) => console.error('Failed to load session:', e))
      .finally(() => setDetailLoading(false));
  }, [selectedId]);

  // Load graph when graph tab is active
  useEffect(() => {
    if (tab !== 'graph' || !selectedId) {
      setGraph(null);
      return;
    }
    setGraphLoading(true);
    api
      .getGraph(selectedId)
      .then(setGraph)
      .catch((e) => console.error('Failed to load graph:', e))
      .finally(() => setGraphLoading(false));
  }, [tab, selectedId]);

  const handleSearch = useCallback((q: string) => {
    setSearchQuery(q);
  }, []);

  const handleSelect = useCallback((id: string) => {
    setSelectedId(id);
    setTab('timeline');
  }, []);

  return (
    <div class="flex h-screen w-screen overflow-hidden">
      <Sidebar
        sessions={sessions}
        stats={stats}
        selectedId={selectedId}
        searchQuery={searchQuery}
        onSearch={handleSearch}
        onSelect={handleSelect}
      />

      <main class="flex-1 flex flex-col overflow-hidden bg-bg">
        {/* Top bar */}
        <div class="flex items-center gap-4 px-6 py-3 border-b border-border bg-bg-elev min-h-[52px]">
          <div class="flex-1 min-w-0">
            <h2 class="text-base font-semibold truncate">
              {detail?.title || stats?.total_sessions ? '总览' : 'Lens'}
            </h2>
            {detail && (
              <div class="text-sm text-text-3 mt-0.5 truncate">{detail.project_path}</div>
            )}
          </div>
          {stats && (
            <div class="flex gap-6">
              <TopStat value={stats.total_sessions} label="会话" />
              <TopStat value={stats.total_turns} label="调用" />
              <TopStat value={stats.total_tokens} label="Tokens" />
            </div>
          )}
        </div>

        {/* Tabs */}
        {selectedId && (
          <div class="flex border-b border-border gap-0">
            <TabBtn active={tab === 'timeline'} onClick={() => setTab('timeline')}>
              时间线
            </TabBtn>
            <TabBtn active={tab === 'graph'} onClick={() => setTab('graph')}>
              关系图
            </TabBtn>
          </div>
        )}

        {/* Views */}
        <div class="flex-1 overflow-hidden">
          {!selectedId && (
            <DashboardView stats={stats} sessions={sessions} />
          )}
          {selectedId && tab === 'timeline' && (
            <TimelineView session={detail} loading={detailLoading} />
          )}
          {tab === 'graph' && (
            <GraphView data={graph} loading={graphLoading} />
          )}
        </div>
      </main>
    </div>
  );
}

function TopStat({ value, label }: { value: number; label: string }) {
  return (
    <div class="flex flex-col items-end">
      <span class="text-base font-bold tabular-nums">{value.toLocaleString()}</span>
      <span class="text-[9px] text-text-4 uppercase tracking-wider">{label}</span>
    </div>
  );
}

function TabBtn({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: preact.ComponentChildren;
}) {
  return (
    <button
      onClick={onClick}
      class={`px-4 py-2.5 text-sm font-medium border-b-2 transition-colors ${
        active
          ? 'text-text border-primary'
          : 'text-text-4 border-transparent hover:text-text'
      }`}
    >
      {children}
    </button>
  );
}
