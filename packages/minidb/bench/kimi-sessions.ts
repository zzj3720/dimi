import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

export interface KimiWorkspace {
  name: string;
  root: string;
  created_at?: string;
  last_opened_at?: string;
}

export interface KimiSessionState {
  id?: string;
  cwd?: string;
  title?: string;
  lastPrompt?: string;
  createdAt?: number;
  updatedAt?: number;
}

export interface KimiSession {
  sessionId: string;
  sessionDir: string;
  workspaceId: string;
  workDir: string;
  state: KimiSessionState;
}

export function loadKimiWorkspaces(homeDir: string): Record<string, KimiWorkspace> {
  const file = JSON.parse(readFileSync(path.join(homeDir, 'workspaces.json'), 'utf8')) as {
    workspaces?: Record<string, KimiWorkspace>;
  };
  return file.workspaces ?? {};
}

export function listKimiSessions(homeDir: string): KimiSession[] {
  const sessionsDir = path.join(homeDir, 'sessions');
  const sessions: KimiSession[] = [];
  for (const workspace of readdirSync(sessionsDir, { withFileTypes: true })) {
    if (!workspace.isDirectory()) continue;
    const workspaceDir = path.join(sessionsDir, workspace.name);
    for (const entry of readdirSync(workspaceDir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const sessionDir = path.join(workspaceDir, entry.name);
      let state: KimiSessionState;
      try {
        state = JSON.parse(readFileSync(path.join(sessionDir, 'state.json'), 'utf8')) as KimiSessionState;
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
