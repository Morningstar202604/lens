import type { Message } from '../types';
import { Markdown } from './Markdown';

interface Props {
  message: Message;
}

const roleLabels: Record<string, string> = {
  user: '用户',
  assistant: 'AI',
  system: '系统',
};

const roleColors: Record<string, string> = {
  user: '#3b82f6',
  assistant: '#22c55e',
  system: '#6b7280',
};

export function MessageCard({ message }: Props) {
  const dotColor = roleColors[message.role] || '#6b7280';

  return (
    <div class="flex gap-4 mb-6 animate-[fadeIn_0.25s_ease-out]">
      {/* Axis dot */}
      <div class="flex flex-col items-center w-7 flex-shrink-0">
        <span
          class="w-2.5 h-2.5 rounded-full flex-shrink-0 z-[1]"
          style={{
            background: dotColor,
            boxShadow: `0 0 8px ${dotColor}55`,
          }}
        />
        <div class="w-0.5 flex-1 bg-gradient-to-b from-border to-transparent mt-1 min-h-[20px]" />
      </div>

      {/* Card */}
      <div class="flex-1 min-w-0 bg-bg-card border border-border rounded-lg overflow-hidden hover:border-border-hover transition-colors">
        <div class="flex items-center gap-2 px-3.5 py-2 bg-bg-elev border-b border-border-dim">
          <span class="text-xs font-bold uppercase tracking-wider" style={{ color: dotColor }}>
            {roleLabels[message.role] || message.role}
          </span>
          {message.model && (
            <span class="text-[10px] text-text-4 bg-bg px-1.5 py-0.5 rounded">
              {message.model}
            </span>
          )}
          {message.tokens != null && (
            <span class="inline-flex items-center gap-1 text-xs font-semibold px-2 py-0.5 rounded-full bg-primary-soft text-primary tabular-nums">
              {message.tokens.toLocaleString()} tok
            </span>
          )}
          <span class="ml-auto text-xs text-text-4 tabular-nums">{message.timestamp}</span>
        </div>
        <div class="px-3.5 py-3">
          <Markdown source={message.content} />
        </div>
      </div>
    </div>
  );
}
