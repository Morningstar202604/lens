/** 前端类型出口。
 *
 * 接口类型全部来自 ts-rs 生成（唯一真相：Rust 结构，见 frontend/src/generated/）。
 * 本文件只做转出与 UI 视图模型的补充定义，不再手写接口镜像。
 */

export type { Session } from './generated/Session';
export type { Session as SessionSummary } from './generated/Session';
export type { SessionDetail } from './generated/SessionDetail';
export type { Turn } from './generated/Turn';
export type { LlmRequest } from './generated/LlmRequest';
export type { LlmResponse } from './generated/LlmResponse';
export type { Message } from './generated/Message';
export type { MessageContent } from './generated/MessageContent';
export type { ContentBlock } from './generated/ContentBlock';
export type { ToolCall } from './generated/ToolCall';
export type { TokenUsage } from './generated/TokenUsage';
export type { Stats } from './generated/Stats';
export type { CountStat } from './generated/CountStat';
export type { GraphNode } from './generated/GraphNode';
export type { GraphEdge } from './generated/GraphEdge';
export type { Provider } from './generated/Provider';

export { AGENT_COLORS, AGENT_LABELS } from './agents';

import type { GraphNode } from './generated/GraphNode';
import type { GraphEdge } from './generated/GraphEdge';

/** 图谱接口返回的聚合结构（两个生成类型的组合）。 */
export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** 时间线视图模型（由 Turn 派生，非 API 契约）。 */
export interface ViewMessage {
  role: string;
  content: string;
  model?: string;
  tokens?: number;
  timestamp: string;
}

export interface ViewToolCall {
  name: string;
  arguments: Record<string, unknown>;
}
