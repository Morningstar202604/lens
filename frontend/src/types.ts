/** Domain types mirrored from the Rust backend. */

export interface SessionSummary {
  id: string;
  title: string;
  model: string | null;
  agent_type: string;
  turn_count: number;
  token_count: number;
  created_at: string;
  updated_at: string;
}

export interface Message {
  role: 'user' | 'assistant' | 'system';
  content: string;
  model?: string;
  tokens?: number;
  timestamp: string;
}

export interface ToolCall {
  name: string;
  arguments: Record<string, unknown>;
}

export interface SessionDetail {
  id: string;
  title: string;
  project_path: string;
  created_at: string;
  updated_at: string;
  messages: Message[];
  tool_calls: ToolCall[];
}

export interface Stats {
  agents: { name: string; count: number }[];
  models: { name: string; count: number }[];
  total_sessions: number;
  total_tokens: number;
  total_turns: number;
}

export interface GraphNode {
  id: string;
  label: string;
  kind: string;
  weight: number;
}

export interface GraphEdge {
  source: string;
  target: string;
  relation: string;
  weight: number;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** Agent display metadata. */
export const AGENT_COLORS: Record<string, string> = {
  opencode: '#6366f5',
  codex: '#f59b0b',
  'claude-code': '#3b82f6',
  trae: '#06b6d4',
  'kimi-code': '#a855f7',
  unknown: '#6b7280',
};

export const AGENT_LABELS: Record<string, string> = {
  opencode: 'OpenCode',
  codex: 'Codex',
  'claude-code': 'Claude Code',
  trae: 'Trae',
  'kimi-code': 'Kimi Code',
  unknown: '未知',
};
