import type { Stats, SessionSummary } from '../types';
import { agentColor, agentLabel, colorForName } from '../agents';
import { compactNumber, exactNumber, relativeTime, absoluteTime, percent } from '../lib/format';
import { Metric, SectionTitle } from './Metric';

interface Props {
  stats: Stats | null;
  sessions: SessionSummary[];
  onSelect: (id: string) => void;
}

export function DashboardView({ stats, sessions, onSelect }: Props) {
  if (!stats) {
    return <div class="flex items-center justify-center h-full text-text-4 text-note">加载中…</div>;
  }

  const daily = stats.daily ?? [];
  const recent = sessions.slice(0, 8);
  const activeDays = daily.length;
  const peak = daily.reduce<{ date: string; turns: number; tokens: number } | null>(
    (best, d) => (best === null || d.turns > best.turns ? d : best),
    null,
  );

  return (
    <div class="h-full overflow-y-auto scroll-thin">
      <div class="max-w-[1100px] px-8 py-7">
        {/* 总量：一行，不切三张卡 */}
        <div class="flex items-baseline gap-10 pb-6 border-b border-border-dim">
          <Metric
            value={compactNumber(stats.total_sessions)}
            label="会话"
            exact={exactNumber(stats.total_sessions)}
          />
          <Metric
            value={compactNumber(stats.total_turns)}
            label="调用"
            exact={exactNumber(stats.total_turns)}
          />
          <Metric
            value={compactNumber(stats.total_tokens)}
            label="tokens"
            exact={exactNumber(stats.total_tokens)}
          />
          <div class="ml-auto text-right">
            <div class="text-label text-text-3">
              {daily.length > 0 ? (
                <>
                  <span class="font-mono text-text-2">{activeDays}</span> 天有活动
                  {peak && (
                    <>
                      {' · '}峰值 <span class="font-mono text-text-2">{compactNumber(peak.turns)}</span> 次（
                      {peak.date}）
                    </>
                  )}
                </>
              ) : (
                '暂无按天数据'
              )}
            </div>
          </div>
        </div>

        {/* 真实趋势：来自 DB 按天聚合，不是拿列表拼的 */}
        <section class="pt-6">
          <SectionTitle hint={daily.length ? `${daily[0].date} → ${daily[daily.length - 1].date}` : undefined}>
            每日调用
          </SectionTitle>
          {daily.length > 0 ? (
            <DailyActivity data={daily} />
          ) : (
            <p class="text-note text-text-4 py-6">
              还没有可聚合的时间数据。启动代理并产生对话后这里会显示真实走势。
            </p>
          )}
        </section>

        {/* 两列：agent / 模型 */}
        <div class="grid grid-cols-1 lg:grid-cols-2 gap-x-10 gap-y-7 pt-8">
          <section>
            <SectionTitle hint={`${stats.agents.length} 个`}>Agent 来源</SectionTitle>
            <BarList
              items={stats.agents.map((a) => ({
                key: a.name,
                label: agentLabel(a.name),
                count: a.count,
                color: agentColor(a.name),
              }))}
              total={stats.total_turns}
            />
          </section>

          <section>
            <SectionTitle hint={`${stats.models.length} 个`}>模型调用</SectionTitle>
            <BarList
              items={stats.models.slice(0, 8).map((m) => ({
                key: m.name,
                label: m.name,
                count: m.count,
                color: agentColor(m.name) !== '#7c89a3' ? agentColor(m.name) : colorForName(m.name),
              }))}
              total={stats.total_turns}
            />
          </section>
        </div>

        {/* 最近会话：仪表盘上看得了内容，不用切页 */}
        {recent.length > 0 && (
          <section class="pt-8">
            <SectionTitle hint="点击进入会话详情">最近会话</SectionTitle>
            <div class="border-t border-border-dim">
              {recent.map((s) => (
                <button
                  key={s.id}
                  onClick={() => onSelect(s.id)}
                  class="w-full text-left flex items-center gap-3 py-2.5 border-b border-border-dim hover:bg-bg-card transition-colors group"
                >
                  <span
                    class="w-1.5 h-1.5 rounded-full flex-shrink-0"
                    style={{ background: agentColor(s.agent_type) }}
                  />
                  <span class="text-body text-text-2 truncate flex-1 group-hover:text-text transition-colors">
                    {s.title}
                  </span>
                  {s.model && (
                    <span class="text-micro font-mono text-text-4 truncate max-w-[160px] hidden sm:block">
                      {s.model}
                    </span>
                  )}
                  <span class="text-micro font-mono text-text-4 tabular-nums w-14 text-right">
                    {s.message_count} 轮
                  </span>
                  <span
                    class="text-micro font-mono text-text-4 tabular-nums w-16 text-right"
                    title={absoluteTime(s.updated_at)}
                  >
                    {relativeTime(s.updated_at)}
                  </span>
                </button>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}

/** 每日调用柱状图。纯 SVG，无依赖；高度编码调用量，颜色编码主色。 */
function DailyActivity({ data }: { data: { date: string; turns: number; tokens: number }[] }) {
  const W = 1040;
  const H = 120;
  const gap = data.length > 45 ? 1 : 2;
  const bw = Math.max(1, (W - gap * (data.length - 1)) / data.length);
  const max = Math.max(...data.map((d) => d.turns), 1);

  return (
    <div class="w-full">
      <svg viewBox={`0 0 ${W} ${H}`} class="w-full h-[120px] block" preserveAspectRatio="none">
        {data.map((d, i) => {
          const h = Math.max(1, (d.turns / max) * (H - 18));
          const x = i * (bw + gap);
          return (
            <g key={d.date}>
              <rect
                x={x}
                y={H - 18 - h}
                width={bw}
                height={h}
                fill="rgba(94,224,245,.5)"
                class="hover:fill-[rgba(94,224,245,.85)] transition-colors"
              >
                <title>{`${d.date}\n${d.turns} 次调用 · ${d.tokens.toLocaleString()} tokens`}</title>
              </rect>
            </g>
          );
        })}
        <line x1="0" y1={H - 18} x2={W} y2={H - 18} stroke="rgba(255,255,255,.09)" strokeWidth="1" />
      </svg>
      <div class="flex justify-between text-micro font-mono text-text-4 mt-1.5">
        <span>{data[0].date}</span>
        <span>峰值 {max} 次/天</span>
        <span>{data[data.length - 1].date}</span>
      </div>
    </div>
  );
}

/** 比例条列表：数值 + 占比。比环形图更能读出"量级差"。 */
function BarList({
  items,
  total,
}: {
  items: { key: string; label: string; count: number; color: string }[];
  total: number;
}) {
  if (items.length === 0) {
    return <p class="text-note text-text-4">暂无数据</p>;
  }
  const max = Math.max(...items.map((i) => i.count), 1);

  return (
    <div class="flex flex-col gap-2">
      {items.map((it) => (
        <div key={it.key} class="flex items-center gap-3" title={`${it.label}: ${exactNumber(it.count)} 次`}>
          <span class="text-note text-text-2 w-32 flex-shrink-0 truncate">{it.label}</span>
          <div class="flex-1 h-1 bg-white/[0.04] rounded-full overflow-hidden">
            <div
              class="h-full rounded-full"
              style={{ width: `${(it.count / max) * 100}%`, background: it.color }}
            />
          </div>
          <span class="text-micro font-mono text-text-3 tabular-nums w-14 text-right flex-shrink-0">
            {compactNumber(it.count)}
          </span>
          <span class="text-micro font-mono text-text-4 tabular-nums w-9 text-right flex-shrink-0">
            {percent(it.count, total)}
          </span>
        </div>
      ))}
    </div>
  );
}
