import { FormEvent, type Dispatch, type SetStateAction, useCallback, useEffect, useRef, useState } from "react";

import {
  ApiError,
  Dashboard,
  EventMessage,
  Provider,
  Run,
  WorkItem,
  cancelRun,
  createWorkItem,
  decideRun,
  getDashboard,
  outputLineSchema,
  parseEvent,
  startWorkItem,
} from "./api";

const sessionTokenKey = "agent-loop-access-token";
type Composer = { projectId: string; title: string; prompt: string; scope: string };
const emptyComposer: Composer = { projectId: "", title: "", prompt: "", scope: "." };

export function App() {
  const [token, setToken] = useState(() => sessionStorage.getItem(sessionTokenKey) ?? "");
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [composer, setComposer] = useState<Composer>(emptyComposer);
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
        new Notification("Agent Loop run finished", { body: "The candidate is ready to inspect or the run has stopped." });
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
        const response = await fetch("/api/events", { headers: { Authorization: `Bearer ${token}` }, signal: controller.signal });
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

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setWorking(true);
    try {
      await createWorkItem(token, {
        projectId: composer.projectId,
        title: composer.title,
        prompt: composer.prompt,
        declaredScope: composer.scope.split(",").map((scope) => scope.trim()).filter(Boolean),
      });
      setComposer((current) => ({ ...current, title: "", prompt: "", scope: "." }));
      await loadDashboard();
    } catch (reason) {
      setError(messageFor(reason));
    } finally {
      setWorking(false);
    }
  };

  if (!token) return <Unlock onUnlock={async (submitted) => { sessionStorage.setItem(sessionTokenKey, submitted); setToken(submitted); }} />;
  return <main className="shell">
    <header className="masthead"><div><p className="eyebrow">LOCAL AGENT CONTROL</p><h1>Agent Loop</h1></div><button className="quiet" onClick={() => void Notification.requestPermission()}>Enable desktop notifications</button></header>
    {error && <p className="error" role="alert">{error}</p>}
    {!dashboard ? <p className="loading">Connecting to your local control plane…</p> : <>
      <section className="overview">
        <div><span>Work items</span><strong>{dashboard.workItems.length}</strong></div>
        <div><span>Run history</span><strong>{dashboard.runs.length}</strong></div>
        <div><span>Active run</span><strong>{dashboard.activeRunId ? "1" : "None"}</strong></div>
      </section>
      <section className="grid">
        <WorkItemComposer dashboard={dashboard} composer={composer} working={working} onChange={setComposer} onSubmit={submit} />
        <section className="card"><p className="eyebrow">EXECUTION BOUNDARY</p><h2>Local only</h2><p>Every attempt starts from its exact baseline in a clean detached worktree. Approval can update only the configured local target branch; no remote is pushed or published.</p></section>
      </section>
      <section className="runs"><div className="section-heading"><div><p className="eyebrow">DURABLE REQUESTS</p><h2>Work items</h2></div></div>
        {dashboard.workItems.length === 0 ? <Empty label="No work items yet." /> : dashboard.workItems.map((item) => <WorkItemCard key={item.id} item={item} active={Boolean(dashboard.activeRunId)} onStart={async (provider) => {
          try { await startWorkItem(token, item.id, provider); await loadDashboard(); } catch (reason) { setError(messageFor(reason)); }
        }} />)}
      </section>
      <section className="runs"><div className="section-heading"><div><p className="eyebrow">CANDIDATES & EVIDENCE</p><h2>Runs</h2></div></div>
        {dashboard.runs.length === 0 ? <Empty label="No runs recorded." /> : dashboard.runs.map((run) => <RunCard key={run.id} run={run} onDecision={async (decision) => {
          try { await decideRun(token, run.id, decision); await loadDashboard(); } catch (reason) { setError(messageFor(reason)); }
        }} onCancel={async () => { try { await cancelRun(token, run.id); } catch (reason) { setError(messageFor(reason)); } }} />)}
      </section>
    </>}
  </main>;
}

function Unlock({ onUnlock }: { onUnlock: (token: string) => Promise<void> }) {
  const [token, setToken] = useState("");
  return <main className="unlock"><form onSubmit={(event) => { event.preventDefault(); void onUnlock(token); }}><p className="eyebrow">SECURE LOCAL SERVICE</p><h1>Unlock Agent Loop</h1><p>Enter the shared access token for this dashboard session.</p><input aria-label="Access token" type="password" autoFocus value={token} onChange={(event) => setToken(event.target.value)} required /><button>Unlock dashboard</button></form></main>;
}

function WorkItemComposer({ dashboard, composer, working, onChange, onSubmit }: { dashboard: Dashboard; composer: Composer; working: boolean; onChange: (value: Composer) => void; onSubmit: (event: FormEvent) => void }) {
  return <section className="card composer"><p className="eyebrow">NEW WORK ITEM</p><h2>Define a bounded change</h2>
    {dashboard.projects.length === 0 ? <p>No projects yet. Run <code>agent-loop init</code> in a Git repository, then refresh.</p> : <form onSubmit={onSubmit}>
      <label>Project<select value={composer.projectId} onChange={(event) => onChange({ ...composer, projectId: event.target.value })}>{dashboard.projects.map((project) => <option key={project.id} value={project.id}>{project.id} · {project.path}</option>)}</select></label>
      <label>Title<input value={composer.title} onChange={(event) => onChange({ ...composer, title: event.target.value })} required /></label>
      <label>Prompt<textarea rows={6} value={composer.prompt} onChange={(event) => onChange({ ...composer, prompt: event.target.value })} required /></label>
      <label>Write scope (comma-separated paths)<input value={composer.scope} onChange={(event) => onChange({ ...composer, scope: event.target.value })} required /></label>
      <button disabled={working}>Create work item</button>
    </form>}
  </section>;
}

function WorkItemCard({ item, active, onStart }: { item: WorkItem; active: boolean; onStart: (provider: Provider | null) => void }) {
  return <article className="run-card"><div className="run-title"><span className={`status ${item.status}`}>{item.status.replace("_", " ")}</span><strong>{item.title}</strong><span className="meta">{item.projectId} · {shortSha(item.baseline.gitSha)} → {item.targetBranch}</span></div><p>{item.prompt}</p><p className="meta">Scope: {item.declaredScope.join(", ")}</p>{item.status === "open" && <div className="actions"><button disabled={active} onClick={() => onStart(null)}>Start default</button><button className="quiet" disabled={active} onClick={() => onStart("codex")}>Codex</button><button className="quiet" disabled={active} onClick={() => onStart("claude")}>Claude</button></div>}</article>;
}

function RunCard({ run, onDecision, onCancel }: { run: Run; onDecision: (decision: "approve" | "reject") => void; onCancel: () => void }) {
  const candidate = run.contract.candidates.at(-1);
  return <article className="run-card"><div className="run-title"><span className={`status ${run.status}`}>{run.status.replace("_", " ")}</span><strong>{run.projectId}</strong><span className="meta">{run.provider} · {formatDate(run.startedAt)}</span>{run.status === "running" && <button className="quiet danger" onClick={onCancel}>Stop run</button>}</div>
    {candidate && <p className="meta">Candidate {shortSha(candidate.gitSha ?? candidate.candidateId)} from {shortSha(candidate.baselineGitSha)} · {candidate.changedPaths.join(", ")}</p>}
    {run.error && <p className="error">{run.error}</p>}
    {run.status === "awaiting_decision" && <div className="actions"><button onClick={() => onDecision("approve")}>Approve & integrate</button><button className="quiet danger" onClick={() => onDecision("reject")}>Reject</button></div>}
    <details open={run.contract.checks.some((check) => check.outcome !== "passed")}><summary>Checks ({run.contract.checks.length})</summary>{run.contract.checks.length === 0 ? <p className="meta">No check results yet.</p> : <ul>{run.contract.checks.map((check) => <li key={check.checkId}><strong>{check.outcome}</strong> · {check.capability}{check.component ? ` (${check.component})` : ""}{check.reason ? ` — ${check.reason}` : ""}</li>)}</ul>}</details>
    <details><summary>Provider output ({run.output.length} lines)</summary><pre>{run.output.map((line) => `[${line.source}] ${line.text}`).join("\n") || "No output yet."}</pre></details>
  </article>;
}

function Empty({ label }: { label: string }) { return <div className="empty"><p>{label}</p></div>; }
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
function shortSha(value: string) { return value.slice(0, 10); }
