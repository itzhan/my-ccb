import type { Account } from '@/api';
import { cn } from '@/lib/utils';
import { formatExpiresAt } from '@/lib/format';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { clientTypeOptions, subscriptionOptions, type FormState } from './form';

type Tone = 'terra' | 'amber' | 'emerald' | 'neutral';
const toneOn: Record<Tone, string> = {
  terra: 'bg-indigo-50 border-indigo-300 text-indigo-700',
  amber: 'bg-amber-50 border-amber-300 text-amber-700',
  emerald: 'bg-emerald-50 border-emerald-300 text-emerald-700',
  neutral: 'bg-neutral-100 border-neutral-300 text-neutral-700',
};
function seg(active: boolean, tone: Tone = 'terra') {
  return cn(
    'flex-1 rounded-lg border px-3 py-2 text-sm font-medium transition-all',
    active ? toneOn[tone] : 'border-border bg-secondary/40 text-muted-foreground hover:text-foreground',
  );
}

export function AccountFormDialog({
  open,
  onOpenChange,
  editing,
  form,
  patch,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  editing: Account | null;
  form: FormState;
  patch: (p: Partial<FormState>) => void;
  onSubmit: () => void;
}) {
  function toggleClient(v: string) {
    const arr = form.allowed_client_types;
    patch({ allowed_client_types: arr.includes(v) ? arr.filter((x) => x !== v) : [...arr, v] });
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{editing ? '编辑账号' : '添加账号'}</DialogTitle>
          <DialogDescription>{editing ? '修改账号信息，凭证留空表示不更改' : '填写新账号信息'}</DialogDescription>
        </DialogHeader>

        <form
          onSubmit={(e) => { e.preventDefault(); onSubmit(); }}
          className="mt-1 flex-1 space-y-4 overflow-y-auto pr-1"
        >
          <div className="space-y-2">
            <Label>备注名（选填）</Label>
            <Input value={form.name} onChange={(e) => patch({ name: e.target.value })} />
          </div>
          <div className="space-y-2">
            <Label>邮箱 <span className="text-red-400">*</span></Label>
            <Input required value={form.email} onChange={(e) => patch({ email: e.target.value })} />
          </div>

          <div className="space-y-2">
            <Label>认证方式</Label>
            <div className="flex gap-2">
              <button type="button" onClick={() => patch({ auth_type: 'setup_token' })} className={seg(form.auth_type === 'setup_token', 'terra')}>Setup Token</button>
              <button type="button" onClick={() => patch({ auth_type: 'oauth' })} className={seg(form.auth_type === 'oauth', 'amber')}>OAuth</button>
            </div>
          </div>

          {form.auth_type === 'setup_token' ? (
            <div className="space-y-2">
              <Label>Setup Token (sk-ant-oat01-...) {!editing && <span className="text-red-400">*</span>}</Label>
              <Textarea rows={3} required={!editing} value={form.setup_token} onChange={(e) => patch({ setup_token: e.target.value })} placeholder={editing ? '留空保持不变' : ''} className="font-mono text-sm" />
            </div>
          ) : (
            <>
              <div className="space-y-2">
                <Label>Access Token（选填）</Label>
                <Textarea rows={2} value={form.access_token} onChange={(e) => patch({ access_token: e.target.value })} placeholder={editing ? '留空保持不变' : '已有 access token 时可直接填写'} className="font-mono text-sm" />
              </div>
              <div className="space-y-2">
                <Label>Refresh Token <span className="text-red-400">*</span></Label>
                <Textarea rows={2} required={!editing} value={form.refresh_token} onChange={(e) => patch({ refresh_token: e.target.value })} placeholder={editing ? '留空保持不变' : ''} className="font-mono text-sm" />
              </div>
              <div className="space-y-2">
                <Label>Expires At（毫秒时间戳，选填）</Label>
                <Input inputMode="numeric" value={form.expires_at} onChange={(e) => patch({ expires_at: e.target.value })} placeholder="例如：1743600000000" className="font-mono text-sm" />
              </div>
            </>
          )}

          {editing?.auth_type === 'oauth' && editing.expires_at && (
            <div className="rounded-lg bg-secondary/50 px-3 py-2 text-xs text-muted-foreground">当前过期时间：{formatExpiresAt(editing.expires_at)}</div>
          )}
          {editing?.auth_type === 'oauth' && editing.auth_error && (
            <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">最近认证错误：{editing.auth_error}</div>
          )}

          <div className="space-y-2">
            <Label>代理地址（选填）</Label>
            <Input value={form.proxy_url} onChange={(e) => patch({ proxy_url: e.target.value })} placeholder="http:// 或 socks5://" />
          </div>

          <div className="space-y-2">
            <Label>Billing 模式</Label>
            <div className="flex gap-2">
              <button type="button" onClick={() => patch({ billing_mode: 'strip' })} className={seg(form.billing_mode === 'strip', 'terra')}>清除 (Strip)</button>
              <button type="button" onClick={() => patch({ billing_mode: 'rewrite' })} className={seg(form.billing_mode === 'rewrite', 'amber')}>重写 (Rewrite)</button>
            </div>
          </div>

          <div className="space-y-2">
            <Label>订阅类型（选填，强烈推荐）</Label>
            <div className="flex flex-wrap gap-2">
              {subscriptionOptions.map((opt) => (
                <button key={opt.value} type="button" onClick={() => patch({ subscription_type: opt.value })}
                  className={cn('rounded-lg border px-3 py-1.5 text-xs font-medium transition-all',
                    form.subscription_type === opt.value ? toneOn.terra : 'border-border bg-secondary/40 text-muted-foreground hover:text-foreground')}>
                  {opt.label}
                </button>
              ))}
            </div>
          </div>

          <div className="flex gap-4">
            <div className="flex-1 space-y-2">
              <Label>Account UUID（选填）</Label>
              <Input value={form.account_uuid} onChange={(e) => patch({ account_uuid: e.target.value })} placeholder="OAuth account UUID" className="font-mono text-sm" />
            </div>
            <div className="flex-1 space-y-2">
              <Label>Organization UUID（选填）</Label>
              <Input value={form.organization_uuid} onChange={(e) => patch({ organization_uuid: e.target.value })} placeholder="OAuth organization UUID" className="font-mono text-sm" />
            </div>
          </div>

          <div className="space-y-2">
            <Label>自动遥测</Label>
            <div className="flex gap-2">
              <button type="button" onClick={() => patch({ auto_telemetry: false })} className={seg(!form.auto_telemetry, 'neutral')}>关闭</button>
              <button type="button" onClick={() => patch({ auto_telemetry: true })} className={seg(form.auto_telemetry, 'emerald')}>开启</button>
            </div>
            <p className="text-xs text-muted-foreground">开启后由网关代替客户端发送遥测请求</p>
          </div>

          <div className="flex gap-4">
            <div className="flex-1 space-y-2">
              <Label>并发数</Label>
              <Input type="number" min={1} value={form.concurrency} onChange={(e) => patch({ concurrency: Number(e.target.value) })} />
            </div>
            <div className="flex-1 space-y-2">
              <Label>最大并发会话(0=不限)</Label>
              <Input type="number" min={0} value={form.max_sessions} onChange={(e) => patch({ max_sessions: Number(e.target.value) })} />
            </div>
            <div className="flex-1 space-y-2">
              <Label>优先级</Label>
              <Input type="number" min={1} value={form.priority} onChange={(e) => patch({ priority: Number(e.target.value) })} />
            </div>
          </div>

          <div className="space-y-2">
            <Label>RPM 限制 <span className="text-xs text-muted-foreground">(0 = 不限)</span></Label>
            <Input type="number" min={0} value={form.rpm_limit} onChange={(e) => patch({ rpm_limit: Number(e.target.value) })} placeholder="0" />
          </div>

          <div className="space-y-2">
            <Label>允许的客户端类型 <span className="text-xs text-muted-foreground">(不勾 = 全部放行)</span></Label>
            <div className="flex flex-wrap gap-2">
              {clientTypeOptions.map((opt) => (
                <button key={opt.value} type="button" onClick={() => toggleClient(opt.value)}
                  className={cn('rounded-md border px-2.5 py-1 text-xs transition-colors',
                    form.allowed_client_types.includes(opt.value) ? 'border-primary bg-primary text-primary-foreground' : 'border-border bg-secondary/40 text-muted-foreground')}>
                  {opt.label}
                </button>
              ))}
            </div>
            <p className="text-[10px] text-muted-foreground">收紧后,只有勾选的类型能用本账号;其它类型自动换号,全不收则 403。例:只勾 cli = 只许真人终端。</p>
          </div>

          <div className="space-y-2 border-t border-border pt-3">
            <Label>身份模拟</Label>
            <div className="flex gap-2">
              <button type="button" onClick={() => patch({ identity_mode: 'passthrough' })} className={seg(form.identity_mode === 'passthrough', 'terra')}>透传（单人）</button>
              <button type="button" onClick={() => patch({ identity_mode: 'normalize' })} className={seg(form.identity_mode === 'normalize', 'emerald')}>归一化（多人共号）</button>
            </div>
            <p className="text-xs text-muted-foreground">
              {form.identity_mode === 'normalize'
                ? '多人共号：把每个用户的 home用户名/git/OS/device_id 统一成下面这套虚拟身份，让一个号始终像同一个人。'
                : '单人：客户端请求原样透传，最高保真（推荐你自己用）。'}
            </p>

            {form.identity_mode === 'normalize' && (
              <>
                <div className="grid grid-cols-2 gap-3 pt-1">
                  <div className="space-y-1">
                    <Label className="text-xs">虚拟用户名（留空自动派生）</Label>
                    <Input value={form.virtual_user} onChange={(e) => patch({ virtual_user: e.target.value })} placeholder="如 alexc" />
                  </div>
                  <div className="space-y-1">
                    <Label className="text-xs">虚拟 git 用户名（留空自动派生）</Label>
                    <Input value={form.virtual_git_name} onChange={(e) => patch({ virtual_git_name: e.target.value })} placeholder="如 Alex Carter" />
                  </div>
                </div>
                <div className="space-y-1 pt-1">
                  <Label className="text-xs">版本重新吸取周期（天，0=永久只吸一次）</Label>
                  <Input type="number" min={0} value={form.recapture_days} onChange={(e) => patch({ recapture_days: Number(e.target.value) })} placeholder="0" />
                  <p className="text-[11px] text-muted-foreground">CC 版本/SDK 版本从该号第一个请求吸取并复用；周期到后由下一个请求重吸（模拟升级 CC）。device_id/系统等仍用预设。</p>
                </div>
              </>
            )}

            {editing?.effective_identity && (
              <div className="space-y-1 rounded-lg border border-border bg-secondary/40 p-3 text-xs text-muted-foreground">
                <p className="font-medium text-foreground/80">当前生效的虚拟身份</p>
                <p>虚拟用户：<span className="font-mono text-foreground">{editing.effective_identity.virtual_user}</span> · git：<span className="font-mono text-foreground">{editing.effective_identity.git_name}</span></p>
                <p>机器：{editing.effective_identity.platform} / {editing.effective_identity.arch} · device_id：<span className="font-mono">{editing.effective_identity.device_id.slice(0, 16)}…</span></p>
                {editing.identity_mode === 'normalize' && (
                  <p>版本吸取：{editing.identity_captured_at
                    ? <><span className="text-emerald-400">已吸取</span> · v{String(editing.canonical_env?.version ?? '')} · {new Date(editing.identity_captured_at).toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })}</>
                    : <span className="text-amber-400">待吸取（首个请求时种入）</span>}</p>
                )}
              </div>
            )}
          </div>

          <DialogFooter className="gap-2 pt-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>取消</Button>
            <Button type="submit">保存</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
