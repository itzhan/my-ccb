import type { Account } from '@/api';

export interface FormState {
  name: string;
  email: string;
  auth_type: 'setup_token' | 'oauth';
  setup_token: string;
  access_token: string;
  refresh_token: string;
  expires_at: string;
  proxy_url: string;
  billing_mode: string;
  account_uuid: string;
  organization_uuid: string;
  subscription_type: string;
  concurrency: number;
  priority: number;
  auto_telemetry: boolean;
  rpm_limit: number;
  identity_mode: string;
  virtual_user: string;
  virtual_git_name: string;
  recapture_days: number;
  max_sessions: number;
  allowed_client_types: string[];
}

export const clientTypeOptions = [
  { value: 'cli', label: 'cli 终端' },
  { value: 'vscode', label: 'VSCode 插件' },
  { value: 'sdk', label: 'Agent SDK' },
  { value: 'desktop', label: '桌面三方' },
  { value: 'other', label: '其它/非CC' },
];

export const subscriptionOptions = [
  { value: '', label: '未设置' },
  { value: 'max', label: 'Max' },
  { value: 'pro', label: 'Pro' },
  { value: 'team', label: 'Team' },
  { value: 'enterprise', label: 'Enterprise' },
];

export function emptyForm(): FormState {
  return {
    name: '', email: '', auth_type: 'setup_token',
    setup_token: '', access_token: '', refresh_token: '', expires_at: '',
    proxy_url: '', billing_mode: 'strip',
    account_uuid: '', organization_uuid: '', subscription_type: '',
    concurrency: 3, priority: 50, auto_telemetry: false, rpm_limit: 0,
    identity_mode: 'passthrough', virtual_user: '', virtual_git_name: '',
    recapture_days: 0, max_sessions: 3, allowed_client_types: [],
  };
}

export function formFromAccount(a: Account): FormState {
  return {
    name: a.name,
    email: a.email,
    auth_type: (a.auth_type as FormState['auth_type']) || 'setup_token',
    setup_token: '', access_token: '', refresh_token: '',
    expires_at: a.expires_at ? String(a.expires_at) : '',
    proxy_url: a.proxy_url,
    billing_mode: a.billing_mode || 'strip',
    account_uuid: a.account_uuid || '',
    organization_uuid: a.organization_uuid || '',
    subscription_type: a.subscription_type || '',
    concurrency: a.concurrency,
    priority: a.priority,
    auto_telemetry: a.auto_telemetry ?? false,
    rpm_limit: a.rpm_limit || 0,
    identity_mode: a.identity_mode || 'passthrough',
    virtual_user: a.virtual_user || '',
    virtual_git_name: a.virtual_git_name || '',
    recapture_days: a.recapture_days ?? 0,
    max_sessions: a.max_sessions ?? 3,
    allowed_client_types: (a.allowed_client_types || '').split(',').map((s) => s.trim()).filter(Boolean),
  };
}
