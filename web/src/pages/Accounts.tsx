import { useCallback, useEffect, useMemo, useState } from 'react';
import { Plus, KeyRound, Users } from 'lucide-react';
import { api, type Account } from '@/api';
import { useToast } from '@/components/Toaster';
import { useDashboardRefresh } from '@/components/Layout';
import { cn } from '@/lib/utils';
import { isRateLimited, statusStyle, usageBarColor, formatTimeLeft, sortAccounts } from '@/lib/format';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from '@/components/ui/table';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { BlurFade } from '@/components/magic/blur-fade';
import { AccountFormDialog } from '@/components/accounts/AccountFormDialog';
import { OAuthFlowDialog } from '@/components/accounts/OAuthFlowDialog';
import { emptyForm, formFromAccount, type FormState } from '@/components/accounts/form';

const PAGE_SIZE = 12;

export default function Accounts() {
  const toast = useToast();
  const refreshDashboard = useDashboardRefresh();

  const [accounts, setAccounts] = useState<Account[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [totalCount, setTotalCount] = useState(0);

  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<Account | null>(null);
  const [form, setForm] = useState<FormState>(emptyForm());

  const [showOAuth, setShowOAuth] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [deleteTargetId, setDeleteTargetId] = useState<number | null>(null);

  const [testing, setTesting] = useState<number | null>(null);
  const [testResult, setTestResult] = useState<{ status: string; message?: string } | null>(null);
  const [refreshingUsage, setRefreshingUsage] = useState<number | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await api.listAccounts(page, PAGE_SIZE);
      setAccounts(res.data ?? []);
      setTotalPages(res.total_pages);
      setTotalCount(res.total);
    } catch {
      setAccounts([]);
    }
  }, [page]);

  // 立即加载 + 每 8s 轮询(实时并发/会话/RPM/用量)
  useEffect(() => {
    load();
    const id = setInterval(load, 8000);
    return () => clearInterval(id);
  }, [load]);

  const sorted = useMemo(() => sortAccounts(accounts), [accounts]);
  const patch = (p: Partial<FormState>) => setForm((f) => ({ ...f, ...p }));

  function openCreate() {
    setEditing(null);
    setForm(emptyForm());
    setShowForm(true);
  }
  function openEdit(a: Account) {
    setEditing(a);
    setForm(formFromAccount(a));
    setShowForm(true);
  }

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
        u.recapture_days = Number(form.recapture_days) || 0;
        u.max_sessions = Math.max(0, Number(form.max_sessions) || 0);
        u.allowed_client_types = form.allowed_client_types.join(',');
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
          recapture_days: Number(form.recapture_days) || 0,
          max_sessions: Math.max(0, Number(form.max_sessions) || 0),
          allowed_client_types: form.allowed_client_types.join(','),
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

  async function test(id: number) {
    setTesting(id);
    setTestResult(null);
    try {
      const r = await api.testAccount(id);
      setTestResult(r);
      if (r.status === 'error') toast(r.message || '测试失败');
    } catch (e) {
      toast((e as Error).message || '测试请求失败');
    }
    setTimeout(() => { setTesting(null); setTestResult(null); }, 3000);
  }

  async function refreshUsage(id: number) {
    setRefreshingUsage(id);
    try {
      const res = await api.refreshUsage(id);
      if (res.status === 'ok' && res.usage) {
        setAccounts((prev) => prev.map((a) => a.id === id ? { ...a, usage_data: res.usage, usage_fetched_at: new Date().toISOString() } : a));
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
      setAccounts((prev) => prev.map((x) => x.id === a.id
        ? { ...x, status: res.status, disable_reason: res.disable_reason ?? '', rate_limited_at: res.rate_limited_at, rate_limit_reset_at: res.rate_limit_reset_at }
        : x));
      refreshDashboard();
    } catch (e) {
      toast((e as Error).message || '切换调度失败');
    }
  }

  function applyOAuth(f: FormState) {
    setEditing(null);
    setForm(f);
    setShowForm(true);
  }

  const visiblePages = useMemo(() => {
    const pages: number[] = [];
    let start = Math.max(1, page - 2);
    const end = Math.min(totalPages, start + 4);
    start = Math.max(1, end - 4);
    for (let i = start; i <= end; i++) pages.push(i);
    return pages;
  }, [page, totalPages]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">账号管理</h2>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => setShowOAuth(true)}><KeyRound className="h-4 w-4" /> 授权登录</Button>
          <Button onClick={openCreate}><Plus className="h-4 w-4" /> 添加账号</Button>
        </div>
      </div>

      <BlurFade>
        <div className="overflow-hidden rounded-xl border border-border bg-card/40">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>账号</TableHead>
                <TableHead>状态</TableHead>
                <TableHead>并发</TableHead>
                <TableHead>会话</TableHead>
                <TableHead>RPM</TableHead>
                <TableHead>用量(5h/7d/Son)</TableHead>
                <TableHead>身份/遥测</TableHead>
                <TableHead>配置</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sorted.map((a) => {
                const st = statusStyle(a);
                const dead = a.status === 'disabled' || isRateLimited(a);
                return (
                  <TableRow key={a.id} className={cn('align-top', dead && 'opacity-55')}>
                    {/* 账号 */}
                    <TableCell>
                      <div className="flex min-w-0 max-w-[220px] items-center gap-2">
                        <div className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-lg bg-primary/15 text-xs font-semibold text-primary">
                          {(a.name || a.email)[0]?.toUpperCase()}
                        </div>
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium">{a.name || a.email}</p>
                          {a.name && <p className="truncate text-xs text-muted-foreground">{a.email}</p>}
                        </div>
                      </div>
                      {a.disable_reason && dead && (
                        <p className={cn('mt-1 max-w-[220px] truncate text-[11px]', a.status === 'disabled' ? 'text-red-400' : 'text-amber-400')}>
                          {a.disable_reason}{isRateLimited(a) && ` · 剩余 ${formatTimeLeft(a.rate_limit_reset_at!)}`}
                        </p>
                      )}
                      {a.auth_type === 'oauth' && a.auth_error && <p className="mt-0.5 max-w-[220px] truncate text-[11px] text-red-400">{a.auth_error}</p>}
                      {testing === a.id && testResult && (
                        <p className={cn('mt-0.5 text-[11px] font-medium', testResult.status === 'ok' ? 'text-emerald-400' : 'text-red-400')}>
                          {testResult.status === 'ok' ? '连接正常' : testResult.message}
                        </p>
                      )}
                    </TableCell>
                    {/* 状态 */}
                    <TableCell>
                      <Badge className={st.className}><span className={cn('h-1.5 w-1.5 rounded-full', st.dot)} />{st.label}</Badge>
                    </TableCell>
                    {/* 并发 */}
                    <TableCell className={cn('text-sm font-medium', (a.current_concurrency || 0) >= a.concurrency ? 'text-red-400' : (a.current_concurrency || 0) > 0 ? 'text-emerald-400' : 'text-foreground')}>
                      {a.current_concurrency || 0} / {a.concurrency}
                    </TableCell>
                    {/* 会话 */}
                    <TableCell className={cn('text-sm font-medium', a.max_sessions && (a.current_sessions || 0) >= a.max_sessions ? 'text-red-400' : (a.current_sessions || 0) > 0 ? 'text-emerald-400' : 'text-foreground')}>
                      {a.current_sessions || 0} / {a.max_sessions || '∞'}
                    </TableCell>
                    {/* RPM */}
                    <TableCell>
                      {a.rpm_limit && a.rpm_limit > 0 ? (
                        <div className="flex min-w-[88px] items-center gap-1.5">
                          <div className="h-1.5 w-12 flex-shrink-0 overflow-hidden rounded-full bg-secondary">
                            <div className={cn('h-full rounded-full', (a.current_rpm || 0) / a.rpm_limit >= 0.8 ? 'bg-red-500' : (a.current_rpm || 0) / a.rpm_limit >= 0.5 ? 'bg-amber-500' : 'bg-emerald-500')}
                              style={{ width: `${Math.min(100, ((a.current_rpm || 0) / a.rpm_limit) * 100)}%` }} />
                          </div>
                          <span className={cn('whitespace-nowrap text-xs', (a.current_rpm || 0) > 0 ? 'font-medium text-foreground' : 'text-muted-foreground')}>{a.current_rpm || 0}/{a.rpm_limit}</span>
                        </div>
                      ) : (
                        <span className={cn('text-sm', (a.current_rpm || 0) > 0 ? 'font-medium text-emerald-400' : 'text-muted-foreground')}>{a.current_rpm || 0}</span>
                      )}
                    </TableCell>
                    {/* 用量 */}
                    <TableCell>
                      <div className="min-w-[112px] space-y-0.5 text-[11px]">
                        {[
                          { label: '5h', d: a.usage_data?.five_hour },
                          { label: '7d', d: a.usage_data?.seven_day },
                          { label: 'Son', d: a.usage_data?.seven_day_sonnet },
                        ].map((w) => (
                          <div key={w.label} className="flex items-center gap-1.5">
                            <span className="w-6 flex-shrink-0 text-muted-foreground">{w.label}</span>
                            <div className="h-1 flex-1 overflow-hidden rounded-full bg-secondary">
                              <div className={cn('h-full rounded-full', usageBarColor(w.d ? w.d.utilization : 0))} style={{ width: `${w.d ? Math.min(w.d.utilization, 100) : 0}%` }} />
                            </div>
                            <span className="w-8 flex-shrink-0 text-right font-medium text-foreground/80">{w.d ? Math.round(w.d.utilization) : 0}%</span>
                          </div>
                        ))}
                      </div>
                    </TableCell>
                    {/* 身份/遥测 */}
                    <TableCell>
                      <div className="max-w-[150px] space-y-0.5 text-[11px]">
                        <p className={cn('truncate', a.identity_mode === 'normalize' ? 'text-emerald-400' : 'text-muted-foreground')}>
                          {a.identity_mode === 'normalize' ? '归一化' : '透传'}
                          {a.identity_mode === 'normalize' && (a.identity_captured_at
                            ? <span className="text-muted-foreground"> v{String(a.canonical_env?.version ?? '')}</span>
                            : <span className="text-amber-400"> 待吸取</span>)}
                        </p>
                        <p className={a.auto_telemetry ? 'text-emerald-400' : 'text-muted-foreground'}>
                          遥测{a.auto_telemetry ? '开' : '关'}{a.telemetry_count > 0 && <span className="text-muted-foreground"> ·{a.telemetry_count}</span>}
                        </p>
                        {a.allowed_client_types && <p className="truncate text-amber-400">仅 {a.allowed_client_types.split(',').filter(Boolean).join('/')}</p>}
                      </div>
                    </TableCell>
                    {/* 配置 */}
                    <TableCell>
                      <div className="space-y-0.5 text-[11px]">
                        <p className="text-foreground/80">优先级 {a.priority}</p>
                        <p className={a.billing_mode === 'rewrite' ? 'text-amber-400' : 'text-muted-foreground'}>{a.billing_mode === 'rewrite' ? '重写' : '清除'}</p>
                      </div>
                    </TableCell>
                    {/* 操作 */}
                    <TableCell>
                      <div className="flex items-center justify-end gap-0.5">
                        <Button variant="ghost" size="sm" onClick={() => toggleScheduling(a)}
                          className={cn('h-7 px-2 text-xs', dead ? 'text-emerald-400 hover:bg-emerald-500/10' : 'text-amber-400 hover:bg-amber-500/10')}>
                          {dead ? '启用' : '停用'}
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => openEdit(a)} className="h-7 px-2 text-xs text-muted-foreground">编辑</Button>
                        <Button variant="ghost" size="sm" onClick={() => refreshUsage(a.id)} disabled={refreshingUsage === a.id} className="h-7 px-2 text-xs text-primary">{refreshingUsage === a.id ? '...' : '用量'}</Button>
                        <Button variant="ghost" size="sm" onClick={() => test(a.id)} disabled={testing === a.id} className="h-7 px-2 text-xs text-primary">{testing === a.id ? '...' : '测试'}</Button>
                        <Button variant="ghost" size="sm" onClick={() => { setDeleteTargetId(a.id); setShowDelete(true); }} className="h-7 px-2 text-xs text-red-400 hover:bg-red-500/10">删除</Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
              {sorted.length === 0 && (
                <TableRow className="hover:bg-transparent border-0">
                  <TableCell colSpan={9} className="py-16">
                    <div className="flex flex-col items-center justify-center text-muted-foreground">
                      <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-secondary"><Users className="h-6 w-6 text-primary/60" /></div>
                      <p className="text-sm">暂无账号，点击"添加账号"开始</p>
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
          <p className="text-sm text-muted-foreground">共 {totalCount} 个账号</p>
          <div className="flex items-center gap-1">
            <Button variant="ghost" size="sm" disabled={page <= 1} onClick={() => setPage((p) => Math.max(1, p - 1))}>上一页</Button>
            {visiblePages.map((p) => (
              <Button key={p} variant={p === page ? 'default' : 'ghost'} size="sm" className="h-8 w-8 p-0" onClick={() => setPage(p)}>{p}</Button>
            ))}
            <Button variant="ghost" size="sm" disabled={page >= totalPages} onClick={() => setPage((p) => Math.min(totalPages, p + 1))}>下一页</Button>
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
