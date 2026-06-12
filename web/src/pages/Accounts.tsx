import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Plus, KeyRound, Users, Search, Pencil, Trash2, X, Unlock, Copy, Check } from 'lucide-react';
import { api, type Account, type ApiToken } from '@/api';
import { useToast } from '@/components/Toaster';
import { useDashboardRefresh } from '@/components/Layout';
import { cn } from '@/lib/utils';
import { isRateLimited, statusStyle, usageBarColor, formatTimeLeft, sortAccounts } from '@/lib/format';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from '@/components/ui/table';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { BlurFade } from '@/components/magic/blur-fade';
import { AccountFormDialog } from '@/components/accounts/AccountFormDialog';
import { OAuthFlowDialog } from '@/components/accounts/OAuthFlowDialog';
import { emptyForm, formFromAccount, type FormState } from '@/components/accounts/form';

const PAGE_SIZE = 10;
type StatusFilter = 'all' | 'active' | 'ratelimited' | 'error' | 'disabled';

function rpmColor(a: Account): string {
  if (a.rpm_limit && a.rpm_limit > 0) {
    const r = (a.current_rpm || 0) / a.rpm_limit;
    if (r >= 0.8) return 'text-red-600';
    if (r >= 0.5) return 'text-amber-600';
    return (a.current_rpm || 0) > 0 ? 'text-emerald-600' : 'text-muted-foreground';
  }
  return (a.current_rpm || 0) > 0 ? 'text-emerald-600' : 'text-muted-foreground';
}

function costRatio(a: Account): number {
  if (!a.window_5h_cost_cap_usd || a.window_5h_cost_cap_usd <= 0) return 0;
  return Math.min(1, (a.cost_5h_usd || 0) / a.window_5h_cost_cap_usd);
}

function costColor(ratio: number, hasCap: boolean): string {
  if (!hasCap) return 'text-neutral-600';
  if (ratio >= 1) return 'text-red-600';
  if (ratio >= 0.85) return 'text-red-500';
  if (ratio >= 0.6) return 'text-amber-600';
  if (ratio > 0) return 'text-emerald-600';
  return 'text-muted-foreground';
}

function costBarColor(ratio: number): string {
  if (ratio >= 1) return 'bg-red-600';
  if (ratio >= 0.85) return 'bg-red-500';
  if (ratio >= 0.6) return 'bg-amber-500';
  return 'bg-emerald-500';
}

export default function Accounts() {
  const toast = useToast();
  const refreshDashboard = useDashboardRefresh();

  const [allAccounts, setAllAccounts] = useState<Account[]>([]);
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [page, setPage] = useState(1);

  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<Account | null>(null);
  const [form, setForm] = useState<FormState>(emptyForm());

  const [showOAuth, setShowOAuth] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [deleteTargetId, setDeleteTargetId] = useState<number | null>(null);

  const [refreshingUsage, setRefreshingUsage] = useState<number | null>(null);

  // 多选 + 批量操作
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [bulkEditOpen, setBulkEditOpen] = useState(false);
  const [bulkDeleteOpen, setBulkDeleteOpen] = useState(false);
  const [bulkBusy, setBulkBusy] = useState(false);
  const [bulkEnabled, setBulkEnabled] = useState<Set<string>>(new Set());
  const [bulkVals, setBulkVals] = useState({
    status: 'active', concurrency: 10, max_sessions: 10, priority: 50,
    device_quota: 0, session_quota: 0, rpm_limit: 0, window_5h_cost_cap_usd: 0,
    warmup_skip: true, identity_mode: 'normalize',
    allowed_client_types: ['cli', 'vscode'] as string[],
    proxy_url: '', billing_mode: 'strip', session_mode: '',
  });

  // 批量创建令牌(可用账号锁定为所选账号)
  const [bulkTokenOpen, setBulkTokenOpen] = useState(false);
  const [bulkTokenForm, setBulkTokenForm] = useState({ name: '', category: 'customer', concurrency: 0, expires_at: '' });
  const [createdToken, setCreatedToken] = useState<string | null>(null);
  const [tokenCopied, setTokenCopied] = useState(false);

  // 拉全量账号(后端单页上限 100,这里按需翻页拼全),供前端搜索/筛选/分页
  const load = useCallback(async () => {
    try {
      const first = await api.listAccounts(1, 100);
      let all = first.data ?? [];
      if (first.total_pages > 1) {
        const rest = await Promise.all(
          Array.from({ length: first.total_pages - 1 }, (_, i) => api.listAccounts(i + 2, 100)),
        );
        all = all.concat(...rest.map((r) => r.data ?? []));
      }
      setAllAccounts(all);
    } catch {
      setAllAccounts([]);
    }
  }, []);

  // 拉全量令牌,用于账号列展示该账号被哪些令牌绑定(调试用)
  const [allTokens, setAllTokens] = useState<ApiToken[]>([]);
  const loadTokens = useCallback(async () => {
    try { setAllTokens((await api.listTokens(1, 100)).data ?? []); } catch { setAllTokens([]); }
  }, []);

  // 账号 ID -> 绑定它的令牌列表(令牌 allowed_accounts 含该账号)
  const tokensByAccount = useMemo(() => {
    const m = new Map<number, ApiToken[]>();
    for (const t of allTokens) {
      for (const s of (t.allowed_accounts || '').split(',')) {
        const id = Number(s.trim());
        if (!s.trim() || Number.isNaN(id)) continue;
        if (!m.has(id)) m.set(id, []);
        m.get(id)!.push(t);
      }
    }
    return m;
  }, [allTokens]);

  // 立即加载 + 每 8s 轮询
  useEffect(() => {
    load(); loadTokens();
    const id = setInterval(load, 8000);
    return () => clearInterval(id);
  }, [load, loadTokens]);

  // 搜索/筛选变化时回到第一页
  useEffect(() => { setPage(1); }, [search, statusFilter]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const matchStatus = (a: Account): boolean => {
      switch (statusFilter) {
        case 'active': return a.status === 'active' && !isRateLimited(a);
        case 'ratelimited': return isRateLimited(a);
        case 'error': return a.status === 'error';
        case 'disabled': return a.status === 'disabled';
        default: return true;
      }
    };
    const matchSearch = (a: Account): boolean => {
      if (!q) return true;
      return [a.name, a.email, a.account_uuid || ''].some((f) => f?.toLowerCase().includes(q));
    };
    return sortAccounts(allAccounts.filter((a) => matchStatus(a) && matchSearch(a)));
  }, [allAccounts, search, statusFilter]);

  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const curPage = Math.min(page, totalPages);
  const pageItems = filtered.slice((curPage - 1) * PAGE_SIZE, curPage * PAGE_SIZE);

  const visiblePages = useMemo(() => {
    const pages: number[] = [];
    let start = Math.max(1, curPage - 2);
    const end = Math.min(totalPages, start + 4);
    start = Math.max(1, end - 4);
    for (let i = start; i <= end; i++) pages.push(i);
    return pages;
  }, [curPage, totalPages]);

  const patch = (p: Partial<FormState>) => setForm((f) => ({ ...f, ...p }));

  function openCreate() { setEditing(null); setForm(emptyForm()); setShowForm(true); }
  function openEdit(a: Account) { setEditing(a); setForm(formFromAccount(a)); setShowForm(true); }

  async function save() {
    try {
      const expiresAt = form.expires_at.trim();
      if (editing) {
        if (form.auth_type === 'setup_token' && !form.setup_token.trim() && editing.auth_type !== 'setup_token')
          throw new Error('切换到 Setup Token 模式时必须填写 Setup Token');
        if (form.auth_type === 'oauth' && !form.refresh_token.trim() && editing.auth_type !== 'oauth')
          throw new Error('切换到 OAuth 模式时必须填写 Refresh Token');
        const u: Record<string, unknown> = {};
        if (form.name) u.name = form.name;
        if (form.email) u.email = form.email;
        u.auth_type = form.auth_type;
        if (form.setup_token) u.setup_token = form.setup_token;
        if (form.access_token) u.access_token = form.access_token;
        if (form.refresh_token) u.refresh_token = form.refresh_token;
        if (expiresAt) u.expires_at = Number(expiresAt);
        u.proxy_url = form.proxy_url;
        u.billing_mode = form.billing_mode;
        u.account_uuid = form.account_uuid || null;
        u.organization_uuid = form.organization_uuid || null;
        u.subscription_type = form.subscription_type || null;
        u.concurrency = form.concurrency;
        u.priority = form.priority;
        u.auto_telemetry = form.auto_telemetry;
        u.rpm_limit = form.rpm_limit || 0;
        u.identity_mode = form.identity_mode;
        u.virtual_user = form.virtual_user;
        u.virtual_git_name = form.virtual_git_name;
        u.path_mode = form.path_mode;
        u.session_mode = form.session_mode;
        u.device_quota = Math.max(0, Number(form.device_quota) || 0);
        u.session_quota = Math.max(0, Number(form.session_quota) || 0);
        u.warmup_skip = form.warmup_skip;
        u.recapture_days = Number(form.recapture_days) || 0;
        u.max_sessions = Math.max(0, Number(form.max_sessions) || 0);
        u.allowed_client_types = form.allowed_client_types.join(',');
        u.window_5h_cost_cap_usd = Math.max(0, Number(form.window_5h_cost_cap_usd) || 0);
        await api.updateAccount(editing.id, u);
      } else {
        if (form.auth_type === 'setup_token' && !form.setup_token.trim()) throw new Error('Setup Token 不能为空');
        if (form.auth_type === 'oauth' && !form.refresh_token.trim()) throw new Error('Refresh Token 不能为空');
        const p: Record<string, unknown> = {
          name: form.name, email: form.email, auth_type: form.auth_type,
          setup_token: form.setup_token, access_token: form.access_token, refresh_token: form.refresh_token,
          proxy_url: form.proxy_url, billing_mode: form.billing_mode,
          account_uuid: form.account_uuid || null, organization_uuid: form.organization_uuid || null,
          subscription_type: form.subscription_type || null,
          concurrency: form.concurrency, priority: form.priority, auto_telemetry: form.auto_telemetry,
          rpm_limit: form.rpm_limit || 0, identity_mode: form.identity_mode,
          virtual_user: form.virtual_user, virtual_git_name: form.virtual_git_name,
          path_mode: form.path_mode,
          session_mode: form.session_mode,
          device_quota: Math.max(0, Number(form.device_quota) || 0),
          session_quota: Math.max(0, Number(form.session_quota) || 0),
          warmup_skip: form.warmup_skip,
          recapture_days: Number(form.recapture_days) || 0,
          max_sessions: Math.max(0, Number(form.max_sessions) || 0),
          allowed_client_types: form.allowed_client_types.join(','),
          window_5h_cost_cap_usd: Math.max(0, Number(form.window_5h_cost_cap_usd) || 0),
        };
        if (expiresAt) p.expires_at = Number(expiresAt);
        await api.createAccount(p);
      }
      setShowForm(false);
      await load();
      refreshDashboard();
    } catch (e) {
      toast((e as Error).message || '保存失败');
    }
  }

  async function executeDelete() {
    if (deleteTargetId === null) return;
    try {
      await api.deleteAccount(deleteTargetId);
      setShowDelete(false);
      setDeleteTargetId(null);
      await load();
      refreshDashboard();
    } catch (e) {
      toast((e as Error).message || '删除失败');
    }
  }

  async function refreshUsage(id: number) {
    setRefreshingUsage(id);
    try {
      const res = await api.refreshUsage(id);
      if (res.status === 'ok' && res.usage) {
        setAllAccounts((prev) => prev.map((a) => a.id === id ? { ...a, usage_data: res.usage, usage_fetched_at: new Date().toISOString() } : a));
      } else if (res.status === 'error') {
        toast(res.message || '刷新用量失败');
      }
    } catch (e) {
      toast((e as Error).message || '刷新用量失败');
    }
    setRefreshingUsage(null);
  }

  async function toggleScheduling(a: Account) {
    try {
      const stopped = a.status === 'disabled' || isRateLimited(a);
      const res = await api.updateAccount(a.id, { status: stopped ? 'active' : 'disabled' });
      setAllAccounts((prev) => prev.map((x) => x.id === a.id
        ? { ...x, status: res.status, disable_reason: res.disable_reason ?? '', rate_limited_at: res.rate_limited_at, rate_limit_reset_at: res.rate_limit_reset_at }
        : x));
      refreshDashboard();
    } catch (e) {
      toast((e as Error).message || '切换调度失败');
    }
  }

  // 重置：把账号(含被封禁/限流)清回正常可调用状态(status=active，清空停用原因与限流窗口)。
  async function resetAccount(a: Account) {
    try {
      const res = await api.updateAccount(a.id, { status: 'active' });
      setAllAccounts((prev) => prev.map((x) => x.id === a.id
        ? { ...x, status: res.status, disable_reason: res.disable_reason ?? '', rate_limited_at: res.rate_limited_at, rate_limit_reset_at: res.rate_limit_reset_at }
        : x));
      refreshDashboard();
      toast('已重置为正常状态');
    } catch (e) {
      toast((e as Error).message || '重置失败');
    }
  }

  function applyOAuth(f: FormState) { setEditing(null); setForm(f); setShowForm(true); }

  // --- 多选 / 批量 ---
  const toggleSelect = (id: number) => setSelected((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const allFilteredSelected = filtered.length > 0 && filtered.every((a) => selected.has(a.id));
  const toggleSelectAll = () => setSelected((s) => {
    const n = new Set(s);
    if (filtered.length > 0 && filtered.every((a) => n.has(a.id))) filtered.forEach((a) => n.delete(a.id));
    else filtered.forEach((a) => n.add(a.id));
    return n;
  });
  const clearSel = () => setSelected(new Set());
  const toggleBulkField = (k: string) => setBulkEnabled((s) => { const n = new Set(s); if (n.has(k)) n.delete(k); else n.add(k); return n; });
  const setBulkVal = (p: Partial<typeof bulkVals>) => setBulkVals((v) => ({ ...v, ...p }));
  const bulkRow = (k: string, label: string, control: ReactNode) => (
    <div className="flex items-center gap-2">
      <input type="checkbox" className="h-4 w-4 flex-shrink-0 accent-indigo-600" checked={bulkEnabled.has(k)} onChange={() => toggleBulkField(k)} />
      <span className={cn('w-24 flex-shrink-0 text-xs', bulkEnabled.has(k) ? 'text-neutral-700' : 'text-neutral-400')}>{label}</span>
      <div className={cn('flex-1', !bulkEnabled.has(k) && 'pointer-events-none opacity-40')}>{control}</div>
    </div>
  );

  // 并发执行(池),返回成功/失败计数
  async function runPool(ids: number[], fn: (id: number) => Promise<void>, c = 5) {
    let i = 0, ok = 0, fail = 0;
    await Promise.all(Array.from({ length: Math.min(c, ids.length) }, async () => {
      while (i < ids.length) { const id = ids[i++]; try { await fn(id); ok++; } catch { fail++; } }
    }));
    return { ok, fail };
  }

  async function executeBulkDelete() {
    const ids = [...selected];
    setBulkBusy(true);
    const { ok, fail } = await runPool(ids, (id) => api.deleteAccount(id));
    setBulkBusy(false); setBulkDeleteOpen(false); clearSel();
    await load(); refreshDashboard();
    toast(`批量删除完成:成功 ${ok}${fail ? `,失败 ${fail}` : ''}`, fail ? 'error' : 'success');
  }

  // 批量创建令牌:可用账号 = 所选账号,其余配置弹窗里正常填
  function openBulkToken() {
    setBulkTokenForm({ name: '', category: 'customer', concurrency: 0, expires_at: '' });
    setCreatedToken(null); setTokenCopied(false);
    setBulkTokenOpen(true);
  }

  async function createBulkToken() {
    try {
      setBulkBusy(true);
      const t = await api.createToken({
        name: bulkTokenForm.name,
        category: bulkTokenForm.category,
        allowed_accounts: [...selected].sort((x, y) => x - y).join(','),
        blocked_accounts: '',
        concurrency: Number(bulkTokenForm.concurrency) || 0,
        expires_at: bulkTokenForm.expires_at ? new Date(bulkTokenForm.expires_at).toISOString() : null,
      });
      setCreatedToken(t.token);
      toast('令牌创建成功', 'success');
      loadTokens();
    } catch (e) {
      toast((e as Error).message || '创建失败', 'error');
    } finally {
      setBulkBusy(false);
    }
  }

  async function copyCreatedToken() {
    if (!createdToken) return;
    try {
      if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(createdToken);
      else {
        const ta = document.createElement('textarea'); ta.value = createdToken; ta.style.position = 'fixed'; ta.style.opacity = '0';
        document.body.appendChild(ta); ta.select(); document.execCommand('copy'); document.body.removeChild(ta);
      }
      setTokenCopied(true); setTimeout(() => setTokenCopied(false), 2000);
    } catch { toast('复制失败'); }
  }

  // 一键放开客户端限制(allowed_client_types 置空 = 全部放行)
  async function bulkUnrestrictClients() {
    const ids = [...selected];
    setBulkBusy(true);
    const { ok, fail } = await runPool(ids, (id) => api.updateAccount(id, { allowed_client_types: '' }).then(() => undefined));
    setBulkBusy(false); clearSel();
    await load(); refreshDashboard();
    toast(`已放开客户端限制:成功 ${ok}${fail ? `,失败 ${fail}` : ''}`, fail ? 'error' : 'success');
  }

  async function applyBulkEdit() {
    const p: Record<string, unknown> = {};
    for (const k of bulkEnabled) {
      if (k === 'allowed_client_types') p.allowed_client_types = bulkVals.allowed_client_types.join(',');
      else p[k] = (bulkVals as Record<string, unknown>)[k];
    }
    if (Object.keys(p).length === 0) { toast('请至少勾选一个要修改的项'); return; }
    const ids = [...selected];
    setBulkBusy(true);
    const { ok, fail } = await runPool(ids, (id) => api.updateAccount(id, p).then(() => undefined));
    setBulkBusy(false); setBulkEditOpen(false); clearSel();
    await load(); refreshDashboard();
    toast(`批量编辑完成:成功 ${ok}${fail ? `,失败 ${fail}` : ''}`, fail ? 'error' : 'success');
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-semibold text-neutral-900">账号管理</h2>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => setShowOAuth(true)}><KeyRound className="h-4 w-4" /> 授权登录</Button>
          <Button onClick={openCreate}><Plus className="h-4 w-4" /> 添加账号</Button>
        </div>
      </div>

      {/* 搜索 + 筛选 */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative w-full max-w-xs">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-neutral-400" />
          <Input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="搜索名称 / 邮箱 / UUID" className="pl-9" />
        </div>
        <Select value={statusFilter} onValueChange={(v) => setStatusFilter(v as StatusFilter)}>
          <SelectTrigger className="w-32"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="all">全部状态</SelectItem>
            <SelectItem value="active">活跃</SelectItem>
            <SelectItem value="ratelimited">限流中</SelectItem>
            <SelectItem value="error">异常</SelectItem>
            <SelectItem value="disabled">停用</SelectItem>
          </SelectContent>
        </Select>
        <span className="ml-auto text-sm text-neutral-500">共 {filtered.length} 个账号</span>
      </div>

      {/* 批量操作条 */}
      {selected.size > 0 && (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-indigo-200 bg-indigo-50/60 px-3 py-2">
          <span className="text-sm font-medium text-indigo-700">已选 {selected.size} 个账号</span>
          <Button size="sm" variant="outline" onClick={() => { setBulkEnabled(new Set()); setBulkEditOpen(true); }}>
            <Pencil className="h-3.5 w-3.5" /> 批量编辑
          </Button>
          <Button size="sm" variant="outline" className="border-emerald-200 text-emerald-600 hover:bg-emerald-50" disabled={bulkBusy} onClick={bulkUnrestrictClients}>
            <Unlock className="h-3.5 w-3.5" /> {bulkBusy ? '处理中...' : '放开客户端限制'}
          </Button>
          <Button size="sm" variant="outline" className="border-indigo-200 text-indigo-600 hover:bg-indigo-50" onClick={openBulkToken}>
            <KeyRound className="h-3.5 w-3.5" /> 创建令牌
          </Button>
          <Button size="sm" variant="outline" className="border-red-200 text-red-600 hover:bg-red-50" onClick={() => setBulkDeleteOpen(true)}>
            <Trash2 className="h-3.5 w-3.5" /> 批量删除
          </Button>
          <Button size="sm" variant="ghost" onClick={clearSel}><X className="h-3.5 w-3.5" /> 取消选择</Button>
        </div>
      )}

      <BlurFade>
        <div className="overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-sm">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent bg-neutral-50/60">
                <TableHead className="w-8">
                  <input type="checkbox" className="h-4 w-4 align-middle accent-indigo-600" checked={allFilteredSelected} onChange={toggleSelectAll} title="全选(当前筛选)" />
                </TableHead>
                <TableHead>账号</TableHead>
                <TableHead>状态</TableHead>
                <TableHead>并发</TableHead>
                <TableHead>会话</TableHead>
                <TableHead>今日配额<br/>设备/会话</TableHead>
                <TableHead>RPM</TableHead>
                <TableHead>5h 配额 ($)</TableHead>
                <TableHead>用量(5h/7d/Son · 重置)</TableHead>
                <TableHead>身份/遥测</TableHead>
                <TableHead>配置</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageItems.map((a) => {
                const st = statusStyle(a);
                const dead = a.status === 'disabled' || isRateLimited(a);
                return (
                  <TableRow key={a.id} className={cn('align-top', dead && 'opacity-60', selected.has(a.id) && 'bg-indigo-50/40')}>
                    {/* 选择 */}
                    <TableCell>
                      <input type="checkbox" className="h-4 w-4 align-middle accent-indigo-600" checked={selected.has(a.id)} onChange={() => toggleSelect(a.id)} />
                    </TableCell>
                    {/* 账号 */}
                    <TableCell>
                      <div className="flex min-w-0 max-w-[220px] items-center gap-2">
                        <div className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-lg bg-indigo-50 text-xs font-semibold text-indigo-600">
                          {(a.name || a.email)[0]?.toUpperCase()}
                        </div>
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium text-neutral-900">{a.name || a.email}</p>
                          {a.name && <p className="truncate text-xs text-neutral-500">{a.email}</p>}
                        </div>
                      </div>
                      {a.disable_reason && dead && (
                        <p className={cn('mt-1 max-w-[220px] truncate text-[11px]', a.status === 'disabled' ? 'text-red-500' : 'text-amber-600')}>
                          {a.disable_reason}{isRateLimited(a) && ` · 剩余 ${formatTimeLeft(a.rate_limit_reset_at!)}`}
                        </p>
                      )}
                      {a.auth_type === 'oauth' && a.auth_error && <p className="mt-0.5 max-w-[220px] truncate text-[11px] text-red-500">{a.auth_error}</p>}
                      {/* 调试胶囊:客户端限制 + 绑定的令牌 */}
                      <div className="mt-1 flex max-w-[220px] flex-wrap gap-1">
                        {a.allowed_client_types
                          ? <span className="rounded-full border border-amber-200 bg-amber-50 px-1.5 py-px text-[10px] leading-4 text-amber-700">仅 {a.allowed_client_types.split(',').filter(Boolean).join('/')}</span>
                          : <span className="rounded-full border border-emerald-200 bg-emerald-50 px-1.5 py-px text-[10px] leading-4 text-emerald-700">全客户端</span>}
                        {(tokensByAccount.get(a.id) ?? []).filter((t) => t.category !== 'warmup').map((t) => (
                          <span key={t.id}
                            title={`令牌 #${t.id} ${t.name || '未命名'}${t.status !== 'active' ? '(已停用)' : ''}\n可用账号: ${t.allowed_accounts}`}
                            className={cn('max-w-[110px] truncate rounded-full border px-1.5 py-px text-[10px] leading-4',
                              t.status === 'active'
                                ? 'border-indigo-200 bg-indigo-50 text-indigo-700'
                                : 'border-neutral-200 bg-neutral-100 text-neutral-400 line-through')}>
                            🔑{t.name || `#${t.id}`}
                          </span>
                        ))}
                      </div>
                    </TableCell>
                    {/* 状态 */}
                    <TableCell>
                      <Badge className={st.className}><span className={cn('h-1.5 w-1.5 rounded-full', st.dot)} />{st.label}</Badge>
                    </TableCell>
                    {/* 并发 */}
                    <TableCell className={cn('text-sm font-medium', (a.current_concurrency || 0) >= a.concurrency ? 'text-red-500' : (a.current_concurrency || 0) > 0 ? 'text-emerald-600' : 'text-neutral-700')}>
                      {a.current_concurrency || 0} / {a.concurrency}
                    </TableCell>
                    {/* 会话 */}
                    <TableCell className={cn('text-sm font-medium', a.max_sessions && (a.current_sessions || 0) >= a.max_sessions ? 'text-red-500' : (a.current_sessions || 0) > 0 ? 'text-emerald-600' : 'text-neutral-700')}>
                      {a.current_sessions || 0} / {a.max_sessions || '∞'}
                    </TableCell>
                    {/* 今日配额:设备/会话(北京时间固定窗口用量) */}
                    <TableCell className="text-[11px] leading-tight">
                      <div className={cn('font-medium', a.device_quota && (a.current_devices || 0) >= a.device_quota ? 'text-red-500' : (a.current_devices || 0) > 0 ? 'text-emerald-600' : 'text-neutral-600')} title="今日(北京时间)已承接的不同设备数 / 设备配额">
                        设 {a.current_devices || 0}/{a.device_quota || '∞'}
                      </div>
                      <div className={cn('font-medium', a.session_quota && (a.current_window_sessions || 0) >= a.session_quota ? 'text-red-500' : (a.current_window_sessions || 0) > 0 ? 'text-emerald-600' : 'text-neutral-600')} title="今日(北京时间)已承接的不同会话数 / 会话配额">
                        话 {a.current_window_sessions || 0}/{a.session_quota || '∞'}
                      </div>
                    </TableCell>
                    {/* RPM(纯数字,无进度条) */}
                    <TableCell>
                      <span className={cn('text-sm font-medium tabular-nums', rpmColor(a))}>
                        {a.current_rpm || 0}{a.rpm_limit && a.rpm_limit > 0 ? ` / ${a.rpm_limit}` : ''}
                      </span>
                    </TableCell>
                    {/* 5h 配额(USD) */}
                    <TableCell>
                      {(() => {
                        const hasCap = !!(a.window_5h_cost_cap_usd && a.window_5h_cost_cap_usd > 0);
                        const ratio = costRatio(a);
                        const cost = a.cost_5h_usd || 0;
                        return (
                          <div className="min-w-[110px] space-y-1">
                            <span className={cn('text-sm font-medium tabular-nums', costColor(ratio, hasCap))}>
                              ${cost.toFixed(cost >= 100 ? 0 : 2)}
                              {hasCap ? ` / $${a.window_5h_cost_cap_usd}` : ''}
                            </span>
                            {hasCap && (
                              <div className="h-1 w-20 overflow-hidden rounded-full bg-neutral-100">
                                <div className={cn('h-full rounded-full', costBarColor(ratio))} style={{ width: `${ratio * 100}%` }} />
                              </div>
                            )}
                          </div>
                        );
                      })()}
                    </TableCell>
                    {/* 用量 + 重置时间 */}
                    <TableCell>
                      <div className="min-w-[196px] space-y-1 text-[11px]">
                        {[
                          { label: '5h', d: a.usage_data?.five_hour },
                          { label: '7d', d: a.usage_data?.seven_day },
                          { label: 'Son', d: a.usage_data?.seven_day_sonnet },
                        ].map((w) => (
                          <div key={w.label} className="flex items-center gap-2">
                            <span className="w-7 flex-shrink-0 text-neutral-400">{w.label}</span>
                            <div className="h-1 w-12 flex-shrink-0 overflow-hidden rounded-full bg-neutral-100">
                              <div className={cn('h-full rounded-full', usageBarColor(w.d ? w.d.utilization : 0))} style={{ width: `${w.d ? Math.min(w.d.utilization, 100) : 0}%` }} />
                            </div>
                            <span className="w-8 flex-shrink-0 text-right font-medium text-neutral-700">{w.d ? Math.round(w.d.utilization) : 0}%</span>
                            <span className="flex-shrink-0 text-[10px] text-neutral-400" title="距离重置剩余时间">{w.d ? `重置 ${formatTimeLeft(w.d.resets_at)}` : '—'}</span>
                          </div>
                        ))}
                      </div>
                    </TableCell>
                    {/* 身份/遥测 */}
                    <TableCell>
                      <div className="max-w-[150px] space-y-0.5 text-[11px]">
                        <p className={cn('truncate', a.identity_mode === 'normalize' ? 'text-emerald-600' : 'text-neutral-500')}>
                          {a.identity_mode === 'normalize' ? '归一化' : '透传'}
                          {a.identity_mode === 'normalize' && <span className="text-neutral-400"> v2.1.168 固定</span>}
                        </p>
                        {a.identity_mode === 'normalize' && (() => {
                          const sids = (a.captured_session_id || '').split(',').filter(Boolean);
                          return (
                            <p className="truncate text-neutral-400" title={sids.length ? `当前上游 ${sids.length} 个虚拟 session（每槽15-20min轮换）：\n${sids.join('\n')}${a.captured_session_at ? `\n更新于 ${a.captured_session_at}` : ''}` : '尚未吸取（首个请求后出现）'}>
                              sid×{sids.length || 0} {sids.length ? `${sids[0].slice(0, 8)}…` : '—'}
                            </p>
                          );
                        })()}
                        <p className={a.auto_telemetry ? 'text-emerald-600' : 'text-neutral-500'}>
                          遥测{a.auto_telemetry ? '开' : '关'}{a.telemetry_count > 0 && <span className="text-neutral-400"> ·{a.telemetry_count}</span>}
                        </p>
                      </div>
                    </TableCell>
                    {/* 配置 */}
                    <TableCell>
                      <div className="space-y-0.5 text-[11px]">
                        <p className="text-neutral-700">优先级 {a.priority}</p>
                        <p className={a.billing_mode === 'rewrite' ? 'text-amber-600' : 'text-neutral-500'}>{a.billing_mode === 'rewrite' ? '重写' : '清除'}</p>
                      </div>
                    </TableCell>
                    {/* 操作 */}
                    <TableCell>
                      <div className="flex items-center justify-end gap-0.5">
                        <Button variant="ghost" size="sm" onClick={() => toggleScheduling(a)}
                          className={cn('h-7 px-2 text-xs', dead ? 'text-emerald-600 hover:bg-emerald-50' : 'text-amber-600 hover:bg-amber-50')}>
                          {dead ? '启用' : '停用'}
                        </Button>
                        {dead && (
                          <Button variant="ghost" size="sm" onClick={() => resetAccount(a)} className="h-7 px-2 text-xs text-blue-600 hover:bg-blue-50">重置</Button>
                        )}
                        <Button variant="ghost" size="sm" onClick={() => openEdit(a)} className="h-7 px-2 text-xs text-neutral-500">编辑</Button>
                        <Button variant="ghost" size="sm" onClick={() => refreshUsage(a.id)} disabled={refreshingUsage === a.id} className="h-7 px-2 text-xs text-indigo-600">{refreshingUsage === a.id ? '...' : '用量'}</Button>
                        <Button variant="ghost" size="sm" onClick={() => { setDeleteTargetId(a.id); setShowDelete(true); }} className="h-7 px-2 text-xs text-red-500 hover:bg-red-50">删除</Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
              {pageItems.length === 0 && (
                <TableRow className="border-0 hover:bg-transparent">
                  <TableCell colSpan={12} className="py-16">
                    <div className="flex flex-col items-center justify-center text-neutral-400">
                      <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-neutral-100"><Users className="h-6 w-6 text-indigo-400" /></div>
                      <p className="text-sm">{allAccounts.length === 0 ? '暂无账号，点击"添加账号"开始' : '没有匹配的账号'}</p>
                    </div>
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      </BlurFade>

      {totalPages > 1 && (
        <div className="flex items-center justify-between pt-1">
          <p className="text-sm text-neutral-500">第 {curPage} / {totalPages} 页</p>
          <div className="flex items-center gap-1">
            <Button variant="ghost" size="sm" disabled={curPage <= 1} onClick={() => setPage(curPage - 1)}>上一页</Button>
            {visiblePages.map((p) => (
              <Button key={p} variant={p === curPage ? 'default' : 'ghost'} size="sm" className="h-8 w-8 p-0" onClick={() => setPage(p)}>{p}</Button>
            ))}
            <Button variant="ghost" size="sm" disabled={curPage >= totalPages} onClick={() => setPage(curPage + 1)}>下一页</Button>
          </div>
        </div>
      )}

      <AccountFormDialog open={showForm} onOpenChange={setShowForm} editing={editing} form={form} patch={patch} onSubmit={save} />
      <OAuthFlowDialog open={showOAuth} onOpenChange={setShowOAuth} onApply={applyOAuth} onRefresh={load} />

      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>确认删除</DialogTitle>
            <DialogDescription>此操作不可撤销，确认要删除此账号吗？</DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 pt-2">
            <Button variant="ghost" onClick={() => setShowDelete(false)}>取消</Button>
            <Button className="bg-red-500 text-white hover:bg-red-600" onClick={executeDelete}>删除</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 批量删除确认 */}
      <Dialog open={bulkDeleteOpen} onOpenChange={setBulkDeleteOpen}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>批量删除 {selected.size} 个账号</DialogTitle>
            <DialogDescription>此操作不可撤销,确认删除所选的 {selected.size} 个账号吗？</DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 pt-2">
            <Button variant="ghost" onClick={() => setBulkDeleteOpen(false)}>取消</Button>
            <Button className="bg-red-500 text-white hover:bg-red-600" disabled={bulkBusy} onClick={executeBulkDelete}>{bulkBusy ? '删除中...' : '删除'}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 批量创建令牌 */}
      <Dialog open={bulkTokenOpen} onOpenChange={setBulkTokenOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>为所选账号创建令牌</DialogTitle>
            <DialogDescription>令牌的可用账号将自动绑定为所选的 {selected.size} 个账号,其余配置正常填写。</DialogDescription>
          </DialogHeader>
          {createdToken ? (
            <div className="space-y-3">
              <p className="text-sm text-emerald-600">令牌创建成功,请复制保存:</p>
              <div className="flex items-center gap-2 rounded-md border border-neutral-200 bg-neutral-50 px-3 py-2">
                <code className="flex-1 break-all font-mono text-[11px] text-neutral-700">{createdToken}</code>
                <button onClick={copyCreatedToken} className="flex-shrink-0 text-neutral-400 hover:text-neutral-700" title="复制令牌">
                  {tokenCopied ? <Check className="h-4 w-4 text-emerald-500" /> : <Copy className="h-4 w-4" />}
                </button>
              </div>
              <DialogFooter className="gap-2 pt-2">
                <Button onClick={() => { setBulkTokenOpen(false); clearSel(); }}>完成</Button>
              </DialogFooter>
            </div>
          ) : (
            <form onSubmit={(e) => { e.preventDefault(); createBulkToken(); }} className="mt-1 space-y-4">
              <div className="space-y-2">
                <Label>绑定账号({selected.size} 个)</Label>
                <div className="flex max-h-24 flex-wrap gap-1.5 overflow-y-auto rounded-md border border-neutral-200 bg-neutral-50 p-2">
                  {allAccounts.filter((a) => selected.has(a.id)).map((a) => (
                    <span key={a.id} className="rounded-md border border-indigo-200 bg-indigo-50 px-2 py-0.5 text-[10px] text-indigo-700">
                      #{a.id} {a.name || a.email}
                    </span>
                  ))}
                </div>
              </div>
              <div className="space-y-2">
                <Label>分类</Label>
                <div className="flex gap-1.5">
                  {([['customer', '客户用'], ['warmup', '养号专用']] as const).map(([val, label]) => (
                    <button key={val} type="button" onClick={() => setBulkTokenForm((f) => ({ ...f, category: val }))}
                      className={cn('flex-1 rounded-md border px-3 py-1.5 text-xs transition-colors',
                        bulkTokenForm.category === val ? 'border-indigo-300 bg-indigo-50 text-indigo-700' : 'border-neutral-200 bg-neutral-50 text-neutral-500 hover:border-indigo-300')}>
                      {label}
                    </button>
                  ))}
                </div>
              </div>
              <div className="space-y-2">
                <Label>备注名(选填)</Label>
                <Input value={bulkTokenForm.name} onChange={(e) => setBulkTokenForm((f) => ({ ...f, name: e.target.value }))} placeholder="例如:生产环境、测试用" />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-2">
                  <Label>并发上限(0=不限)</Label>
                  <Input type="number" min={0} value={bulkTokenForm.concurrency} onChange={(e) => setBulkTokenForm((f) => ({ ...f, concurrency: Number(e.target.value) }))} placeholder="0" />
                </div>
                <div className="space-y-2">
                  <Label>过期时间(选填)</Label>
                  <Input type="datetime-local" value={bulkTokenForm.expires_at} onChange={(e) => setBulkTokenForm((f) => ({ ...f, expires_at: e.target.value }))} />
                </div>
              </div>
              <DialogFooter className="gap-2 pt-2">
                <Button type="button" variant="ghost" onClick={() => setBulkTokenOpen(false)}>取消</Button>
                <Button type="submit" disabled={bulkBusy}>{bulkBusy ? '创建中...' : '创建令牌'}</Button>
              </DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>

      {/* 批量编辑 */}
      <Dialog open={bulkEditOpen} onOpenChange={setBulkEditOpen}>
        <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>批量编辑 {selected.size} 个账号</DialogTitle>
            <DialogDescription>勾选要修改的项,仅勾选项会应用到所选账号(未勾选的保持不变)。</DialogDescription>
          </DialogHeader>
          <div className="mt-1 flex-1 space-y-2.5 overflow-y-auto pr-1">
            {bulkRow('status', '状态', (
              <select className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm" value={bulkVals.status} onChange={(e) => setBulkVal({ status: e.target.value })}>
                <option value="active">启用(active)</option>
                <option value="disabled">停用(disabled)</option>
              </select>
            ))}
            {bulkRow('concurrency', '并发数', <Input type="number" min={0} value={bulkVals.concurrency} onChange={(e) => setBulkVal({ concurrency: Number(e.target.value) })} />)}
            {bulkRow('max_sessions', '会话窗口', <Input type="number" min={0} value={bulkVals.max_sessions} onChange={(e) => setBulkVal({ max_sessions: Number(e.target.value) })} />)}
            {bulkRow('priority', '优先级', <Input type="number" min={0} value={bulkVals.priority} onChange={(e) => setBulkVal({ priority: Number(e.target.value) })} />)}
            {bulkRow('device_quota', '设备配额', <Input type="number" min={0} value={bulkVals.device_quota} onChange={(e) => setBulkVal({ device_quota: Number(e.target.value) })} placeholder="0=不限" />)}
            {bulkRow('session_quota', '会话配额', <Input type="number" min={0} value={bulkVals.session_quota} onChange={(e) => setBulkVal({ session_quota: Number(e.target.value) })} placeholder="0=不限" />)}
            {bulkRow('rpm_limit', 'RPM上限', <Input type="number" min={0} value={bulkVals.rpm_limit} onChange={(e) => setBulkVal({ rpm_limit: Number(e.target.value) })} placeholder="0=不限" />)}
            {bulkRow('window_5h_cost_cap_usd', '5h成本上限', <Input type="number" min={0} step="0.01" value={bulkVals.window_5h_cost_cap_usd} onChange={(e) => setBulkVal({ window_5h_cost_cap_usd: Number(e.target.value) })} placeholder="0=不限" />)}
            {bulkRow('warmup_skip', '新号升温', (
              <select className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm" value={bulkVals.warmup_skip ? '1' : '0'} onChange={(e) => setBulkVal({ warmup_skip: e.target.value === '1' })}>
                <option value="1">跳过升温</option>
                <option value="0">参与升温</option>
              </select>
            ))}
            {bulkRow('identity_mode', '身份模拟', (
              <select className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm" value={bulkVals.identity_mode} onChange={(e) => setBulkVal({ identity_mode: e.target.value })}>
                <option value="normalize">归一化(normalize)</option>
                <option value="passthrough">透传(passthrough)</option>
              </select>
            ))}
            {bulkRow('allowed_client_types', '允许客户端', (
              <div className="flex flex-wrap gap-1.5">
                {['cli', 'vscode', 'sdk', 'desktop', 'other'].map((c) => {
                  const on = bulkVals.allowed_client_types.includes(c);
                  return (
                    <button key={c} type="button" onClick={() => setBulkVal({ allowed_client_types: on ? bulkVals.allowed_client_types.filter((x) => x !== c) : [...bulkVals.allowed_client_types, c] })}
                      className={cn('rounded-md border px-2 py-0.5 text-[11px]', on ? 'border-indigo-300 bg-indigo-50 text-indigo-700' : 'border-neutral-200 bg-neutral-50 text-neutral-500')}>
                      {c}
                    </button>
                  );
                })}
              </div>
            ))}
            {bulkRow('billing_mode', '计费模式', (
              <select className="h-9 w-full rounded-md border border-input bg-background px-2 text-sm" value={bulkVals.billing_mode} onChange={(e) => setBulkVal({ billing_mode: e.target.value })}>
                <option value="strip">清除(strip)</option>
                <option value="rewrite">重写(rewrite)</option>
              </select>
            ))}
            {bulkRow('proxy_url', '代理地址', <Input value={bulkVals.proxy_url} onChange={(e) => setBulkVal({ proxy_url: e.target.value })} placeholder="http:// 或 socks5://（留空=清除代理）" />)}
          </div>
          <DialogFooter className="gap-2 pt-2">
            <Button variant="ghost" onClick={() => setBulkEditOpen(false)}>取消</Button>
            <Button disabled={bulkBusy || bulkEnabled.size === 0} onClick={applyBulkEdit}>{bulkBusy ? '应用中...' : `应用到 ${selected.size} 个账号`}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
