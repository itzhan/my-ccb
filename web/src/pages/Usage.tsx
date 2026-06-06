import { Fragment, useCallback, useEffect, useMemo, useState } from 'react';
import { ChevronRight, ChevronDown, RotateCcw } from 'lucide-react';
import { api, type UsageLog, type UsageStat, type Account, type ApiToken } from '@/api';
import { cn } from '@/lib/utils';
import { calculateCost, fmtCost } from '@/lib/pricing';
import { usePolling } from '@/hooks/usePolling';
import { Button } from '@/components/ui/button';
import { BlurFade } from '@/components/magic/blur-fade';

const PAGE_SIZE = 50;
const selectCls = 'h-9 rounded-lg border border-neutral-200 bg-white px-2 text-sm text-neutral-700 focus:outline-none focus:ring-2 focus:ring-ring/30';
const fmt = (n: number) => (n ?? 0).toLocaleString('en-US');
const todayUtc = () => new Date().toISOString().slice(0, 10);
// 账号名只显示前几个字符,省空间
const truncName = (s: string, n = 6) => (s.length > n ? s.slice(0, n) + '…' : s);
const rowCost = (r: UsageLog) => {
  // SSE usage 若未区分 5m/1h,把合并的 cache_creation 当作 5m 缓存写入
  let cw5m = r.cache_creation_5m_tokens || 0;
  const cw1h = r.cache_creation_1h_tokens || 0;
  if (cw5m === 0 && cw1h === 0) cw5m = r.cache_creation_tokens || 0;
  return calculateCost(r.model, r.input_tokens, r.output_tokens, cw5m, cw1h, r.cache_read_tokens);
};
function pretty(s: string): string {
  if (!s) return '';
  try { return JSON.stringify(JSON.parse(s), null, 2); } catch { return s; }
}

function StatCard({ title, stat }: { title: string; stat: UsageStat | null }) {
  const cells = [
    { v: stat?.input_tokens, label: '输入', c: 'text-neutral-900' },
    { v: stat?.output_tokens, label: '输出', c: 'text-neutral-900' },
    { v: stat?.cache_read_tokens, label: '缓存读', c: 'text-indigo-600' },
    { v: stat?.cache_creation_tokens, label: '缓存创建', c: 'text-indigo-600' },
  ];
  return (
    <div className="rounded-2xl border border-neutral-200 bg-white p-5 shadow-sm">
      <p className="mb-3 text-xs font-medium text-neutral-500">{title}</p>
      <div className="grid grid-cols-4 gap-3 text-center">
        {cells.map((c) => (
          <div key={c.label}>
            <p className={cn('text-lg font-semibold tabular-nums', c.c)}>{fmt(c.v || 0)}</p>
            <p className="text-[11px] text-neutral-500">{c.label}</p>
          </div>
        ))}
      </div>
      <p className="mt-3 text-center text-[11px] text-neutral-500">{fmt(stat?.req_count || 0)} 次调用</p>
    </div>
  );
}

export default function Usage() {
  const [logs, setLogs] = useState<UsageLog[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [loading, setLoading] = useState(false);

  const [todayStat, setTodayStat] = useState<UsageStat | null>(null);
  const [allStat, setAllStat] = useState<UsageStat | null>(null);

  const [accounts, setAccounts] = useState<Account[]>([]);
  const [tokens, setTokens] = useState<ApiToken[]>([]);
  const [models, setModels] = useState<string[]>([]);

  const [fAccount, setFAccount] = useState('');
  const [fToken, setFToken] = useState('');
  const [fModel, setFModel] = useState('');
  const [fResult, setFResult] = useState('');
  const [fStart, setFStart] = useState('');
  const [fEnd, setFEnd] = useState('');

  const [expanded, setExpanded] = useState<number | null>(null);

  const accountName = useCallback((id: number) => accounts.find((a) => a.id === id)?.name || (id ? `#${id}` : '-'), [accounts]);
  const tokenName = useCallback((id: number) => tokens.find((t) => t.id === id)?.name || (id ? `#${id}` : '-'), [tokens]);

  const totalPages = useMemo(() => Math.max(1, Math.ceil(total / PAGE_SIZE)), [total]);

  // 初始化:账号/令牌/模型(基本不变,无需轮询)
  useEffect(() => {
    (async () => {
      try {
        const [accRes, tokRes] = await Promise.all([api.listAccounts(1, 200), api.listTokens(1, 200)]);
        setAccounts(accRes.data ?? []); setTokens(tokRes.data ?? []);
      } catch { /* ignore */ }
      try { setModels(((await api.getUsageStats({ group_by: 'model' })).data ?? []).map((d) => d.key).filter(Boolean)); } catch { /* ignore */ }
    })();
  }, []);

  const loadStats = useCallback(async () => {
    try {
      const [t, a] = await Promise.all([
        api.getUsageStats({ group_by: 'total', start: todayUtc(), end: todayUtc() }),
        api.getUsageStats({ group_by: 'total' }),
      ]);
      setTodayStat(t.data?.[0] ?? null); setAllStat(a.data?.[0] ?? null);
    } catch { /* ignore */ }
  }, []);

  const loadLogs = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const res = await api.getUsageLogs({
        page, page_size: PAGE_SIZE,
        account_id: fAccount === '' ? undefined : Number(fAccount),
        token_id: fToken === '' ? undefined : Number(fToken),
        model: fModel || undefined,
        result: fResult || undefined,
        start: fStart ? `${fStart}T00:00:00Z` : undefined,
        end: fEnd ? `${fEnd}T23:59:59Z` : undefined,
      });
      setLogs(res.data ?? []); setTotal(res.total ?? 0);
    } catch { setLogs([]); setTotal(0); }
    finally { if (!silent) setLoading(false); }
  }, [page, fAccount, fToken, fModel, fResult, fStart, fEnd]);

  // 任意筛选/翻页变化:带加载态查询
  useEffect(() => { loadLogs(); }, [loadLogs]);
  // 实时刷新:每 5 秒静默拉取最新记录与汇总(不闪烁加载态)
  usePolling(() => { loadLogs(true); loadStats(); }, 5000);

  function reset() {
    setFAccount(''); setFToken(''); setFModel(''); setFResult(''); setFStart(''); setFEnd(''); setPage(1);
  }
  const onFilter = (setter: (v: string) => void) => (e: React.ChangeEvent<HTMLSelectElement | HTMLInputElement>) => {
    setter(e.target.value); setPage(1);
  };

  return (
    <BlurFade>
      <div className="space-y-6">
        {/* 汇总 */}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <StatCard title="今日用量 (UTC)" stat={todayStat} />
          <StatCard title="累计用量" stat={allStat} />
        </div>

        {/* 过滤 */}
        <div className="flex flex-wrap items-end gap-3 rounded-2xl border border-neutral-200 bg-white p-4 shadow-sm">
          <Field label="账号"><select className={selectCls} value={fAccount} onChange={onFilter(setFAccount)}><option value="">全部</option>{accounts.map((a) => <option key={a.id} value={a.id}>{a.name || a.email}</option>)}</select></Field>
          <Field label="令牌"><select className={selectCls} value={fToken} onChange={onFilter(setFToken)}><option value="">全部</option>{tokens.map((t) => <option key={t.id} value={t.id}>{t.name || `#${t.id}`}</option>)}</select></Field>
          <Field label="结果"><select className={selectCls} value={fResult} onChange={onFilter(setFResult)}><option value="">全部</option><option value="success">仅成功</option><option value="error">仅失败</option></select></Field>
          <Field label="模型"><select className={selectCls} value={fModel} onChange={onFilter(setFModel)}><option value="">全部</option>{models.map((m) => <option key={m} value={m}>{m}</option>)}</select></Field>
          <Field label="起始日期"><input type="date" className={selectCls} value={fStart} onChange={onFilter(setFStart)} /></Field>
          <Field label="结束日期"><input type="date" className={selectCls} value={fEnd} onChange={onFilter(setFEnd)} /></Field>
          <Button variant="outline" className="h-9" onClick={reset}><RotateCcw className="h-4 w-4" /> 重置</Button>
        </div>

        {/* 明细表 */}
        <div className="overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-sm">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-neutral-200 text-left text-xs text-neutral-500">
                  <th className="px-4 py-3 font-medium">时间</th>
                  <th className="px-4 py-3 font-medium">令牌</th>
                  <th className="px-4 py-3 font-medium">账号</th>
                  <th className="px-4 py-3 font-medium">模型</th>
                  <th className="px-4 py-3 text-right font-medium">输入</th>
                  <th className="px-4 py-3 text-right font-medium">输出</th>
                  <th className="px-4 py-3 text-right font-medium">缓存读</th>
                  <th className="px-4 py-3 text-right font-medium">缓存创建</th>
                  <th className="px-4 py-3 text-right font-medium">扣费</th>
                  <th className="px-4 py-3 text-right font-medium">耗时</th>
                  <th className="px-4 py-3 text-right font-medium">状态</th>
                </tr>
              </thead>
              <tbody>
                {loading && <tr><td colSpan={11} className="px-4 py-8 text-center text-neutral-500">加载中…</td></tr>}
                {!loading && logs.length === 0 && <tr><td colSpan={11} className="px-4 py-8 text-center text-neutral-500">暂无调用记录</td></tr>}
                {!loading && logs.map((r) => {
                  const ok = r.status_code >= 200 && r.status_code < 300;
                  const open = expanded === r.id;
                  return (
                    <Fragment key={r.id}>
                      <tr className={cn('cursor-pointer border-b border-neutral-100 hover:bg-neutral-50', open && 'border-b-0')} onClick={() => setExpanded(open ? null : r.id)}>
                        <td className="whitespace-nowrap px-4 py-2.5 text-neutral-800">
                          {open ? <ChevronDown className="mr-1 inline h-3 w-3 text-neutral-400" /> : <ChevronRight className="mr-1 inline h-3 w-3 text-neutral-400" />}
                          {new Date(r.created_at).toLocaleString()}
                        </td>
                        <td className="px-4 py-2.5 text-neutral-500">{tokenName(r.token_id)}</td>
                        <td className="px-4 py-2.5 text-neutral-500" title={accountName(r.account_id)}>{truncName(accountName(r.account_id))}</td>
                        <td className="px-4 py-2.5 text-neutral-800">{r.model || '-'}{r.stream && <span className="ml-1 text-[10px] text-indigo-500">流</span>}</td>
                        <td className="px-4 py-2.5 text-right tabular-nums text-neutral-800">{fmt(r.input_tokens)}</td>
                        <td className="px-4 py-2.5 text-right tabular-nums text-neutral-800">{fmt(r.output_tokens)}</td>
                        <td className="px-4 py-2.5 text-right tabular-nums text-indigo-600">{fmt(r.cache_read_tokens)}</td>
                        <td className="px-4 py-2.5 text-right tabular-nums text-indigo-600">{fmt(r.cache_creation_tokens)}</td>
                        <td className="px-4 py-2.5 text-right tabular-nums font-medium text-amber-600">{fmtCost(rowCost(r))}</td>
                        <td className="px-4 py-2.5 text-right text-neutral-500">{r.duration_ms}ms</td>
                        <td className={cn('px-4 py-2.5 text-right font-medium', ok ? 'text-emerald-600' : 'text-red-500')}>{r.status_code}</td>
                      </tr>
                      {open && (
                        <tr className="border-b border-neutral-100 bg-neutral-50/60">
                          <td colSpan={11} className="px-4 pb-4 pt-1">
                            {r.error && (
                              <div className="mb-3">
                                <p className="mb-1 text-[10px] uppercase tracking-wider text-neutral-400">错误正文</p>
                                <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-red-50 px-3 py-2 text-[11px] text-red-600">{pretty(r.error)}</pre>
                              </div>
                            )}
                            <div className="mb-3 grid grid-cols-2 gap-x-6 gap-y-2 md:grid-cols-3">
                              {[
                                ['请求ID', r.request_id], ['客户端IP', r.client_ip], ['出口代理', r.proxy || '直连'],
                                ['User-Agent', r.user_agent], ['会话ID', r.session_id], ['user_id', r.user_id], ['路径', r.path],
                              ].map(([k, v]) => (
                                <div key={k}>
                                  <span className="text-[10px] uppercase tracking-wider text-neutral-400">{k}</span>
                                  <p className="break-all font-mono text-[11px] text-neutral-700">{v || '-'}</p>
                                </div>
                              ))}
                            </div>
                            <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
                              <div>
                                <p className="mb-1 text-[10px] uppercase tracking-wider text-neutral-400">上游响应头(含限流/cf-ray/request-id)</p>
                                <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-neutral-200 bg-white px-3 py-2 text-[11px] text-neutral-700">{pretty(r.resp_headers) || '-'}</pre>
                              </div>
                              <div>
                                <p className="mb-1 text-[10px] uppercase tracking-wider text-neutral-400">请求头(已脱敏)</p>
                                <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-neutral-200 bg-white px-3 py-2 text-[11px] text-neutral-700">{pretty(r.req_headers) || '-'}</pre>
                              </div>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </div>
          {totalPages > 1 && (
            <div className="flex items-center justify-between border-t border-neutral-200 px-4 py-3">
              <p className="text-xs text-neutral-500">共 {fmt(total)} 条，第 {page} / {totalPages} 页</p>
              <div className="flex gap-2">
                <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setPage(1)}>首页</Button>
                <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setPage(page - 1)}>上一页</Button>
                <Button variant="outline" size="sm" disabled={page >= totalPages} onClick={() => setPage(page + 1)}>下一页</Button>
                <Button variant="outline" size="sm" disabled={page >= totalPages} onClick={() => setPage(totalPages)}>末页</Button>
              </div>
            </div>
          )}
        </div>
      </div>
    </BlurFade>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-500">{label}</label>
      {children}
    </div>
  );
}
