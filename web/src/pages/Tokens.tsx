import { useCallback, useEffect, useState } from 'react';
import { Plus, Copy, Check, KeyRound } from 'lucide-react';
import { api, type ApiToken, type Account } from '@/api';
import { useToast } from '@/components/Toaster';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from '@/components/ui/table';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { BlurFade } from '@/components/magic/blur-fade';

interface TokenForm { name: string; category: string; allowed_accounts: string; blocked_accounts: string; concurrency: number; expires_at: string }
const emptyForm = (): TokenForm => ({ name: '', category: 'customer', allowed_accounts: '', blocked_accounts: '', concurrency: 0, expires_at: '' });

function isoToLocalInput(iso?: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '';
  const off = d.getTimezoneOffset() * 60000;
  return new Date(d.getTime() - off).toISOString().slice(0, 16);
}
function localInputToIso(v: string): string | null {
  if (!v) return null;
  const d = new Date(v);
  return isNaN(d.getTime()) ? null : d.toISOString();
}
function maskToken(token: string): string {
  if (token.length <= 12) return token;
  return token.slice(0, 7) + '...' + token.slice(-4);
}

export default function Tokens() {
  const toast = useToast();
  const [tokens, setTokens] = useState<ApiToken[]>([]);
  const [allAccounts, setAllAccounts] = useState<Account[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<ApiToken | null>(null);
  const [form, setForm] = useState<TokenForm>(emptyForm());
  const [showDelete, setShowDelete] = useState(false);
  const [deleteTargetId, setDeleteTargetId] = useState<number | null>(null);
  const [copiedId, setCopiedId] = useState<number | null>(null);

  const load = useCallback(async () => {
    try { setTokens((await api.listTokens(1, 100)).data ?? []); } catch { setTokens([]); }
  }, []);
  const loadAccounts = useCallback(async () => {
    try { setAllAccounts((await api.listAccounts(1, 100)).data ?? []); } catch { setAllAccounts([]); }
  }, []);
  useEffect(() => { load(); loadAccounts(); }, [load, loadAccounts]);

  const patch = (p: Partial<TokenForm>) => setForm((f) => ({ ...f, ...p }));

  function openCreate() { setEditing(null); setForm(emptyForm()); setShowForm(true); }
  function openEdit(t: ApiToken) {
    setEditing(t);
    setForm({ name: t.name, category: t.category || 'customer', allowed_accounts: t.allowed_accounts, blocked_accounts: t.blocked_accounts, concurrency: t.concurrency ?? 0, expires_at: isoToLocalInput(t.expires_at) });
    setShowForm(true);
  }

  async function save() {
    try {
      const payload = {
        name: form.name,
        category: form.category,
        allowed_accounts: form.allowed_accounts,
        blocked_accounts: form.blocked_accounts,
        concurrency: Number(form.concurrency) || 0,
        expires_at: localInputToIso(form.expires_at),
      };
      if (editing) await api.updateToken(editing.id, payload);
      else await api.createToken(payload);
      setShowForm(false);
      await load();
    } catch (e) { toast((e as Error).message || '保存失败'); }
  }

  async function executeDelete() {
    if (deleteTargetId === null) return;
    try {
      await api.deleteToken(deleteTargetId);
      setShowDelete(false); setDeleteTargetId(null); await load();
    } catch (e) { toast((e as Error).message || '删除失败'); }
  }

  async function toggleStatus(t: ApiToken) {
    try { await api.updateToken(t.id, { status: t.status === 'active' ? 'disabled' : 'active' }); await load(); }
    catch (e) { toast((e as Error).message || '操作失败'); }
  }

  async function copyToken(t: ApiToken) {
    try {
      if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(t.token);
      else {
        const ta = document.createElement('textarea'); ta.value = t.token; ta.style.position = 'fixed'; ta.style.opacity = '0';
        document.body.appendChild(ta); ta.select(); document.execCommand('copy'); document.body.removeChild(ta);
      }
      setCopiedId(t.id); setTimeout(() => setCopiedId(null), 2000);
    } catch { toast('复制失败'); }
  }

  function formatAccountIds(ids: string): string {
    if (!ids) return '不限制';
    return ids.split(',').map((id) => {
      const acc = allAccounts.find((a) => a.id === Number(id.trim()));
      return acc ? (acc.name || acc.email) : `#${id.trim()}`;
    }).join(', ');
  }

  function toggleAccountId(field: 'allowed_accounts' | 'blocked_accounts', id: number) {
    const ids = form[field].split(',').map((s) => s.trim()).filter(Boolean);
    const i = ids.indexOf(String(id));
    if (i >= 0) ids.splice(i, 1); else ids.push(String(id));
    patch({ [field]: ids.join(',') } as Partial<TokenForm>);
  }
  function isSelected(field: 'allowed_accounts' | 'blocked_accounts', id: number) {
    return form[field].split(',').map((s) => s.trim()).includes(String(id));
  }

  // 选择器只展示活跃账号(排除手动停用 disabled 和 401/异常 error)
  const selectable = allAccounts.filter((a) => a.status === 'active');

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold text-neutral-900">令牌管理</h2>
        <Button onClick={openCreate}><Plus className="h-4 w-4" /> 创建令牌</Button>
      </div>

      <BlurFade>
        <div className="overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-sm">
          <Table>
            <TableHeader>
              <TableRow className="bg-neutral-50/60 hover:bg-transparent">
                <TableHead>令牌</TableHead>
                <TableHead>Token</TableHead>
                <TableHead>可用账号</TableHead>
                <TableHead>不可用账号</TableHead>
                <TableHead>并发</TableHead>
                <TableHead>过期时间</TableHead>
                <TableHead>状态</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tokens.map((t) => (
                <TableRow key={t.id} className="align-top">
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <div className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-lg bg-indigo-50 text-indigo-600"><KeyRound className="h-3.5 w-3.5" /></div>
                      <div>
                        <div className="flex items-center gap-1.5">
                          <p className="text-sm font-medium text-neutral-900">{t.name || '未命名令牌'}</p>
                          {t.category === 'warmup' && (
                            <Badge className="border-amber-200 bg-amber-50 text-amber-700">养号</Badge>
                          )}
                        </div>
                        <p className="text-xs text-neutral-400">{new Date(t.created_at).toLocaleDateString('zh-CN')}</p>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-1.5">
                      <code className="font-mono text-[11px] text-neutral-500">{maskToken(t.token)}</code>
                      <button onClick={() => copyToken(t)} className="text-neutral-400 hover:text-neutral-700" title="复制完整令牌">
                        {copiedId === t.id ? <Check className="h-3.5 w-3.5 text-emerald-500" /> : <Copy className="h-3.5 w-3.5" />}
                      </button>
                    </div>
                  </TableCell>
                  <TableCell className="max-w-[160px] truncate text-xs text-neutral-600" title={formatAccountIds(t.allowed_accounts)}>{formatAccountIds(t.allowed_accounts)}</TableCell>
                  <TableCell className="max-w-[160px] truncate text-xs text-neutral-600" title={formatAccountIds(t.blocked_accounts)}>{formatAccountIds(t.blocked_accounts)}</TableCell>
                  <TableCell className="text-sm text-neutral-700">{t.concurrency > 0 ? t.concurrency : '不限'}</TableCell>
                  <TableCell className="text-xs text-neutral-600">{t.expires_at ? new Date(t.expires_at).toLocaleString('zh-CN') : '永不过期'}</TableCell>
                  <TableCell>
                    <Badge className={t.status === 'active' ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-neutral-200 bg-neutral-100 text-neutral-500'}>
                      <span className={cn('h-1.5 w-1.5 rounded-full', t.status === 'active' ? 'bg-emerald-500' : 'bg-neutral-400')} />
                      {t.status === 'active' ? '活跃' : '停用'}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center justify-end gap-0.5">
                      <Button variant="ghost" size="sm" onClick={() => openEdit(t)} className="h-7 px-2 text-xs text-neutral-500">编辑</Button>
                      <Button variant="ghost" size="sm" onClick={() => toggleStatus(t)} className={cn('h-7 px-2 text-xs', t.status === 'active' ? 'text-amber-600 hover:bg-amber-50' : 'text-emerald-600 hover:bg-emerald-50')}>
                        {t.status === 'active' ? '停用' : '启用'}
                      </Button>
                      <Button variant="ghost" size="sm" onClick={() => { setDeleteTargetId(t.id); setShowDelete(true); }} className="h-7 px-2 text-xs text-red-500 hover:bg-red-50">删除</Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
              {tokens.length === 0 && (
                <TableRow className="border-0 hover:bg-transparent">
                  <TableCell colSpan={8} className="py-16">
                    <div className="flex flex-col items-center justify-center text-neutral-400">
                      <div className="mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-neutral-100"><KeyRound className="h-6 w-6 text-indigo-400" /></div>
                      <p className="text-sm">暂无令牌，点击"创建令牌"开始</p>
                    </div>
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      </BlurFade>

      {/* 新建/编辑令牌 */}
      <Dialog open={showForm} onOpenChange={setShowForm}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{editing ? '编辑令牌' : '创建令牌'}</DialogTitle>
            <DialogDescription>{editing ? '修改令牌设置' : '创建新的 API 令牌，令牌将自动生成'}</DialogDescription>
          </DialogHeader>
          <form onSubmit={(e) => { e.preventDefault(); save(); }} className="mt-1 space-y-4">
            <div className="space-y-2">
              <Label>分类</Label>
              <div className="flex gap-1.5">
                {([['customer', '客户用'], ['warmup', '养号专用']] as const).map(([val, label]) => (
                  <button key={val} type="button" onClick={() => patch({ category: val })}
                    className={cn('flex-1 rounded-md border px-3 py-1.5 text-xs transition-colors',
                      form.category === val ? 'border-indigo-300 bg-indigo-50 text-indigo-700' : 'border-neutral-200 bg-neutral-50 text-neutral-500 hover:border-indigo-300')}>
                    {label}
                  </button>
                ))}
              </div>
              {form.category === 'warmup' && (
                <p className="text-[11px] text-amber-600">养号令牌建议在下方"可用账号"中只选一个账号（一个 key 绑一个账号）。</p>
              )}
            </div>
            <div className="space-y-2">
              <Label>备注名（选填）</Label>
              <Input value={form.name} onChange={(e) => patch({ name: e.target.value })} placeholder="例如：生产环境、测试用" />
            </div>
            <div className="space-y-2">
              <Label>可用账号（选填，留空不限制）</Label>
              <Input value={form.allowed_accounts} onChange={(e) => patch({ allowed_accounts: e.target.value })} placeholder="账号 ID，逗号分隔" />
              {selectable.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {selectable.map((a) => (
                    <button key={a.id} type="button" onClick={() => toggleAccountId('allowed_accounts', a.id)}
                      className={cn('rounded-md border px-2 py-0.5 text-[10px] transition-colors',
                        isSelected('allowed_accounts', a.id) ? 'border-indigo-300 bg-indigo-50 text-indigo-700' : 'border-neutral-200 bg-neutral-50 text-neutral-500 hover:border-indigo-300')}>
                      #{a.id} {a.name || a.email}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className="space-y-2">
              <Label>不可用账号（选填）</Label>
              <Input value={form.blocked_accounts} onChange={(e) => patch({ blocked_accounts: e.target.value })} placeholder="账号 ID，逗号分隔" />
              {selectable.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {selectable.map((a) => (
                    <button key={a.id} type="button" onClick={() => toggleAccountId('blocked_accounts', a.id)}
                      className={cn('rounded-md border px-2 py-0.5 text-[10px] transition-colors',
                        isSelected('blocked_accounts', a.id) ? 'border-red-200 bg-red-50 text-red-500' : 'border-neutral-200 bg-neutral-50 text-neutral-500 hover:border-red-200')}>
                      #{a.id} {a.name || a.email}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-2">
                <Label>并发上限（0=不限）</Label>
                <Input type="number" min={0} value={form.concurrency} onChange={(e) => patch({ concurrency: Number(e.target.value) })} placeholder="0" />
              </div>
              <div className="space-y-2">
                <Label>过期时间（选填）</Label>
                <Input type="datetime-local" value={form.expires_at} onChange={(e) => patch({ expires_at: e.target.value })} />
              </div>
            </div>
            <DialogFooter className="gap-2 pt-2">
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>取消</Button>
              <Button type="submit">保存</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>确认删除</DialogTitle>
            <DialogDescription>此操作不可撤销，删除后使用该令牌的客户端将无法访问。</DialogDescription>
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
