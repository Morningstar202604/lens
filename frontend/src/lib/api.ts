import type { SessionSummary, SessionDetail, Stats, GraphData } from '../types';

const BASE = '/api';

async function getJSON<T>(path: string): Promise<T> {
  const res = await fetch(BASE + path);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

export const api = {
  listSessions: () =>
    getJSON<{ sessions: SessionSummary[] }>('/sessions').then((r) => r.sessions),

  searchSessions: (q: string) =>
    getJSON<{ sessions: SessionSummary[] }>(
      `/sessions/search?q=${encodeURIComponent(q)}`,
    ).then((r) => r.sessions),

  getSession: (id: string) =>
    getJSON<{ session: SessionDetail }>(`/sessions/${id}`).then((r) => r.session),

  getStats: () => getJSON<Stats>('/stats'),

  getGraph: (id: string) => getJSON<GraphData>(`/sessions/${id}/graph`),

  getGlobalGraph: () => getJSON<GraphData>('/graph'),

  getSummary: (id: string) =>
    getJSON<{ summary: string }>(`/sessions/${id}/summary`).then((r) => r.summary),
};
