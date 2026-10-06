import type { Stats, SessionSummary } from '../types';
import { AGENT_COLORS, AGENT_LABELS } from '../types';
import { KpiCard } from './KpiCard';
import { DonutChart } from './DonutChart';
import { Sparkline } from './Sparkline';

interface Props {
  stats: Stats | null;
  sessions: SessionSummary[];
}

export function DashboardView({ stats, sessions }: Props) {
  if (!stats) {
    return (
      <div class="flex items-center justify-center h-full text-text-4">
        加载中...
      </div>
    );
  }

  const agentSlices = stats.agents.map((a) => ({
    label: AGENT_LABELS[a.name] || a.name,
    value: a.count,
    color: AGENT_COLORS[a.name] || '#6b7280',
  }));

  const maxModelCount = Math.max(...stats.models.map((m) => m.count), 1);

  // Mock sparkline - in real app would come from time-series data
  const tokenTrend = sessions.slice(0, 10).map((s) => s.token_count);

  return (
    <div class="h-full overflow-y-auto scroll-thin px-8 py-6">
      {/* KPI row */}
      <div class="grid grid-cols-3 gap-4 mb-6">
        <KpiCard label="总会话" value={stats.total_sessions} color="#6366f5" />
        <KpiCard label="总调用" value={stats.total_turns} color="#3b82f6" />
        <KpiCard label="总 Tokens" value={stats.total_tokens} color="#22c55e" />
      </div>

      <div class="grid grid-cols-3 gap-4">
        {/* Agent distribution */}
        <div class="bg-bg-card border border-border rounded-xl p-5 hover:border-border-hover transition-colors">
          <div class="text-xs font-semibold text-text-4 uppercase tracking-wider mb-4">
            Agent 分布
          </div>
          <DonutChart data={agentSlices} />
        </div>

        {/* Token trend */}
        <div class="bg-bg-card border border-border rounded-xl p-5 hover:border-border-hover transition-colors">
          <div class="text-xs font-semibold text-text-4 uppercase tracking-wider mb-4">
            Token 趋势
          </div>
          <div class="h-[100px] flex items-end">
            <Sparkline data={tokenTrend.length > 1 ? tokenTrend : [0, 0]} color="#6366f5" />
          </div>
        </div>

        {/* Model comparison */}
        <div class="bg-bg-card border border-border rounded-xl p-5 hover:border-border-hover transition-colors">
          <div class="text-xs font-semibold text-text-4 uppercase tracking-wider mb-4">
            模型使用
          </div>
          <div class="flex flex-col gap-2.5">
            {stats.models.slice(0, 5).map((m) => {
              const color = AGENT_COLORS[m.name] || '#6366f5';
              const pct = (m.count / maxModelCount) * 100;
              return (
                <div key={m.name} class="flex items-center gap-2.5">
                  <span class="text-sm text-text-2 w-20 flex-shrink-0 truncate">
                    {AGENT_LABELS[m.name] || m.name}
                  </span>
                  <div class="flex-1 h-1.5 bg-bg rounded-full overflow-hidden">
                    <div
                      class="h-full rounded-full transition-all duration-500"
                      style={{ width: `${pct}%`, background: color }}
                    />
                  </div>
                  <span class="text-xs font-semibold text-text-3 w-8 text-right tabular-nums">
                    {m.count}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
