import { useEffect, useState } from 'preact/hooks';
import type { ContentBlock, MessageContent, SessionDetail, Turn } from '../types';
import { agentColor, agentLabel } from '../agents';
import { compactNumber, clockTime, exactNumber, shortenPath } from '../lib/format';
import { MessageCard } from './MessageCard';
import { ToolCallCard } from './ToolCallCard';
import { api } from '../lib/api';

interface Props {
  sessionId: string;
}

function contentText(content: MessageContent): string {
  if (typeof content === 'string') return content;
  return (content as ContentBlock[]).map((b) => b.text ?? '').filter(Boolean).join('\n\n');
}

/** 请求里最后一条 user 消息（请求携带完整历史，只展示本轮新增）。 */
function lastUserText(turn: Turn): string {
  const messages = turn.request.messages;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'user') return contentText(messages[i].content);
  }
  return '';
}

function assistantText(turn: Turn): string {
  return turn.response.content
    .filter((b) => b.type === 'text')
    .map((b) => b.text ?? '')
    .filter(Boolean)
    .join('\n\n');
}

/** 本轮工具调用数（用于按轮折叠，避免长会话刷屏）。 */
function toolCount(turn: Turn): number {
  return turn.response.tool_calls?.length ?? 0;
}

export function TimelineView({ sessionId }: Props) {
  const [session, setSession] = useState<SessionDetail | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [showSummary, setShowSummary] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setSummary(null);
    api.getSession(sessionId)
      .then((d) => {
        if (!cancelled) {
          setSession(d);
          setLoading(false);
        }
      })
      .catch(() => !cancelled && setLoading(false));
    api.getSummary(sessionId).then((s) => !cancelled && setSummary(s)).catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  if (loading) {
    return <div class="flex items-center justify-center h-full text-text-4 text-note">加载中…</div>;
  }
  if (!session) {
    return <div class="flex items-center justify-center h-full text-text-4 text-note">选择一个会话查看详情</div>;
  }

  const turns = session.turns;
  const meta = session.session;
  const totalTokens = turns.reduce((s, t) => s + (t.response.usage?.total_tokens ?? 0), 0);
  const totalTools = turns.reduce((s, t) => s + toolCount(t), 0);
  const withReply = turns.filter((t) => assistantText(t).length > 0).length;
  const files = collectFiles(turns);
  const models = [...new Set(turns.map((t) => t.request.model).filter(Boolean))] as string[];

  return (
    <div class="h-full overflow-y-auto scroll-thin">
      <div class="max-w-[900px] px-8 py-7">
        {/* 会话头：一次说清这是什么会话 */}
        <header class="pb-5 border-b border-border-dim">
          <h1 class="text-title font-semibold leading-snug break-words">{meta.title}</h1>
          <div class="flex flex-wrap items-center gap-x-4 gap-y-1 mt-2 text-micro text-text-4 font-mono">
            <span class="inline-flex items-center gap-1.5">
              <span
                class="w-1.5 h-1.5 rounded-full"
                style={{ background: agentColor(meta.agent_type) }}
              />
              {agentLabel(meta.agent_type)}
            </span>
            {models.length > 0 && <span class="truncate max-w-[280px]">{models.join(' / ')}</span>}
            <span>{clockTime(meta.created_at)} → {clockTime(meta.updated_at)}</span>
          </div>

          {/* 事实行：轮次 / 有回复 / 工具 / tokens —— 全部是不同口径的量，不再重复同一数字 */}
          <div class="flex items-baseline gap-7 mt-4">
            <Fact value={exactNumber(turns.length)} label="请求" />
            <Fact value={exactNumber(withReply)} label="有回复" />
            <Fact value={exactNumber(totalTools)} label="工具调用" />
            <Fact value={compactNumber(totalTokens)} label="tokens" exact={exactNumber(totalTokens)} />
          </div>

          {files.length > 0 && (
            <div class="mt-4 flex flex-wrap gap-1.5">
              {files.slice(0, 12).map((f) => (
                <span
                  key={f}
                  class="text-micro font-mono text-text-3 bg-bg-card border border-border-dim rounded px-1.5 py-0.5"
                  title={f}
                >
                  {shortenPath(f)}
                </span>
              ))}
            </div>
          )}

          {summary && summary.trim() && (
            <div class="mt-4">
              <button
                onClick={() => setShowSummary((v) => !v)}
                class="text-micro text-text-3 hover:text-text-2 transition-colors"
              >
                {showSummary ? '▾ 收起摘要' : '▸ 展开摘要'}
              </button>
              {showSummary && (
                <div class="mt-2 text-note text-text-2 whitespace-pre-line leading-relaxed bg-bg-card border border-border-dim rounded-md p-3.5">
                  {summary}
                </div>
              )}
            </div>
          )}
        </header>

        {/* 时间线 */}
        <div class="pt-6">
          {renderTurns(turns)}
        </div>
      </div>
    </div>
  );
}

/** 连续"只有工具调用、无文本"的轮次合并成一行活动摘要。
 *
 * agent（尤其 OpenCode）会把一次任务拆成多轮纯工具调用，逐轮展开会让
 * 时间线里塞满空块。这里把它们压成一行，需要时再展开看细节。
 */
function renderTurns(turns: Turn[]) {
  const out: preact.JSX.Element[] = [];
  let i = 0;
  let turnIndex = 0;

  while (i < turns.length) {
    const t = turns[i];
    const isBare = !lastUserText(t) && !assistantText(t) && (t.response.tool_calls?.length ?? 0) > 0;

    if (isBare) {
      // 收集连续的纯工具轮
      const group: Turn[] = [];
      while (
        i < turns.length &&
        !lastUserText(turns[i]) &&
        !assistantText(turns[i]) &&
        (turns[i].response.tool_calls?.length ?? 0) > 0
      ) {
        group.push(turns[i]);
        i++;
      }
      const firstIdx = turnIndex;
      turnIndex += group.length;
      out.push(<ToolRun key={group[0].id} group={group} startIndex={firstIdx} />);
      continue;
    }

    out.push(<TurnBlock key={t.id} turn={t} index={turnIndex} />);
    turnIndex += 1;
    i += 1;
  }
  return out;
}

/** 一段连续的工具调用。 */
function ToolRun({ group, startIndex }: { group: Turn[]; startIndex: number }) {
  const [open, setOpen] = useState(false);
  const calls = group.flatMap((t) => t.response.tool_calls ?? []);
  const tokens = group.reduce((s, t) => s + (t.response.usage?.total_tokens ?? 0), 0);
  const names = countNames(calls.map((c) => c.function.name));

  return (
    <section class="mb-7">
      <div class="flex items-center gap-2 mb-1.5 text-micro font-mono text-text-4">
        <span class="tabular-nums">
          #{startIndex + 1}–{startIndex + group.length}
        </span>
        <span>{clockTime(group[0].timestamp)}</span>
        {tokens > 0 && <span>· {compactNumber(tokens)} tok</span>}
      </div>
      <button
        onClick={() => setOpen((v) => !v)}
        class="w-full text-left flex items-center gap-2 pl-3 border-l-2 border-l-primary/20 py-0.5 group"
      >
        <span class="text-micro text-text-4 group-hover:text-text-3 transition-colors">
          {open ? '▾' : '▸'}
        </span>
        <span class="text-note text-text-3 group-hover:text-text-2 transition-colors">
          {calls.length} 次工具调用
        </span>
        <span class="text-micro font-mono text-text-4 truncate">
          {names.join(' · ')}
        </span>
      </button>
      {open && (
        <div class="pl-3 border-l-2 border-l-primary/20 mt-1.5">
          {group.map((t, gi) => (
            <div key={t.id}>
              {group.length > 1 && (
                <div class="text-micro font-mono text-text-4 mb-1 mt-2">
                  #{startIndex + gi + 1} {clockTime(t.timestamp)}
                </div>
              )}
              {(t.response.tool_calls ?? []).map((call, ci) => (
                <ToolCallCard
                  key={`${t.id}-tool-${ci}`}
                  call={{
                    name: call.function.name,
                    arguments: (call.function.arguments ?? {}) as Record<string, unknown>,
                  }}
                />
              ))}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

/** 工具名 → 次数，按次数降序。 */
function countNames(names: string[]): string[] {
  const counts = new Map<string, number>();
  for (const n of names) counts.set(n, (counts.get(n) ?? 0) + 1);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([n, c]) => (c > 1 ? `${n}×${c}` : n));
}

/** 单轮：用户输入 + 助手回复 + 工具（工具默认折叠成一行计数）。 */
function TurnBlock({ turn, index }: { turn: Turn; index: number }) {
  const userText = lastUserText(turn);
  const reply = assistantText(turn);
  const tools = turn.response.tool_calls ?? [];
  const [showTools, setShowTools] = useState(false);
  const tokens = turn.response.usage?.total_tokens;

  return (
    <section class="mb-7">
      <div class="flex items-center gap-2 mb-2 text-micro font-mono text-text-4">
        <span class="tabular-nums">#{index + 1}</span>
        <span>{clockTime(turn.timestamp)}</span>
        {tokens != null && <span class="text-text-4">· {compactNumber(tokens)} tok</span>}
        {turn.request.model && <span class="truncate max-w-[200px]">· {turn.request.model}</span>}
      </div>

      {userText && <MessageCard role="user" content={userText} timestamp={clockTime(turn.timestamp)} />}

      {reply && (
        <MessageCard
          role="assistant"
          content={reply}
          timestamp={clockTime(turn.timestamp)}
          tokens={tokens}
        />
      )}

      {tools.length > 0 && (
        <div class="pl-3 border-l border-border-dim ml-1">
          <button
            onClick={() => setShowTools((v) => !v)}
            class="text-micro text-text-3 hover:text-text-2 transition-colors mb-1.5"
          >
            {showTools ? '▾' : '▸'} {tools.length} 次工具调用
            <span class="text-text-4 ml-1.5 font-mono">
              {tools.map((t) => t.function.name).slice(0, 4).join(', ')}
              {tools.length > 4 && ' …'}
            </span>
          </button>
          {showTools &&
            tools.map((call, i) => (
              <ToolCallCard
                key={`${turn.id}-tool-${i}`}
                call={{
                  name: call.function.name,
                  arguments: (call.function.arguments ?? {}) as Record<string, unknown>,
                }}
              />
            ))}
        </div>
      )}
    </section>
  );
}

function Fact({ value, label, exact }: { value: string; label: string; exact?: string }) {
  return (
    <div title={exact}>
      <div class="text-lead font-semibold font-mono tabular-nums leading-tight">{value}</div>
      <div class="text-micro text-text-4 mt-0.5">{label}</div>
    </div>
  );
}

/** 本轮会话涉及的文件（从工具调用参数里取实参路径，比正则抓文本准）。 */
function collectFiles(turns: Turn[]): string[] {
  const seen: string[] = [];
  const keys = ['filePath', 'file_path', 'path', 'filename', 'notebook_path'];
  for (const turn of turns) {
    for (const call of turn.response.tool_calls ?? []) {
      const args = call.function.arguments;
      if (!args || typeof args !== 'object') continue;
      for (const k of keys) {
        const v = (args as Record<string, unknown>)[k];
        if (typeof v === 'string' && v.length > 2 && v.length < 200 && !seen.includes(v)) {
          seen.push(v);
          if (seen.length >= 40) return seen;
        }
      }
    }
  }
  return seen;
}
