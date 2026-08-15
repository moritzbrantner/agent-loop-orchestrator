import { z } from "zod";

const providerSchema = z.enum(["codex", "claude"]);
const runStatusSchema = z.enum([
  "preparing",
  "running",
  "evaluating",
  "awaiting_decision",
  "integrating",
  "completed",
  "failed",
  "cancelled",
]);
const workItemStatusSchema = z.enum([
  "open",
  "running",
  "awaiting_decision",
  "approved",
  "rejected",
  "failed",
  "cancelled",
]);

export const outputLineSchema = z.object({
  source: z.enum(["stdout", "stderr"]),
  text: z.string(),
  receivedAt: z.string(),
});

const baselineSchema = z.object({ gitSha: z.string(), ref: z.string().optional() });
const workItemSchema = z.object({
  id: z.string(),
  projectId: z.string(),
  repositoryRoot: z.string(),
  title: z.string(),
  prompt: z.string(),
  declaredScope: z.array(z.string()),
  baseline: baselineSchema,
  targetBranch: z.string(),
  status: workItemStatusSchema,
  runId: z.string().nullable(),
  createdAt: z.string(),
});

const candidateSchema = z.object({
  candidateId: z.string(),
  gitSha: z.string().optional(),
  baselineGitSha: z.string(),
  changedPaths: z.array(z.string()),
});
const checkSchema = z.object({
  checkId: z.string(),
  capability: z.string(),
  component: z.string().optional(),
  outcome: z.enum(["passed", "failed", "unavailable", "skipped", "error"]),
  required: z.boolean(),
  reason: z.string().optional(),
});
const runContractSchema = z.object({
  baseline: baselineSchema,
  candidates: z.array(candidateSchema),
  checks: z.array(checkSchema),
  decisions: z.array(z.object({ decision: z.enum(["approved", "rejected", "changes-requested"]), reason: z.string().optional() })),
  publications: z.array(z.object({ kind: z.string(), status: z.string(), candidateIdentity: z.string() })),
});
const runSchema = z.object({
  id: z.string(),
  workItemId: z.string(),
  projectId: z.string(),
  provider: providerSchema,
  status: runStatusSchema,
  targetBranch: z.string(),
  worktreePath: z.string(),
  startedAt: z.string(),
  finishedAt: z.string().nullable(),
  output: z.array(outputLineSchema),
  error: z.string().nullable(),
  contract: runContractSchema,
});

const dashboardSchema = z.object({
  projects: z.array(z.object({
    id: z.string(),
    path: z.string(),
    defaultProvider: providerSchema,
    targetBranch: z.string(),
  })),
  workItems: z.array(workItemSchema),
  runs: z.array(runSchema),
  activeRunId: z.string().nullable(),
});

const eventSchema = z.object({
  kind: z.enum(["state", "output"]),
  runId: z.string().optional().nullable(),
  line: outputLineSchema.optional().nullable(),
});

export type Dashboard = z.infer<typeof dashboardSchema>;
export type WorkItem = z.infer<typeof workItemSchema>;
export type Run = z.infer<typeof runSchema>;
export type EventMessage = z.infer<typeof eventSchema>;
export type Provider = z.infer<typeof providerSchema>;

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
  }
}

const errorSchema = z.object({ error: z.string() });

async function request<T>(path: string, token: string, schema: z.ZodType<T>, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: {
      Authorization: `Bearer ${token}`,
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const body = errorSchema.safeParse(await response.json().catch(() => ({})));
    throw new ApiError(body.success ? body.data.error : "The service rejected this request.", response.status);
  }
  if (response.status === 204) return undefined as T;
  return schema.parse(await response.json());
}

export function getDashboard(token: string) {
  return request("/api/dashboard", token, dashboardSchema);
}

export function createWorkItem(token: string, input: {
  projectId: string;
  title: string;
  prompt: string;
  declaredScope: string[];
}) {
  return request("/api/work-items", token, workItemSchema, {
    method: "POST",
    body: JSON.stringify(input),
  });
}

export function startWorkItem(token: string, id: string, provider: Provider | null) {
  return request(`/api/work-items/${id}/start`, token, z.object({ disposition: z.literal("started"), workItemId: z.string() }), {
    method: "POST",
    body: JSON.stringify({ provider }),
  });
}

export function decideRun(token: string, id: string, decision: "approve" | "reject") {
  return request(`/api/runs/${id}/decision`, token, runSchema, {
    method: "POST",
    body: JSON.stringify({ decision }),
  });
}

export function cancelRun(token: string, id: string) {
  return request(`/api/runs/${id}/cancel`, token, z.undefined(), { method: "POST" });
}

export function parseEvent(value: string): EventMessage | null {
  const result = eventSchema.safeParse(JSON.parse(value));
  return result.success ? result.data : null;
}
