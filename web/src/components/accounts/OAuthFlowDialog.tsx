import { useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { api, type OAuthExchangeResult } from '@/api';
import { useToast } from '@/components/Toaster';
import { cn } from '@/lib/utils';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { emptyForm, type FormState } from './form';

type Mode = 'oauth' | 'setup_token' | 'session_key';
type Step = 'generate' | 'exchange' | 'done';

async function copyText(text: string, toast: (m: string, t?: 'success' | 'error') => void) {
  if (!text) return toast('没有可复制的内容');
  if (navigator.clipboard && window.isSecureContext) {
    try { await navigator.clipboard.writeText(text); return toast('已复制', 'success'); } catch { /* fallthrough */ }
  }
  try {
    const ta = document.createElement('textarea');
    ta.value = text; ta.style.position = 'fixed'; ta.style.opacity = '0';
    document.body.appendChild(ta); ta.select(); document.execCommand('copy'); document.body.removeChild(ta);
    toast('已复制', 'success');
  } catch { toast('复制失败'); }
}

function pill(active: boolean, tone: 'amber' | 'terra' | 'emerald') {
  const on = {
    amber: 'bg-amber-50 border-amber-300 text-amber-700',
    terra: 'bg-indigo-50 border-indigo-300 text-indigo-700',
    emerald: 'bg-emerald-50 border-emerald-300 text-emerald-700',
  }[tone];
  return cn(
    'flex-1 rounded-lg border px-3 py-2 text-sm font-medium transition-all',
    active ? on : 'border-border bg-neutral-50 text-muted-foreground hover:text-foreground',
  );
}

export function OAuthFlowDialog({
  open,
  onOpenChange,
  onApply,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onApply: (form: FormState) => void;
}) {
  const toast = useToast();
  const [mode, setMode] = useState<Mode>('oauth');
  const [step, setStep] = useState<Step>('generate');
  const [proxyUrl, setProxyUrl] = useState('');
  const [sessionId, setSessionId] = useState('');
  const [authUrl, setAuthUrl] = useState('');
  const [code, setCode] = useState('');
  const [sessionKey, setSessionKey] = useState('');
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<OAuthExchangeResult | null>(null);

  function reset() {
    setMode('oauth'); setStep('generate'); setProxyUrl(''); setSessionId('');
    setAuthUrl(''); setCode(''); setSessionKey(''); setLoading(false); setResult(null);
  }

  function close(o: boolean) {
    if (!o) reset();
    onOpenChange(o);
  }

  async function generate() {
    setLoading(true);
    try {
      const proxy = proxyUrl.trim() || undefined;
      const res = mode === 'oauth' ? await api.generateAuthUrl(proxy) : await api.generateSetupTokenUrl(proxy);
      setSessionId(res.session_id); setAuthUrl(res.auth_url); setStep('exchange');
    } catch (e) { toast((e as Error).message || '生成授权链接失败'); }
    setLoading(false);
  }

  async function exchange() {
    if (!code.trim()) return toast('请输入授权码');
    setLoading(true);
    try {
      const res = mode === 'oauth'
        ? await api.exchangeCode(sessionId, code.trim())
        : await api.exchangeSetupTokenCode(sessionId, code.trim());
      setResult(res); setStep('done');
    } catch (e) { toast((e as Error).message || '交换 Token 失败'); }
    setLoading(false);
  }

  async function exchangeKey() {
    if (!sessionKey.trim()) return toast('请粘贴 sessionKey');
    setLoading(true);
    try {
      const res = await api.exchangeSessionKey(sessionKey.trim(), proxyUrl.trim() || undefined);
      setResult(res); setStep('done');
    } catch (e) { toast((e as Error).message || 'Session Key 授权失败'); }
    setLoading(false);
  }

  function apply() {
    if (!result) return;
    const isSetup = mode === 'setup_token';
    const f = emptyForm();
    f.email = result.email_address || '';
    f.auth_type = isSetup ? 'setup_token' : 'oauth';
    f.setup_token = isSetup ? result.access_token : '';
    f.access_token = isSetup ? '' : (result.access_token || '');
    f.refresh_token = isSetup ? '' : (result.refresh_token || '');
    f.expires_at = (!isSetup && result.expires_at) ? String(result.expires_at * 1000) : '';
    f.proxy_url = proxyUrl || '';
    f.account_uuid = result.account_uuid || '';
    f.organization_uuid = result.organization_uuid || '';
    reset();
    onApply(f);
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>OAuth 授权</DialogTitle>
          <DialogDescription>通过浏览器完成 OAuth 授权，自动获取 Token 和账号信息</DialogDescription>
        </DialogHeader>

        <div className="mt-1 flex-1 space-y-4 overflow-y-auto pr-1">
          {step === 'generate' && (
            <>
              <div className="space-y-2">
                <Label>授权类型</Label>
                <div className="flex gap-2">
                  <button type="button" onClick={() => setMode('oauth')} className={pill(mode === 'oauth', 'amber')}>OAuth（完整）</button>
                  <button type="button" onClick={() => setMode('setup_token')} className={pill(mode === 'setup_token', 'terra')}>Setup Token</button>
                  <button type="button" onClick={() => setMode('session_key')} className={pill(mode === 'session_key', 'emerald')}>Session Key</button>
                </div>
                <p className="text-xs text-muted-foreground">
                  {mode === 'oauth' ? '完整 scope，支持 profile、用量查询等'
                    : mode === 'setup_token' ? '仅 user:inference scope，有效期 1 年'
                    : '粘贴 claude.ai 的 sessionKey（sk-ant-sid01-…），自动完成授权，无需浏览器'}
                </p>
              </div>
              <div className="space-y-2">
                <Label>代理地址（选填）</Label>
                <Input value={proxyUrl} onChange={(e) => setProxyUrl(e.target.value)} placeholder="http:// 或 socks5://" />
              </div>
              {mode === 'session_key' ? (
                <>
                  <div className="space-y-2">
                    <Label>Session Key <span className="text-red-400">*</span></Label>
                    <Textarea rows={2} value={sessionKey} onChange={(e) => setSessionKey(e.target.value)} placeholder="粘贴 claude.ai 的 sessionKey（sk-ant-sid01-…）" className="font-mono text-sm" />
                    <p className="text-xs text-muted-foreground">浏览器登录 claude.ai 后，从 Cookie 里复制 sessionKey 的值</p>
                  </div>
                  <Button onClick={exchangeKey} disabled={loading || !sessionKey.trim()} className="w-full bg-emerald-500 hover:bg-emerald-600">
                    {loading ? '授权中...' : '授权并录号'}
                  </Button>
                </>
              ) : (
                <Button onClick={generate} disabled={loading} className="w-full bg-amber-500 hover:bg-amber-600 text-black">
                  {loading ? '生成中...' : '生成授权链接'}
                </Button>
              )}
            </>
          )}

          {step === 'exchange' && (
            <>
              <div className="space-y-2">
                <Label>授权链接</Label>
                <div className="relative">
                  <Textarea readOnly rows={3} value={authUrl} className="pr-16 font-mono text-xs" />
                  <button type="button" onClick={() => copyText(authUrl, toast)} className="absolute right-2 top-2 rounded-md bg-primary px-2 py-1 text-xs text-primary-foreground hover:bg-primary/90">复制</button>
                </div>
                <a href={authUrl} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-xs text-amber-400 underline">
                  点击打开授权页面 <ExternalLink className="h-3 w-3" />
                </a>
              </div>
              <div className="space-y-1 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
                <p className="font-medium">操作步骤：</p>
                <ol className="list-inside list-decimal space-y-0.5">
                  <li>点击上方链接或复制到浏览器打开</li>
                  <li>完成 Claude 登录授权</li>
                  <li>授权完成后，从回调页面复制授权码</li>
                  <li>将授权码粘贴到下方输入框</li>
                </ol>
              </div>
              <div className="space-y-2">
                <Label>授权码 <span className="text-red-400">*</span></Label>
                <Textarea rows={2} value={code} onChange={(e) => setCode(e.target.value)} placeholder="粘贴授权码（authorization code）" className="font-mono text-sm" />
              </div>
              <div className="flex gap-2">
                <Button variant="ghost" onClick={() => setStep('generate')}>返回</Button>
                <Button onClick={exchange} disabled={loading || !code.trim()} className="flex-1 bg-amber-500 hover:bg-amber-600 text-black">
                  {loading ? '交换中...' : '交换 Token'}
                </Button>
              </div>
            </>
          )}

          {step === 'done' && result && (
            <>
              <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-sm font-medium text-emerald-300">授权成功</div>
              <div className="space-y-3">
                {result.email_address && <Field label="邮箱"><span className="text-sm">{result.email_address}</span></Field>}
                {result.account_uuid && <Field label="Account UUID"><span className="break-all font-mono text-xs text-muted-foreground">{result.account_uuid}</span></Field>}
                {result.organization_uuid && <Field label="Organization UUID"><span className="break-all font-mono text-xs text-muted-foreground">{result.organization_uuid}</span></Field>}
                <Field label="Access Token">
                  <div className="flex items-center gap-2">
                    <span className="flex-1 truncate font-mono text-xs text-muted-foreground">{result.access_token.slice(0, 30)}...</span>
                    <button type="button" onClick={() => copyText(result.access_token, toast)} className="rounded bg-secondary px-2 py-0.5 text-[10px] hover:bg-secondary/70">复制</button>
                  </div>
                </Field>
                {result.refresh_token && (
                  <Field label="Refresh Token">
                    <div className="flex items-center gap-2">
                      <span className="flex-1 truncate font-mono text-xs text-muted-foreground">{result.refresh_token.slice(0, 30)}...</span>
                      <button type="button" onClick={() => copyText(result.refresh_token, toast)} className="rounded bg-secondary px-2 py-0.5 text-[10px] hover:bg-secondary/70">复制</button>
                    </div>
                  </Field>
                )}
                <Field label="Scope"><span className="text-xs text-muted-foreground">{result.scope || '—'}</span></Field>
                <Field label="过期时间"><span className="text-xs text-muted-foreground">{new Date(result.expires_at * 1000).toLocaleString('zh-CN')}</span></Field>
              </div>
              <div className="flex gap-2 pt-2">
                <Button variant="ghost" onClick={() => close(false)}>关闭</Button>
                <Button onClick={apply} className="flex-1">填入并创建账号</Button>
              </div>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</p>
      {children}
    </div>
  );
}
