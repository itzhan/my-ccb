import { useCallback, useEffect, useMemo, useState } from 'react';
import { Plus, KeyRound, Users, Search } from 'lucide-react';
import { api, type Account } from '@/api';
import { useToast } from '@/components/Toaster';
import { useDashboardRefresh } from '@/components/Layout';
import { cn } from '@/lib/utils';
import { isRateLimited, statusStyle, usageBarColor, formatTimeLeft, sortAccounts } from '@/lib/format';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
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

  // 立即加载 + 每 8s 轮询
  useEffect(() => {
    load();
    const id = setInterval(load, 8000);
    return () => clearInterval(id);
  }, [load]);

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

      <BlurFade>
        <div className="overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-sm">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent bg-neutral-50/60">
                <TableHead>账号</TableHead>
                <TableHead>状态</TableHead>
                <TableHead>并发</TableHead>
                <TableHead>会话</TableHead>
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
                  <TableRow key={a.id} className={cn('align-top', dead && 'opacity-60')}>
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
                          {a.identity_mode === 'normalize' && (a.identity_captured_at
                            ? <span className="text-neutral-400"> v{String(a.canonical_env?.version ?? '')}</span>
                            : <span className="text-amber-600"> 待吸取</span>)}
                        </p>
                        <p className={a.auto_telemetry ? 'text-emerald-600' : 'text-neutral-500'}>
                          遥测{a.auto_telemetry ? '开' : '关'}{a.telemetry_count > 0 && <span className="text-neutral-400"> ·{a.telemetry_count}</span>}
                        </p>
                        {a.allowed_client_types && <p className="truncate text-amber-600">仅 {a.allowed_client_types.split(',').filter(Boolean).join('/')}</p>}
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
                  <TableCell colSpan={10} className="py-16">
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
      <OAuthFlowDialog open={showOAuth} onOpenChange={setShowOAuth} onApply={applyOAuth} />

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
    </div>
  );
}
