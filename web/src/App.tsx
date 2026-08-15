import { FormEvent, type Dispatch, type SetStateAction, useCallback, useEffect, useRef, useState } from "react";

import {
  ApiError,
  Dashboard,
  EventMessage,
  PendingRun,
  Provider,
  Run,
  cancelRun,
  discardPending,
  getDashboard,
  outputLineSchema,
  parseEvent,
  savePending,
  startPending,
  submitRun,
} from "./api";

const sessionTokenKey = "agent-loop-access-token";

type Composer = {
  projectId: string;
  provider: "" | Provider;
  prompt: string;
};

const emptyComposer: Composer = { projectId: "", provider: "", prompt: "" };

export function App() {
  const [token, setToken] = useState(() => sessionStorage.getItem(sessionTokenKey) ?? "");
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [composer, setComposer] = useState<Composer>(emptyComposer);
  const [editingPending, setEditingPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const previousActive = useRef<string | null>(null);

  const loadDashboard = useCallback(async () => {
    if (!token) return;
    try {
      const next = await getDashboard(token);
      const wasActive = previousActive.current;
      previousActive.current = next.activeRunId;
      setDashboard(next);
      setComposer((current) => current.projectId || !next.projects.length
        ? current
        : { ...current, projectId: next.projects[0].id });
      if (wasActive && !next.activeRunId && Notification.permission === "granted") {
        new Notification("Agent Loop run finished", { body: "Your next run is ready to start or revise." });
      }
      setError(null);
    } catch (reason) {
      setError(messageFor(reason));
      if (reason instanceof ApiError && reason.status === 401) {
        sessionStorage.removeItem(sessionTokenKey);
        setToken("");
      }
    }
  }, [token]);

  useEffect(() => { void loadDashboard(); }, [loadDashboard]);

  useEffect(() => {
    if (!token) return;
    const controller = new AbortController();
    const connect = async () => {
      try {
        const response = await fetch("/api/events", {
          headers: { Authorization: `Bearer ${token}` },
          signal: controller.signal,
        });
        if (!response.ok || !response.body) throw new Error("Could not connect to live updates.");
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = "";
        while (!controller.signal.aborted) {
          const next = await reader.read();
          if (next.done) break;
          buffer += decoder.decode(next.value, { stream: true });
          const events = buffer.split("\n\n");
          buffer = events.pop() ?? "";
          for (const rawEvent of events) handleEvent(rawEvent, setDashboard, loadDashboard);
        }
      } catch (reason) {
        if (!controller.signal.aborted) setError(messageFor(reason));
      }
    };
    void connect();
    return () => controller.abort();
  }, [token, loadDashboard]);

  const unlock = async (submittedToken: string) => {
    sessionStorage.setItem(sessionTokenKey, submittedToken);
    setToken(submittedToken);
  };

  const submitComposer = async (event: FormEvent) => {
    event.preventDefault();
    if (!token) return;
    setWorking(true);
    try {
      const request = { ...composer, provider: composer.provider || null };
      if (editingPending) await savePending(token, request);
      else await submitRun(token, request);
      setComposer((current) => ({ ...current, prompt: "" }));
      setEditingPending(false);
      await loadDashboard();
    } catch (reason) {
      setError(messageFor(reason));
    } finally {
      setWorking(false);
    }
  };

  if (!token) return <Unlock onUnlock={unlock} />;

  return (
    <main className="shell">
      <header className="masthead">
        <div><p className="eyebrow">LOCAL AGENT CONTROL</p><h1>Agent Loop</h1></div>
        <button className="quiet" onClick={() => void Notification.requestPermission()}>Enable desktop notifications</button>
      </header>
      {error && <p className="error" role="alert">{error}</p>}
      {!dashboard ? <p className="loading">Connecting to your local control plane…</p> : <>
        <section className="overview">
          <div><span>Registered projects</span><strong>{dashboard.projects.length}</strong></div>
          <div><span>Run history</span><strong>{dashboard.runs.length}</strong></div>
          <div><span>Active run</span><strong>{dashboard.activeRunId ? "1" : "None"}</strong></div>
        </section>
        <section className="grid">
          <RunComposer
            dashboard={dashboard}
            composer={composer}
            working={working}
            editingPending={editingPending}
            onChange={setComposer}
            onSubmit={submitComposer}
          />
          <PendingCard
            pending={dashboard.pending}
            active={Boolean(dashboard.activeRunId)}
            onStart={async () => { try { await startPending(token); await loadDashboard(); } catch (reason) { setError(messageFor(reason)); } }}
            onEdit={(pending) => { setComposer(toComposer(pending)); setEditingPending(true); }}
            onDiscard={async () => { try { await discardPending(token); await loadDashboard(); } catch (reason) { setError(messageFor(reason)); } }}
          />
        </section>
        <section className="runs">
          <div className="section-heading"><div><p className="eyebrow">DURABLE HISTORY</p><h2>Runs</h2></div></div>
          {dashboard.runs.length === 0 ? <EmptyRuns /> : dashboard.runs.map((run) => <RunCard key={run.id} run={run} onCancel={async () => {
            if (!confirm("Stop this active run? Its output will be kept.")) return;
            try { await cancelRun(token, run.id); } catch (reason) { setError(messageFor(reason)); }
          }} />)}
        </section>
      </>}
    </main>
  );
}

function Unlock({ onUnlock }: { onUnlock: (token: string) => Promise<void> }) {
  const [token, setToken] = useState("");
  return <main className="unlock"><form onSubmit={(event) => { event.preventDefault(); void onUnlock(token); }}><p className="eyebrow">SECURE LOCAL SERVICE</p><h1>Unlock Agent Loop</h1><p>Enter the shared access token for this dashboard session.</p><input aria-label="Access token" type="password" autoFocus value={token} onChange={(event) => setToken(event.target.value)} required /><button>Unlock dashboard</button></form></main>;
}

function RunComposer({ dashboard, composer, working, editingPending, onChange, onSubmit }: {
  dashboard: Dashboard; composer: Composer; working: boolean; editingPending: boolean;
  onChange: (composer: Composer) => void; onSubmit: (event: FormEvent) => void;
}) {
  const active = Boolean(dashboard.activeRunId);
  return <section className="card composer"><p className="eyebrow">{active ? "NEXT UP" : "NEW RUN"}</p><h2>{active ? "Save the next run" : "Start a run"}</h2>
    {dashboard.projects.length === 0 ? <p>No projects yet. Run <code>agent-loop init</code> in a Git repository, then refresh.</p> : <form onSubmit={onSubmit}>
      <label>Project<select value={composer.projectId} onChange={(event) => onChange({ ...composer, projectId: event.target.value })}>{dashboard.projects.map((project) => <option key={project.id} value={project.id}>{project.id} · {project.path}</option>)}</select></label>
      <label>Provider<select value={composer.provider} onChange={(event) => onChange({ ...composer, provider: event.target.value as "" | Provider })}><option value="">Project default</option><option value="codex">Codex</option><option value="claude">Claude</option></select></label>
      <label>Prompt<textarea rows={6} value={composer.prompt} onChange={(event) => onChange({ ...composer, prompt: event.target.value })} placeholder="Describe the coding work to do…" required /></label>
      <button disabled={working}>{editingPending ? "Update pending run" : active ? "Save as pending" : "Start run"}</button>
    </form>}
  </section>;
}

function PendingCard({ pending, active, onStart, onEdit, onDiscard }: { pending: PendingRun | null; active: boolean; onStart: () => void; onEdit: (pending: PendingRun) => void; onDiscard: () => void }) {
  return <section className="card pending"><p className="eyebrow">PENDING RUN</p><h2>{pending ? pending.projectId : "Nothing saved"}</h2>{pending ? <><p className="prompt-preview">{pending.prompt}</p><p className="meta">{pending.provider ?? "Project default"} · saved {formatDate(pending.savedAt)}</p><div className="actions">{!active && <button onClick={onStart}>Start now</button>}<button className="quiet" onClick={() => onEdit(pending)}>Revise</button><button className="quiet danger" onClick={onDiscard}>Discard</button></div></> : <p>Save one upcoming run while an active run is in progress.</p>}</section>;
}

function RunCard({ run, onCancel }: { run: Run; onCancel: () => void }) {
  return <article className="run-card"><div className="run-title"><span className={`status ${run.status}`}>{run.status}</span><strong>{run.projectId}</strong><span className="meta">{run.provider} · {formatDate(run.startedAt)}</span>{run.status === "running" && <button className="quiet danger" onClick={onCancel}>Stop run</button>}</div><p>{run.prompt}</p>{run.error && <p className="error">{run.error}</p>}<details><summary>Output ({run.output.length} lines)</summary><pre>{run.output.map((line) => `[${line.source}] ${line.text}`).join("\n") || "No output yet."}</pre></details></article>;
}

function EmptyRuns() { return <div className="empty"><h3>No runs recorded</h3><p>Your completed, failed, and cancelled runs will remain here until you choose to remove them in a later version.</p></div>; }

function handleEvent(rawEvent: string, setDashboard: Dispatch<SetStateAction<Dashboard | null>>, refresh: () => Promise<void>) {
  const data = rawEvent.split("\n").find((line) => line.startsWith("data: "))?.slice(6);
  if (!data) return;
  let event: EventMessage | null = null;
  try { event = parseEvent(data); } catch { return; }
  if (!event) return;
  if (event.kind === "state") { void refresh(); return; }
  const line = outputLineSchema.safeParse(event.line);
  if (!line.success || !event.runId) return;
  setDashboard((current) => current && ({ ...current, runs: current.runs.map((run) => run.id === event.runId ? { ...run, output: [...run.output, line.data] } : run) }));
}

function messageFor(reason: unknown) { return reason instanceof Error ? reason.message : "Something unexpected went wrong."; }
function formatDate(value: string) { return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value)); }
function toComposer(pending: PendingRun): Composer { return { projectId: pending.projectId, provider: pending.provider ?? "", prompt: pending.prompt }; }
