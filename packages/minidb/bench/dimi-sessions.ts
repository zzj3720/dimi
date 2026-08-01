import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

export interface DimiWorkspace {
  name: string;
  root: string;
  created_at?: string;
  last_opened_at?: string;
}

export interface DimiSessionState {
  id?: string;
  cwd?: string;
  title?: string;
  lastPrompt?: string;
  createdAt?: number;
  updatedAt?: number;
}

export interface DimiSession {
  sessionId: string;
  sessionDir: string;
  workspaceId: string;
  workDir: string;
  state: DimiSessionState;
}

export function loadDimiWorkspaces(homeDir: string): Record<string, DimiWorkspace> {
  const file = JSON.parse(readFileSync(path.join(homeDir, 'workspaces.json'), 'utf8')) as {
    workspaces?: Record<string, DimiWorkspace>;
  };
  return file.workspaces ?? {};
}

export function listDimiSessions(homeDir: string): DimiSession[] {
  const sessionsDir = path.join(homeDir, 'sessions');
  const sessions: DimiSession[] = [];
  for (const workspace of readdirSync(sessionsDir, { withFileTypes: true })) {
    if (!workspace.isDirectory()) continue;
    const workspaceDir = path.join(sessionsDir, workspace.name);
    for (const entry of readdirSync(workspaceDir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const sessionDir = path.join(workspaceDir, entry.name);
      let state: DimiSessionState;
      try {
        state = JSON.parse(readFileSync(path.join(sessionDir, 'state.json'), 'utf8')) as DimiSessionState;
      } catch {
        continue;
      }
      sessions.push({
        sessionId: state.id ?? entry.name,
        sessionDir,
        workspaceId: workspace.name,
        workDir: state.cwd ?? '',
        state,
      });
    }
  }
  return sessions;
}
