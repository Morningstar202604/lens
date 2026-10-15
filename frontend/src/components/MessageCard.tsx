import { Markdown } from './Markdown';

interface Props {
  role: 'user' | 'assistant' | 'system' | string;
  content: string;
  timestamp?: string;
  tokens?: number;
  model?: string;
}

const ROLE_META: Record<string, { label: string; mark: string }> = {
  user: { label: '用户', mark: '›' },
  assistant: { label: '助手', mark: '✦' },
  system: { label: '系统', mark: '·' },
};

/** 一条消息。角色用极小色标 + 左侧细线区分，不用整块彩色卡片。 */
export function MessageCard({ role, content, timestamp, tokens, model }: Props) {
  const meta = ROLE_META[role] ?? { label: role, mark: '·' };

  return (
    <article class={`mb-3 pl-3 border-l-2 ${role === 'user' ? 'border-l-white/12' : 'border-l-primary/35'}`}>
      <div class="flex items-center gap-2.5 mb-1 text-micro">
        <span class={`font-medium ${role === 'user' ? 'text-text-3' : 'text-primary/80'}`}>
          {meta.mark} {meta.label}
        </span>
        {model && <span class="font-mono text-text-4 truncate max-w-[240px]">{model}</span>}
        {tokens != null && (
          <span class="font-mono text-text-4 tabular-nums">{tokens.toLocaleString()} tok</span>
        )}
        {timestamp && <span class="font-mono text-text-4 tabular-nums ml-auto">{timestamp}</span>}
      </div>
      <div class={role === 'user' ? 'text-text-2' : 'text-text'}>
        <Markdown source={content} />
      </div>
    </article>
  );
}
