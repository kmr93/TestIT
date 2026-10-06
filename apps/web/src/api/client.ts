const API_BASE = '/api/v1';

function csrfToken(): string | undefined {
  if (typeof document === 'undefined') return undefined;
  const prefix = 'testit_csrf=';
  const item = document.cookie.split(';').map((value) => value.trim()).find((value) => value.startsWith(prefix));
  return item ? decodeURIComponent(item.slice(prefix.length)) : undefined;
}

export async function apiRequest<T>(endpoint: string, options?: RequestInit): Promise<T> {
  const headers = new Headers(options?.headers);
  if (options?.body && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  if (options?.method && !['GET', 'HEAD', 'OPTIONS'].includes(options.method.toUpperCase())) {
    const token = csrfToken();
    if (token) headers.set('X-CSRF-Token', token);
  }
  const res = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers,
    credentials: 'same-origin',
  });

  if (!res.ok) {
    const errorText = await res.text();
    let msg = `Request failed (${res.status})`;
    try {
      const parsed = JSON.parse(errorText);
      msg = parsed.error || msg;
    } catch (_) {
      msg = errorText || msg;
    }
    throw new Error(msg);
  }

  return res.json();
}

export async function downloadProjectBundle(): Promise<Blob> {
  const response = await fetch(`${API_BASE}/portability/export`, {
    credentials: 'same-origin',
  });
  if (!response.ok) {
    const errorText = await response.text();
    let message = `Request failed (${response.status})`;
    try {
      message = JSON.parse(errorText).error || message;
    } catch (_) {
      message = errorText || message;
    }
    throw new Error(message);
  }
  return response.blob();
}

export const previewProjectBundle = (file: File) =>
  apiRequest<any>('/portability/preview', {
    method: 'POST',
    headers: { 'Content-Type': 'application/zip' },
    body: file,
  });

export const importProjectBundle = (file: File) =>
  apiRequest<any>('/portability/import', {
    method: 'POST',
    headers: { 'Content-Type': 'application/zip' },
    body: file,
  });

export const login = (email: string, password: string) =>
  apiRequest<any>('/auth/login', { method: 'POST', body: JSON.stringify({ email, password }) });
export const logout = () => apiRequest<{ status: string }>('/auth/logout', { method: 'POST' });
export const getCurrentUser = () => apiRequest<any>('/me');
export const getUsers = () => apiRequest<any[]>('/admin/users');
export const createUser = (data: { email: string; display_name: string; role: string; password: string }) =>
  apiRequest<any>('/admin/users', { method: 'POST', body: JSON.stringify(data) });
export const updateUser = (id: string, data: { display_name?: string; role?: string; active?: boolean; password?: string }) =>
  apiRequest<any>(`/admin/users/${id}`, { method: 'PATCH', body: JSON.stringify(data) });

// Assets
export const getAssets = () => apiRequest<any[]>('/assets');
export const createAsset = (data: { kind: string; name: string; description?: string; initial_draft?: any }) =>
  apiRequest<any>('/assets', { method: 'POST', body: JSON.stringify(data) });
export const getAssetDraft = (id: string) => apiRequest<any>(`/assets/${id}/draft`);
export const updateAssetDraft = (id: string, data: { name?: string; description?: string; draft_json: any; expected_draft_version: number }) =>
  apiRequest<any>(`/assets/${id}/draft`, { method: 'PATCH', body: JSON.stringify(data) });
export const publishAssetRevision = (id: string, data: { expected_draft_version: number; change_note?: string }) =>
  apiRequest<any>(`/assets/${id}/publish`, { method: 'POST', body: JSON.stringify(data) });
export const getAssetRevisions = (id: string) => apiRequest<any[]>(`/assets/${id}/revisions`);
export const getAssetRevision = (assetId: string, revisionId: string) => apiRequest<any>(`/assets/${assetId}/revisions/${revisionId}`);

// Runs
export const getRuns = () => apiRequest<any[]>('/runs');
export const getRun = (id: string) => apiRequest<any>(`/runs/${id}`);
export const getRunStats = (id: string) => apiRequest<any>(`/runs/${id}/stats`);
export const triggerRun = (data: { suite_revision_id: string; environment_id: string; inputs?: any }) =>
  apiRequest<any>('/runs', { method: 'POST', body: JSON.stringify(data) });
export const cancelRun = (id: string) => apiRequest<any>(`/runs/${id}/cancel`, { method: 'POST' });
export const rerunFailed = (id: string) => apiRequest<any>(`/runs/${id}/rerun-failed`, { method: 'POST' });
export const getRunComparison = (id: string) => apiRequest<any>(`/runs/${id}/comparison`);

// Variables
export const previewVariables = (data: { node_config: any; environment_id?: string; sample_inputs?: any; sample_iteration?: any; suite_variables?: any; case_variables?: any }) =>
  apiRequest<any>('/variables/preview', { method: 'POST', body: JSON.stringify(data) });

// Environments & Connections & Secrets
export const getEnvironments = () => apiRequest<any[]>('/environments');
export const createEnvironment = (data: { name: string; description?: string; variables: any }) =>
  apiRequest<any>('/environments', { method: 'POST', body: JSON.stringify(data) });
export const getConnections = () => apiRequest<any[]>('/connections');
export const createConnection = (data: { name: string; connector_type: string; settings: any; secret_refs?: any }) =>
  apiRequest<any>('/connections', { method: 'POST', body: JSON.stringify(data) });
export const updateConnection = (id: string, data: { name: string; connector_type: string; settings: any; secret_refs?: any }) =>
  apiRequest<any>(`/connections/${id}`, { method: 'PUT', body: JSON.stringify(data) });
export const testConnection = (id: string) => apiRequest<any>(`/connections/${id}/test`, { method: 'POST' });
export const getSecrets = () => apiRequest<any[]>('/secrets');
export const createSecret = (data: { name: string; plaintext: string }) =>
  apiRequest<any>('/secrets', { method: 'POST', body: JSON.stringify(data) });

// OpenAPI
export const validateOpenApi = (spec_content: string) =>
  apiRequest<any>('/specifications/openapi/validate', { method: 'POST', body: JSON.stringify({ spec_content }) });
export const importOpenApi = (spec_content: string, selected_operation_ids: string[]) =>
  apiRequest<any>('/specifications/openapi/import', { method: 'POST', body: JSON.stringify({ spec_content, selected_operation_ids }) });
