import type { SessionSummary, Stats } from '../types';
import { agentColor, agentLabel } from '../agents';
import { compactNumber, exactNumber, relativeTime, absoluteTime } from '../lib/format';

interface Props {
  sessions: SessionSummary[];
  stats: Stats | null;
  selectedId: string | null;
  searchQuery: string;
  onSearch: (q: string) => void;
  onSelect: (id: string) => void;
}

export function Sidebar({ sessions, stats, selectedId, searchQuery, onSearch, onSelect }: Props) {
  // agent 过滤在前端做：列表已按时间倒序，这里只按"有哪些 agent"分流
  const agentCounts = new Map<string, number>();
  for (const s of sessions) {
    agentCounts.set(s.agent_type, (agentCounts.get(s.agent_type) ?? 0) + 1);
  }

  return (
    <aside class="w-[290px] flex-shrink-0 flex flex-col border-r border-border bg-bg h-full">
      <div class="px-4 py-3.5 flex items-center gap-2.5 border-b border-border-dim">
        <div class="w-7 h-7 rounded-md bg-primary/10 border border-primary/25 flex items-center justify-center">
          <span class="text-primary font-semibold text-note leading-none">L</span>
        </div>
        <div class="min-w-0">
          <div class="text-lead font-semibold tracking-tight leading-tight">Lens</div>
          <div class="text-micro text-text-4">AI 编程代理透镜</div>
        </div>
        {stats && (
          <div class="ml-auto text-right">
            <div class="text-micro font-mono text-text-2 tabular-nums leading-tight">
              {compactNumber(stats.total_turns)}
            </div>
            <div class="text-micro text-text-4 leading-tight">调用</div>
          </div>
        )}
      </div>

      <div class="px-3 pt-3 pb-2">
        <div class="relative">
          <input
            type="text"
            value={searchQuery}
            onInput={(e) => onSearch((e.target as HTMLInputElement).value)}
            placeholder="搜索会话"
            class="w-full bg-bg-card border border-border rounded-md pl-8 pr-3 py-1.5 text-note text-text placeholder:text-text-4 focus:outline-none focus:border-primary/50 transition-colors"
          />
          <span class="absolute left-2.5 top-1/2 -translate-y-1/2 text-text-4 text-note leading-none">
            ⌕
          </span>
        </div>
      </div>

      {agentCounts.size > 1 && (
        <div class="px-3 pb-2 flex flex-wrap gap-x-3 gap-y-1">
          {[...agentCounts.entries()].slice(0, 5).map(([agent, count]) => (
            <span key={agent} class="inline-flex items-center gap-1.5 text-micro text-text-3">
              <span
                class="w-1.5 h-1.5 rounded-full flex-shrink-0"
                style={{ background: agentColor(agent) }}
              />
              {agentLabel(agent)}
              <span class="font-mono text-text-4 tabular-nums">{count}</span>
            </span>
          ))}
        </div>
      )}

      <div class="flex-1 overflow-y-auto scroll-thin">
        {sessions.length === 0 ? (
          <div class="px-4 py-10 text-center">
            <p class="text-note text-text-3">
              {searchQuery ? '没有匹配的会话' : '还没有记录到会话'}
            </p>
            <p class="text-micro text-text-4 mt-1.5 leading-relaxed">
              {searchQuery ? (
                '换个关键词试试'
              ) : (
                <>
                  启动代理：
                  <code class="font-mono text-text-3">lens proxy</code>
                  <br />
                  然后把 agent 的 HTTP(S)_PROXY 指过来
                </>
              )}
            </p>
          </div>
        ) : (
          sessions.map((s) => {
            const color = agentColor(s.agent_type);
            const isActive = s.id === selectedId;
            return (
              <button
                key={s.id}
                onClick={() => onSelect(s.id)}
                title={`${s.title}\n${agentLabel(s.agent_type)} · ${exactNumber(s.message_count)} 轮 · ${absoluteTime(s.updated_at)}`}
                class={`w-full text-left px-3 py-2 border-l-2 transition-colors ${
                  isActive
                    ? 'border-l-primary bg-primary-soft'
                    : 'border-l-transparent hover:bg-bg-card'
                }`}
              >
                <div class="text-note leading-snug line-clamp-2 text-text-2">{s.title}</div>
                <div class="flex items-center gap-2 mt-1">
                  <span class="w-1.5 h-1.5 rounded-full flex-shrink-0" style={{ background: color }} />
                  <span class="text-micro text-text-4 truncate max-w-[110px] font-mono">
                    {s.model ?? agentLabel(s.agent_type)}
                  </span>
                  <span class="text-micro text-text-4 font-mono tabular-nums ml-auto flex-shrink-0">
                    {relativeTime(s.updated_at)}
                  </span>
                </div>
              </button>
            );
          })
        )}
      </div>
    </aside>
  );
}
