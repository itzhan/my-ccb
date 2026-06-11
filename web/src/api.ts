const BASE = ''

let authToken = ''

export function setAuth(token: string) {
  authToken = token
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(BASE + path, {
    method,
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${authToken}`,
    },
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(err.error || res.statusText)
  }
  return res.json()
}

export interface Account {
  id: number
  name: string
  email: string
  status: string
  auth_type: string
  setup_token: string
  access_token: string
  refresh_token: string
  expires_at?: number | null
  oauth_refreshed_at?: string
  auth_error?: string
  proxy_url: string
  device_id: string
  canonical_env?: Record<string, unknown>
  canonical_prompt_env?: Record<string, unknown>
  canonical_process?: {
    constrained_memory?: number
    rss_range?: number[]
    heap_total_range?: number[]
    heap_used_range?: number[]
  }
  billing_mode: string
  account_uuid?: string | null
  organization_uuid?: string | null
  subscription_type?: string | null
  concurrency: number
  priority: number
  auto_telemetry: boolean
  telemetry_count: number
  telemetry_expires_at?: string
  rate_limited_at?: string
  rate_limit_reset_at?: string
  disable_reason?: string
  rpm_limit?: number | null
  current_rpm?: number | null
  current_concurrency?: number
  max_sessions?: number
  current_sessions?: number
  current_devices?: number
  current_window_sessions?: number
  allowed_client_types?: string
  window_5h_cost_cap_usd?: number | null
  cost_5h_usd?: number
  usage_data?: UsageData
  usage_fetched_at?: string
  identity_mode?: string
  virtual_user?: string
  virtual_git_name?: string
  path_mode?: string
  session_mode?: string
  device_quota?: number
  session_quota?: number
  warmup_skip?: boolean
  recapture_days?: number
  identity_captured_at?: string | null
  captured_session_id?: string
  captured_session_at?: string | null
  effective_identity?: {
    device_id: string
    virtual_user: string
    git_name: string
    platform: string
    arch: string
  }
  created_at: string
  updated_at: string
}

export interface PagedResult<T> {
  data: T[]
  total: number
  page: number
  page_size: number
  total_pages: number
}

export interface UsageWindow {
  utilization: number
  resets_at: string
}

export interface UsageData {
  five_hour?: UsageWindow
  seven_day?: UsageWindow
  seven_day_sonnet?: UsageWindow
}

export interface ApiToken {
  id: number;
  name: string;
  token: string;
  allowed_accounts: string;
  blocked_accounts: string;
  status: string;
  /** customer（默认，客户用）/ warmup（养号专用） */
  category: string;
  concurrency: number;
  expires_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface WarmupTask {
  id: number;
  name: string;
  /** 逗号分隔的 warmup 令牌 ID */
  token_ids: string;
  msg_interval_secs: number;
  total_duration_secs: number;
  work_duration_secs: number;
  rest_duration_secs: number;
  jitter_pct: number;
  model: string;
  status: string;
  error: string;
  messages_sent: number;
  started_at?: string | null;
  ends_at?: string | null;
  last_message_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface UsageLog {
  id: number;
  token_id: number;
  account_id: number;
  request_id: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  cache_creation_5m_tokens: number;
  cache_creation_1h_tokens: number;
  stream: boolean;
  status_code: number;
  duration_ms: number;
  error: string;
  client_ip: string;
  user_agent: string;
  path: string;
  session_id: string;
  user_id: string;
  proxy: string;
  req_headers: string;
  resp_headers: string;
  created_at: string;
}

export interface UsageStat {
  key: string;
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
  req_count: number;
}

export interface Dashboard {
  accounts: { total: number; active: number; error: number; disabled: number };
  tokens: number;
}

export interface OAuthGenerateResult {
  auth_url: string;
  session_id: string;
}

export interface OAuthExchangeResult {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  expires_at: number;
  scope: string;
  account_uuid: string;
  organization_uuid: string;
  email_address: string;
}

export interface SettingsResp {
  client_restriction: string;
  thinking_repair?: string;
  warmup_enabled?: string;
  warmup_schedule?: string;
}

export const api = {
  listAccounts: (page = 1, pageSize = 12) =>
    request<PagedResult<Account>>('GET', `/admin/accounts?page=${page}&page_size=${pageSize}`),
  createAccount: (a: Partial<Account>) => request<Account>('POST', '/admin/accounts', a),
  updateAccount: (id: number, a: Partial<Account>) => request<Account>('PUT', `/admin/accounts/${id}`, a),
  deleteAccount: (id: number) => request<void>('DELETE', `/admin/accounts/${id}`),
  testAccount: (id: number) => request<{ status: string; message?: string }>('POST', `/admin/accounts/${id}/test`),
  refreshUsage: (id: number) => request<{ status: string; usage?: UsageData; message?: string }>('POST', `/admin/accounts/${id}/usage`),
  listTokens: (page = 1, pageSize = 20) =>
    request<PagedResult<ApiToken>>('GET', `/admin/tokens?page=${page}&page_size=${pageSize}`),
  createToken: (t: Partial<ApiToken>) => request<ApiToken>('POST', '/admin/tokens', t),
  updateToken: (id: number, t: Partial<ApiToken>) => request<ApiToken>('PUT', `/admin/tokens/${id}`, t),
  deleteToken: (id: number) => request<void>('DELETE', `/admin/tokens/${id}`),
  // 养号
  listWarmupTasks: () => request<{ data: WarmupTask[] }>('GET', '/admin/warmup/tasks'),
  createWarmupTask: (t: Partial<WarmupTask>) => request<WarmupTask>('POST', '/admin/warmup/tasks', t),
  updateWarmupTask: (id: number, t: Partial<WarmupTask>) => request<WarmupTask>('PUT', `/admin/warmup/tasks/${id}`, t),
  deleteWarmupTask: (id: number) => request<void>('DELETE', `/admin/warmup/tasks/${id}`),
  startWarmupTask: (id: number) => request<{ status: string }>('POST', `/admin/warmup/tasks/${id}/start`),
  stopWarmupTask: (id: number) => request<{ status: string }>('POST', `/admin/warmup/tasks/${id}/stop`),
  listWarmupTokens: () => request<{ data: ApiToken[] }>('GET', '/admin/warmup/tokens'),
  ensureWarmupTokens: (accountIds: number[]) =>
    request<{ data: ApiToken[] }>('POST', '/admin/warmup/ensure-tokens', { account_ids: accountIds }),
  getWarmupLogs: (page = 1, pageSize = 50) =>
    request<PagedResult<UsageLog>>('GET', `/admin/warmup/logs?page=${page}&page_size=${pageSize}`),
  warmupQuestionsCount: () => request<{ count: number }>('GET', '/admin/warmup/questions/count'),

  getDashboard: () => request<Dashboard>('GET', '/admin/dashboard'),
  getSettings: () => request<SettingsResp>('GET', '/admin/settings'),
  updateSettings: (s: { client_restriction?: string; thinking_repair?: string; warmup_enabled?: string; warmup_schedule?: string }) =>
    request<SettingsResp>('PUT', '/admin/settings', s),

  getUsageLogs: (params: { token_id?: number; account_id?: number; model?: string; result?: string; start?: string; end?: string; page?: number; page_size?: number } = {}) => {
    const qs = Object.entries(params)
      .filter(([, v]) => v !== undefined && v !== null && v !== '')
      .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
      .join('&')
    return request<PagedResult<UsageLog>>('GET', `/admin/usage/logs${qs ? '?' + qs : ''}`)
  },
  getUsageStats: (params: { group_by?: string; start?: string; end?: string } = {}) => {
    const qs = Object.entries(params)
      .filter(([, v]) => v !== undefined && v !== null && v !== '')
      .map(([k, v]) => `${k}=${encodeURIComponent(String(v))}`)
      .join('&')
    return request<{ data: UsageStat[] }>('GET', `/admin/usage/stats${qs ? '?' + qs : ''}`)
  },

  generateAuthUrl: (proxyUrl?: string) =>
    request<OAuthGenerateResult>('POST', '/admin/oauth/generate-auth-url', { proxy_url: proxyUrl || null }),
  generateSetupTokenUrl: (proxyUrl?: string) =>
    request<OAuthGenerateResult>('POST', '/admin/oauth/generate-setup-token-url', { proxy_url: proxyUrl || null }),
  exchangeCode: (sessionId: string, code: string) =>
    request<OAuthExchangeResult>('POST', '/admin/oauth/exchange-code', { session_id: sessionId, code }),
  exchangeSetupTokenCode: (sessionId: string, code: string) =>
    request<OAuthExchangeResult>('POST', '/admin/oauth/exchange-setup-token-code', { session_id: sessionId, code }),
  exchangeSessionKey: (sessionKey: string, proxyUrl?: string) =>
    request<OAuthExchangeResult>('POST', '/admin/oauth/exchange-session-key', { session_key: sessionKey, proxy_url: proxyUrl || null }),
}
