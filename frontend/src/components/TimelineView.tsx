import type { SessionDetail, Message, ToolCall } from '../types';
import { MessageCard } from './MessageCard';
import { ToolCallCard } from './ToolCallCard';
import { api } from '../lib/api';
import { useState, useEffect } from 'preact/hooks';

interface Props {
  sessionId: string;
}

export function TimelineView({ sessionId }: Props) {
  const [session, setSession] = useState<SessionDetail | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    api.getSession(sessionId).then(d => { setSession(d); setLoading(false); }).catch(() => setLoading(false));
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

  // Group messages with their tool calls
  const items: Array<{ type: 'message'; data: Message } | { type: 'tool'; data: ToolCall }> = [];
  let toolIdx = 0;
  for (const msg of session.messages) {
    items.push({ type: 'message', data: msg });
    // Attach tool calls after assistant messages
    if (msg.role === 'assistant' && toolIdx < session.tool_calls.length) {
      items.push({ type: 'tool', data: session.tool_calls[toolIdx] });
      toolIdx++;
    }
  }
  // Remaining tool calls
  while (toolIdx < session.tool_calls.length) {
    items.push({ type: 'tool', data: session.tool_calls[toolIdx] });
    toolIdx++;
  }

  const totalTokens = session.messages.reduce((s, m) => s + (m.tokens || 0), 0);
  const userTurns = session.messages.filter((m) => m.role === 'user').length;
  const assistantTurns = session.messages.filter((m) => m.role === 'assistant').length;

  return (
    <div class="h-full overflow-y-auto scroll-thin px-8 py-6">
      {/* Hero stats */}
      <div class="bg-gradient-to-br from-bg-card to-bg-elev border border-border rounded-xl p-5 mb-6 relative overflow-hidden">
        <div class="grid grid-cols-5 relative z-[1]">
          <Stat value={session.messages.length} label="消息" color="#6366f5" />
          <Stat value={userTurns} label="用户" color="#3b82f6" />
          <Stat value={assistantTurns} label="AI" color="#22c55e" />
          <Stat value={session.tool_calls.length} label="工具" color="#06b6d4" />
          <Stat value={totalTokens} label="Tokens" color="#f59b0b" />
        </div>
      </div>

      {/* Timeline items */}
      {items.map((item, i) =>
        item.type === 'message' ? (
          <MessageCard key={`msg-${i}`} message={item.data} />
        ) : (
          <div key={`tool-${i}`} class="ml-7 mb-4">
            <ToolCallCard call={item.data} />
          </div>
        ),
      )}
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
