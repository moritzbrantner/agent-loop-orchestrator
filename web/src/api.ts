import { z } from "zod";

const providerSchema = z.enum(["codex", "claude"]);
const statusSchema = z.enum(["running", "completed", "failed", "cancelled", "interrupted"]);

export const outputLineSchema = z.object({
  source: z.enum(["stdout", "stderr"]),
  text: z.string(),
  receivedAt: z.string(),
});

const runSchema = z.object({
  id: z.string(),
  projectId: z.string(),
  provider: providerSchema,
  prompt: z.string(),
  status: statusSchema,
  startedAt: z.string(),
  finishedAt: z.string().nullable(),
  output: z.array(outputLineSchema),
  error: z.string().nullable(),
});

const pendingSchema = z.object({
  projectId: z.string(),
  provider: providerSchema.nullable(),
  prompt: z.string(),
  savedAt: z.string(),
});

const dashboardSchema = z.object({
  projects: z.array(z.object({
    id: z.string(),
    path: z.string(),
    defaultProvider: providerSchema,
  })),
  runs: z.array(runSchema),
  pending: pendingSchema.nullable(),
  activeRunId: z.string().nullable(),
});

const eventSchema = z.object({
  kind: z.enum(["state", "output"]),
  runId: z.string().optional(),
  line: outputLineSchema.optional().nullable(),
});

export type Dashboard = z.infer<typeof dashboardSchema>;
export type PendingRun = z.infer<typeof pendingSchema>;
export type Run = z.infer<typeof runSchema>;
export type OutputLine = z.infer<typeof outputLineSchema>;
export type EventMessage = z.infer<typeof eventSchema>;
export type Provider = z.infer<typeof providerSchema>;

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
  }
}

const errorSchema = z.object({ error: z.string() });

async function request<T>(
  path: string,
  token: string,
  schema: z.ZodType<T>,
  init?: RequestInit,
): Promise<T> {
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

export function submitRun(token: string, runRequest: { projectId: string; provider: Provider | null; prompt: string }) {
  return request("/api/runs", token, z.object({ disposition: z.enum(["started", "saved"]), run: runSchema.nullable(), pending: pendingSchema.nullable() }), {
    method: "POST",
    body: JSON.stringify(runRequest),
  });
}

export function savePending(token: string, pendingRequest: { projectId: string; provider: Provider | null; prompt: string }) {
  return request("/api/pending", token, pendingSchema, { method: "PUT", body: JSON.stringify(pendingRequest) });
}

export function startPending(token: string) {
  return request("/api/pending/start", token, runSchema, { method: "POST" });
}

export function discardPending(token: string) {
  return request("/api/pending", token, z.undefined(), { method: "DELETE" });
}

export function cancelRun(token: string, id: string) {
  return request(`/api/runs/${id}/cancel`, token, z.undefined(), { method: "POST" });
}

export function parseEvent(value: string): EventMessage | null {
  const result = eventSchema.safeParse(JSON.parse(value));
  return result.success ? result.data : null;
}
