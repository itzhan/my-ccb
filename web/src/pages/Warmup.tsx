import { useCallback, useEffect, useMemo, useState } from 'react';
import { Plus, Flame, Play, Square, ScrollText } from 'lucide-react';
import { api, type WarmupTask, type ApiToken, type Account, type UsageLog } from '@/api';
import { useToast } from '@/components/Toaster';
import { usePolling } from '@/hooks/usePolling';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from '@/components/ui/table';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { BlurFade } from '@/components/magic/blur-fade';

// 表单内时长用分钟,间隔用秒;account_ids 为逗号分隔的账号 ID(保存时换算为养号令牌)。
interface TaskForm {
  name: string;
  account_ids: string;
  msg_interval_secs: number;
  total_min: number;
  work_min: number;
  rest_min: number;
  jitter_pct: number;
  model: string;
}
const emptyForm = (): TaskForm => ({
  name: '', account_ids: '', msg_interval_secs: 60, total_min: 60, work_min: 0, rest_min: 0, jitter_pct: 20, model: '',
});

const STATUS_META: Record<string, { label: string; cls: string; dot: string }> = {
  pending: { label: '待启动', cls: 'border-neutral-200 bg-neutral-100 text-neutral-500', dot: 'bg-neutral-400' },
  running: { label: '运行中', cls: 'border-emerald-200 bg-emerald-50 text-emerald-700', dot: 'bg-emerald-500 animate-pulse' },
  completed: { label: '已完成', cls: 'border-indigo-200 bg-indigo-50 text-indigo-700', dot: 'bg-indigo-500' },
  stopped: { label: '已停止', cls: 'border-neutral-200 bg-neutral-100 text-neutral-500', dot: 'bg-neutral-400' },
  error: { label: '出错', cls: 'border-red-200 bg-red-50 text-red-600', dot: 'bg-red-500' },
};

function fmtSecs(s: number): string {
  if (s <= 0) return '关';
  if (s < 60) return `${s}秒`;
  if (s < 3600) return `${Math.round(s / 60)}分`;
  return `${(s / 3600).toFixed(1)}时`;
}

export default function Warmup() {
  const toast = useToast();
  const [tasks, setTasks] = useState<WarmupTask[]>([]);
  const [tokens, setTokens] = useState<ApiToken[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [logs, setLogs] = useState<UsageLog[]>([]);
  const [qCount, setQCount] = useState<number>(0);
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<WarmupTask | null>(null);
  const [form, setForm] = useState<TaskForm>(emptyForm());
  const [saving, setSaving] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [deleteTargetId, setDeleteTargetId] = useState<number | null>(null);

  // 账号 id -> 名称
  const accName = useCallback((id: number) => {
    const a = accounts.find((x) => x.id === id);
    return a ? (a.name || a.email) : `#${id}`;
  }, [accounts]);
  // 养号令牌 id -> 绑定的账号 id
  const tokenToAccount = useCallback((tokenId: number): number | null => {
    const t = tokens.find((x) => x.id === tokenId);
    if (!t || !t.allowed_accounts) return null;
    const first = t.allowed_accounts.split(',')[0]?.trim();
    return first ? Number(first) : null;
  }, [tokens]);

  const load = useCallback(async () => {
    try { setTasks((await api.listWarmupTasks()).data ?? []); } catch { /* ignore */ }
    try { setLogs((await api.getWarmupLogs(1, 50)).data ?? []); } catch { /* ignore */ }
  }, []);
  const loadRefs = useCallback(async () => {
    try { setTokens((await api.listWarmupTokens()).data ?? []); } catch { setTokens([]); }
    try { setAccounts((await api.listAccounts(1, 100)).data ?? []); } catch { setAccounts([]); }
  }, []);
  useEffect(() => {
    loadRefs();
    api.warmupQuestionsCount().then((r) => setQCount(r.count)).catch(() => {});
  }, [loadRefs]);
  usePolling(load, 5000);

  const patch = (p: Partial<TaskForm>) => setForm((f) => ({ ...f, ...p }));

  function openCreate() { setEditing(null); setForm(emptyForm()); loadRefs(); setShowForm(true); }
  function openEdit(t: WarmupTask) {
    setEditing(t);
    // token_ids -> account_ids
    const accIds = t.token_ids.split(',').map((s) => s.trim()).filter(Boolean)
      .map((id) => tokenToAccount(Number(id))).filter((x): x is number => x !== null);
    setForm({
      name: t.name,
      account_ids: accIds.join(','),
      msg_interval_secs: t.msg_interval_secs,
      total_min: Math.round(t.total_duration_secs / 60),
      work_min: Math.round(t.work_duration_secs / 60),
      rest_min: Math.round(t.rest_duration_secs / 60),
      jitter_pct: t.jitter_pct,
      model: t.model,
    });
    loadRefs();
    setShowForm(true);
  }

  async function save() {
    const accIds = form.account_ids.split(',').map((s) => Number(s.trim())).filter((n) => n > 0);
    if (accIds.length === 0) { toast('请至少选择一个账号'); return; }
    setSaving(true);
    try {
      // 为选中的账号确保养号令牌(已存在则复用),拿到 token_ids
      const ensured = (await api.ensureWarmupTokens(accIds)).data ?? [];
      const tokenIds = ensured.map((t) => t.id).join(',');
      const payload = {
        name: form.name,
        token_ids: tokenIds,
        msg_interval_secs: Number(form.msg_interval_secs) || 60,
        total_duration_secs: (Number(form.total_min) || 60) * 60,
        work_duration_secs: (Number(form.work_min) || 0) * 60,
        rest_duration_secs: (Number(form.rest_min) || 0) * 60,
        jitter_pct: Number(form.jitter_pct) || 0,
        model: form.model.trim(),
      };
      if (editing) await api.updateWarmupTask(editing.id, payload);
      else await api.createWarmupTask(payload);
      setShowForm(false);
      await loadRefs();
      await load();
    } catch (e) { toast((e as Error).message || '保存失败'); }
    finally { setSaving(false); }
  }

  async function start(t: WarmupTask) {
    try { await api.startWarmupTask(t.id); await load(); } catch (e) { toast((e as Error).message || '启动失败'); }
  }
  async function stop(t: WarmupTask) {
    try { await api.stopWarmupTask(t.id); await load(); } catch (e) { toast((e as Error).message || '停止失败'); }
  }
  async function executeDelete() {
    if (deleteTargetId === null) return;
    try { await api.deleteWarmupTask(deleteTargetId); setShowDelete(false); setDeleteTargetId(null); await load(); }
    catch (e) { toast((e as Error).message || '删除失败'); }
  }

  function toggleAccount(id: number) {
    const ids = form.account_ids.split(',').map((s) => s.trim()).filter(Boolean);
    const i = ids.indexOf(String(id));
    if (i >= 0) ids.splice(i, 1); else ids.push(String(id));
    patch({ account_ids: ids.join(',') });
  }
  function isAccSelected(id: number) {
    return form.account_ids.split(',').map((s) => s.trim()).includes(String(id));
  }
  function taskAccountNames(t: WarmupTask): string {
    const ids = t.token_ids.split(',').map((s) => s.trim()).filter(Boolean)
      .map((id) => tokenToAccount(Number(id))).filter((x): x is number => x !== null);
    if (ids.length === 0) return '—';
    return ids.map((id) => accName(id)).join(', ');
  }

  const logCount = useMemo(() => logs.length, [logs]);
  // 选择器只展示活跃账号(排除手动停用 disabled 和 401/异常 error)
  const selectableAccounts = useMemo(() => accounts.filter((a) => a.status === 'active'), [accounts]);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-neutral-900">自动养号</h2>
          <p className="text-xs text-neutral-400">题库 {qCount} 题 · 选账号即自动建/复用养号令牌(令牌名=账号名),每账号一个常驻 Claude Code 持续交互</p>
        </div>
        <Button onClick={openCreate}><Plus className="h-4 w-4" /> 创建任务</Button>
      </div>

      <BlurFade>
        <div className="overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-sm">
          <Table>
            <TableHeader>
              <TableRow className="bg-neutral-50/60 hover:bg-transparent">
                <TableHead>任务</TableHead>
                <TableHead>养号账号</TableHead>
                <TableHead>节奏</TableHead>
                <TableHead>已发消息</TableHead>
                <TableHead>状态</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tasks.map((t) => {
                const meta = STATUS_META[t.status] ?? STATUS_META.pending;
                const running = t.status === 'running';
                return (
                  <TableRow key={t.id} className="align-top">
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <div className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-600"><Flame className="h-3.5 w-3.5" /></div>
                        <div>
                          <p className="text-sm font-medium text-neutral-900">{t.name || '未命名任务'}</p>
                          <p className="text-xs text-neutral-400">{new Date(t.created_at).toLocaleDateString('zh-CN')}</p>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="max-w-[200px] truncate text-xs text-neutral-600" title={taskAccountNames(t)}>{taskAccountNames(t)}</TableCell>
                    <TableCell className="text-xs text-neutral-600">
                      <div>间隔 {fmtSecs(t.msg_interval_secs)} · 总 {fmtSecs(t.total_duration_secs)}</div>
                      <div className="text-neutral-400">
                        {t.work_duration_secs > 0 ? `工作 ${fmtSecs(t.work_duration_secs)}/休息 ${fmtSecs(t.rest_duration_secs)}` : '不休息'}
                        {t.jitter_pct > 0 ? ` · 抖动 ${t.jitter_pct}%` : ''}
                      </div>
                    </TableCell>
                    <TableCell className="text-sm text-neutral-700">{t.messages_sent}</TableCell>
                    <TableCell>
                      <Badge className={meta.cls}>
                        <span className={cn('h-1.5 w-1.5 rounded-full', meta.dot)} />
                        {meta.label}
                      </Badge>
                      {t.status === 'error' && t.error && (
                        <p className="mt-1 max-w-[180px] truncate text-[11px] text-red-500" title={t.error}>{t.error}</p>
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center justify-end gap-0.5">
                        {running ? (
                          <Button variant="ghost" size="sm" onClick={() => stop(t)} className="h-7 px-2 text-xs text-amber-600 hover:bg-amber-50"><Square className="h-3 w-3" /> 停止</Button>
                        ) : (
                          <Button variant="ghost" size="sm" onClick={() => start(t)} className="h-7 px-2 text-xs text-emerald-600 hover:bg-emerald-50"><Play className="h-3 w-3" /> 启动</Button>
                        )}
                        <Button variant="ghost" size="sm" onClick={() => openEdit(t)} disabled={running} className="h-7 px-2 text-xs text-neutral-500">编辑</Button>
                        <Button variant="ghost" size="sm" onClick={() => { setDeleteTargetId(t.id); setShowDelete(true); }} className="h-7 px-2 text-xs text-red-500 hover:bg-red-50">删除</Button>
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })}
              {tasks.length === 0 && (
                <TableRow className="border-0 hover:bg-transparent">
                  <TableCell colSpan={6} className="py-16">
                    <div className="flex flex-col items-center justify-center text-neutral-400">
                      <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-neutral-100"><Flame className="h-6 w-6 text-amber-400" /></div>
                      <p className="text-sm">暂无养号任务，点击"创建任务"开始</p>
                    </div>
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      </BlurFade>

      {/* 养号日志 */}
      <BlurFade>
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <ScrollText className="h-4 w-4 text-neutral-500" />
            <h3 className="text-sm font-medium text-neutral-700">养号日志</h3>
            <span className="text-xs text-neutral-400">最近 {logCount} 条养号调用(每 5 秒刷新)</span>
          </div>
          <div className="overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-sm">
            <Table>
              <TableHeader>
                <TableRow className="bg-neutral-50/60 hover:bg-transparent">
                  <TableHead>时间</TableHead>
                  <TableHead>账号</TableHead>
                  <TableHead>模型</TableHead>
                  <TableHead>状态</TableHead>
                  <TableHead>输入/输出</TableHead>
                  <TableHead>耗时</TableHead>
                  <TableHead>错误</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {logs.map((l) => {
                  const ok = l.status_code > 0 && l.status_code < 400;
                  return (
                    <TableRow key={l.id} className="align-top">
                      <TableCell className="text-xs text-neutral-500 whitespace-nowrap">{new Date(l.created_at).toLocaleTimeString('zh-CN')}</TableCell>
                      <TableCell className="max-w-[140px] truncate text-xs text-neutral-700" title={accName(l.account_id)}>{accName(l.account_id)}</TableCell>
                      <TableCell className="text-xs text-neutral-600">{l.model || '—'}</TableCell>
                      <TableCell>
                        <span className={cn('rounded px-1.5 py-0.5 text-[11px]', l.status_code === 0 ? 'bg-neutral-100 text-neutral-500' : ok ? 'bg-emerald-50 text-emerald-700' : 'bg-red-50 text-red-600')}>
                          {l.status_code || '—'}
                        </span>
                      </TableCell>
                      <TableCell className="text-xs text-neutral-600 whitespace-nowrap">{l.input_tokens}/{l.output_tokens}</TableCell>
                      <TableCell className="text-xs text-neutral-500 whitespace-nowrap">{l.duration_ms}ms</TableCell>
                      <TableCell className="max-w-[200px] truncate text-[11px] text-red-500" title={l.error}>{l.error || ''}</TableCell>
                    </TableRow>
                  );
                })}
                {logs.length === 0 && (
                  <TableRow className="border-0 hover:bg-transparent">
                    <TableCell colSpan={7} className="py-10">
                      <p className="text-center text-sm text-neutral-400">暂无养号日志(开始养号后这里会显示每次调用)</p>
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        </div>
      </BlurFade>

      {/* 新建/编辑任务 */}
      <Dialog open={showForm} onOpenChange={setShowForm}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{editing ? '编辑养号任务' : '创建养号任务'}</DialogTitle>
            <DialogDescription>直接选要养的账号,系统会自动为每个账号创建/复用养号令牌(令牌名=账号名)。</DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => { e.preventDefault(); save(); }} className="mt-1 space-y-4">
            <div className="space-y-2">
              <Label>任务名（选填）</Label>
              <Input value={form.name} onChange={(e) => patch({ name: e.target.value })} placeholder="例如：夜间批量养号" />
            </div>
            <div className="space-y-2">
              <Label>养号账号（必选，可多选批量养号）</Label>
              {selectableAccounts.length > 0 ? (
                <div className="flex max-h-40 flex-wrap gap-1.5 overflow-y-auto">
                  {selectableAccounts.map((a) => (
                    <button key={a.id} type="button" onClick={() => toggleAccount(a.id)}
                      className={cn('rounded-md border px-2 py-0.5 text-[10px] transition-colors',
                        isAccSelected(a.id) ? 'border-amber-300 bg-amber-50 text-amber-700' : 'border-neutral-200 bg-neutral-50 text-neutral-500 hover:border-amber-300')}>
                      #{a.id} {a.name || a.email}
                    </button>
                  ))}
                </div>
              ) : (
                <p className="text-[11px] text-neutral-400">没有可用账号(已停用/异常的账号不在此显示)。请到「账号」页添加或恢复账号。</p>
              )}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label>消息间隔（秒）</Label>
                <Input type="number" min={1} value={form.msg_interval_secs} onChange={(e) => patch({ msg_interval_secs: Number(e.target.value) })} />
              </div>
              <div className="space-y-2">
                <Label>总时长（分钟）</Label>
                <Input type="number" min={1} value={form.total_min} onChange={(e) => patch({ total_min: Number(e.target.value) })} />
              </div>
              <div className="space-y-2">
                <Label>工作时长（分钟，0=不休息）</Label>
                <Input type="number" min={0} value={form.work_min} onChange={(e) => patch({ work_min: Number(e.target.value) })} />
              </div>
              <div className="space-y-2">
                <Label>休息时长（分钟）</Label>
                <Input type="number" min={0} value={form.rest_min} onChange={(e) => patch({ rest_min: Number(e.target.value) })} />
              </div>
              <div className="space-y-2">
                <Label>间隔抖动（%）</Label>
                <Input type="number" min={0} max={100} value={form.jitter_pct} onChange={(e) => patch({ jitter_pct: Number(e.target.value) })} />
              </div>
              <div className="space-y-2">
                <Label>模型（选填）</Label>
                <Input value={form.model} onChange={(e) => patch({ model: e.target.value })} placeholder="opus / sonnet，留空=默认" />
              </div>
            </div>
            <DialogFooter className="gap-2 pt-2">
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>取消</Button>
              <Button type="submit" disabled={saving}>{saving ? '保存中...' : '保存'}</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>确认删除</DialogTitle>
            <DialogDescription>删除任务会先停止其正在运行的养号进程，此操作不可撤销。(账号对应的养号令牌会保留)</DialogDescription>
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
