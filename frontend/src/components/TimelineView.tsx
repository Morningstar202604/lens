import { useEffect, useState } from 'preact/hooks';
import type { ContentBlock, MessageContent, SessionDetail, Turn } from '../types';
import { MessageCard } from './MessageCard';
import { ToolCallCard } from './ToolCallCard';
import { api } from '../lib/api';

interface Props {
  sessionId: string;
}

function blockText(block: ContentBlock): string {
  return block.text ?? '';
}

function contentText(content: MessageContent): string {
  if (typeof content === 'string') return content;
  return content.map(blockText).filter(Boolean).join('\n\n');
}

/** 请求里最后一条 user 消息（请求携带完整历史，只展示本轮新增）。 */
function lastUserText(turn: Turn): string {
  const messages = turn.request.messages;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'user') {
      return contentText(messages[i].content);
    }
  }
  return '';
}

function assistantText(turn: Turn): string {
  return turn.response.content
    .filter((b) => b.type === 'text')
    .map(blockText)
    .filter(Boolean)
    .join('\n\n');
}

export function TimelineView({ sessionId }: Props) {
  const [session, setSession] = useState<SessionDetail | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    setSummary(null);
    api.getSession(sessionId).then((d) => { setSession(d); setLoading(false); })
      .catch(() => setLoading(false));
    api.getSummary(sessionId).then(setSummary).catch(() => {});
  }, [sessionId]);

  if (loading) {
    return (
      <div class="flex items-center justify-center h-full text-text-4">
        <div class="animate-pulse">加载中...</div>
      </div>
    );
  }
  if (!session) {
    return (
      <div class="flex items-center justify-center h-full text-text-4 text-sm">
        选择一个会话查看详情
      </div>
    );
  }

  const turns = session.turns;
  const totalTokens = turns.reduce((sum, t) => sum + (t.response.usage?.total_tokens ?? 0), 0);
  const toolCount = turns.reduce((sum, t) => sum + (t.response.tool_calls?.length ?? 0), 0);
  const assistantTurns = turns.filter((t) => assistantText(t).length > 0).length;

  return (
    <div class="h-full overflow-y-auto scroll-thin px-8 py-6">
      {summary && summary.trim().length > 0 && (
        <div class="bg-bg-card border border-border rounded-xl p-4 mb-6">
          <div class="text-[10px] text-text-4 uppercase tracking-wider mb-2">会话摘要（规则提取）</div>
          <div class="text-sm text-text-2 whitespace-pre-line leading-relaxed">{summary}</div>
        </div>
      )}
      <div class="bg-gradient-to-br from-bg-card to-bg-elev border border-border rounded-xl p-5 mb-6 relative overflow-hidden">
        <div class="grid grid-cols-5 relative z-[1]">
          <Stat value={turns.length} label="轮次" color="#6366f5" />
          <Stat value={turns.length} label="用户" color="#3b82f6" />
          <Stat value={assistantTurns} label="AI" color="#22c55e" />
          <Stat value={toolCount} label="工具" color="#06b6d4" />
          <Stat value={totalTokens} label="Tokens" color="#f59b0b" />
        </div>
      </div>

      {turns.map((turn) => (
        <div key={turn.id}>
          <MessageCard
            message={{
              role: 'user',
              content: lastUserText(turn),
              timestamp: turn.timestamp.slice(11, 19),
            }}
          />
          {assistantText(turn) && (
            <MessageCard
              message={{
                role: 'assistant',
                content: assistantText(turn),
                model: turn.request.model ?? undefined,
                tokens: turn.response.usage?.total_tokens ?? undefined,
                timestamp: turn.timestamp.slice(11, 19),
              }}
            />
          )}
          {(turn.response.tool_calls ?? []).map((call, i) => (
            <div key={`${turn.id}-tool-${i}`} class="ml-7 mb-4">
              <ToolCallCard
                call={{
                  name: call.function.name,
                  arguments: (call.function.arguments ?? {}) as Record<string, unknown>,
                }}
              />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

function Stat({ value, label, color }: { value: number; label: string; color: string }) {
  return (
    <div class="text-center relative">
      <div class="text-xl font-bold tabular-nums" style={{ color }}>
        {value.toLocaleString()}
      </div>
      <div class="text-[10px] text-text-4 uppercase tracking-wider mt-0.5">{label}</div>
    </div>
  );
}
