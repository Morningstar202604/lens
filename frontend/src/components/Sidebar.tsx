import type { SessionSummary, Stats } from '../types';
import { AGENT_COLORS, AGENT_LABELS } from '../types';
import { KpiCard } from './KpiCard';

interface Props {
  sessions: SessionSummary[];
  stats: Stats | null;
  selectedId: string | null;
  searchQuery: string;
  onSearch: (q: string) => void;
  onSelect: (id: string) => void;
}

export function Sidebar({ sessions, stats, selectedId, searchQuery, onSearch, onSelect }: Props) {
  const agents = stats?.agents ?? [];
  const topAgents = agents.slice(0, 4);

  return (
    <aside class="w-72 flex-shrink-0 flex flex-col border-r border-border bg-bg-elev h-full">
      {/* Brand */}
      <div class="px-5 py-4 flex items-center gap-3 border-b border-border">
        <div class="w-9 h-9 rounded-lg bg-primary flex items-center justify-center text-white font-bold text-lg shadow-lg shadow-primary/20">
          L
        </div>
        <div>
          <h1 class="text-base font-semibold tracking-tight">Lens</h1>
          <div class="text-[10px] text-text-4">AI 编程代理透镜</div>
        </div>
      </div>

      {/* Search */}
      <div class="px-3 py-2">
        <input
          type="text"
          value={searchQuery}
          onInput={(e) => onSearch((e.target as HTMLInputElement).value)}
          placeholder="搜索会话..."
          class="w-full bg-bg border border-border rounded-md px-3 py-2 text-sm text-text placeholder:text-text-4 focus:outline-none focus:border-primary transition-colors"
        />
      </div>

      {/* Stats KPIs */}
      {stats && (
        <div class="px-3 pb-2">
          <div class="grid grid-cols-3 gap-1.5">
            <KpiCard label="会话" value={stats.total_sessions} color="#6366f5" />
            <KpiCard label="调用" value={stats.total_turns} color="#3b82f6" />
            <KpiCard label="Tokens" value={stats.total_tokens} color="#22c55e" />
          </div>
        </div>
      )}

      {/* Agent chips */}
      {topAgents.length > 0 && (
        <div class="px-3 pb-2 flex flex-wrap gap-1.5">
          {topAgents.map((a) => {
            const color = AGENT_COLORS[a.name] || '#6b7280';
            return (
              <span
                key={a.name}
                class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium"
                style={{ background: `${color}15`, color }}
              >
                <span class="w-1.5 h-1.5 rounded-full" style={{ background: color }} />
                {AGENT_LABELS[a.name] || a.name}
              </span>
            );
          })}
        </div>
      )}

      {/* Session list */}
      <div class="flex-1 overflow-y-auto scroll-thin px-2 py-1">
        {sessions.length === 0 ? (
          <div class="text-center text-text-4 text-sm py-12">
            暂无会话数据
            <br />
            <span class="text-xs">启动代理后开始拦截</span>
          </div>
        ) : (
          sessions.map((s) => {
            const color = AGENT_COLORS[s.agent_type] || '#6b7280';
            const isActive = s.id === selectedId;
            return (
              <button
                key={s.id}
                onClick={() => onSelect(s.id)}
                class={`w-full text-left px-3 py-2.5 rounded-lg mb-0.5 border transition-all ${
                  isActive
                    ? 'bg-primary/5 border-border-active shadow-[0_0_0_1px_rgba(99,102,245,.1)]'
                    : 'bg-transparent border-transparent hover:bg-bg-card hover:border-border'
                }`}
              >
                <div class="flex items-center gap-2">
                  <span class="w-1.5 h-1.5 rounded-full flex-shrink-0" style={{ background: color }} />
                  <span class="text-sm font-medium truncate flex-1">{s.title}</span>
                </div>
                <div class="flex items-center gap-2 mt-1.5 pl-3.5">
                  {s.model && (
                    <span class="text-[10px] text-text-4 bg-bg px-1.5 py-0.5 rounded">
                      {s.model}
                    </span>
                  )}
                  <span class="text-[10px] text-text-4 ml-auto">{s.turn_count} turns</span>
                </div>
              </button>
            );
          })
        )}
      </div>
    </aside>
  );
}
