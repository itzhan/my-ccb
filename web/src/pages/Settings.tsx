import { useCallback, useEffect, useState } from 'react';
import { api } from '@/api';
import { useToast } from '@/components/Toaster';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { BlurFade } from '@/components/magic/blur-fade';

type Restriction = 'off' | 'ua' | 'cli' | 'strict';

const OPTIONS: { value: Restriction; title: string; desc: string }[] = [
  { value: 'off', title: '关闭', desc: '不限制。任何带有效令牌的客户端都能访问（普通 API 客户端会被伪装成 CC 转发）。' },
  { value: 'ua', title: '仅校验 UA', desc: '只检查 User-Agent 是 claude-code / claude-cli。宽松，可被伪造（SDK/VSCode 也放行）。' },
  { value: 'cli', title: '仅 Claude Code（CLI/VSCode）', desc: '只放行交互式 Claude Code（终端 cli 与 VSCode 插件），挡掉 Agent SDK 程序化调用（sdk-cli/sdk-ts/local-agent）和桌面三方连接器。' },
  { value: 'strict', title: '严格', desc: 'UA + 系统提示相似度 + 必需 header，只放行真实 Claude Code 客户端。' },
];

export default function Settings() {
  const toast = useToast();
  const [value, setValue] = useState<Restriction>('off');
  const [thinkingRepair, setThinkingRepair] = useState(false);
  const [warmupEnabled, setWarmupEnabled] = useState(true);
  const [warmupTiers, setWarmupTiers] = useState<{ h: number; max: number }[]>([{ h: 2, max: 2 }, { h: 12, max: 3 }, { h: 24, max: 5 }]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savingWarmup, setSavingWarmup] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const s = await api.getSettings();
      setValue((s.client_restriction as Restriction) || 'off');
      setThinkingRepair(s.thinking_repair === 'on');
      setWarmupEnabled(s.warmup_enabled !== 'off');
      try {
        const t = JSON.parse(s.warmup_schedule || '[]');
        if (Array.isArray(t) && t.length) setWarmupTiers(t.map((x: { h: number; max: number }) => ({ h: Number(x.h), max: Number(x.max) })));
      } catch { /* keep default */ }
    } catch { /* ignore */ }
    setLoading(false);
  }, []);

  async function saveWarmup(enabled: boolean, tiers: { h: number; max: number }[]) {
    setSavingWarmup(true);
    try {
      const sorted = [...tiers].filter((t) => t.h > 0 && t.max >= 0).sort((a, b) => a.h - b.h);
      await api.updateSettings({ warmup_enabled: enabled ? 'on' : 'off', warmup_schedule: JSON.stringify(sorted) });
      toast('已保存，立即生效', 'success');
    } catch (e) {
      toast((e as Error).message || '保存失败');
    }
    setSavingWarmup(false);
  }

  useEffect(() => { load(); }, [load]);

  async function save() {
    setSaving(true);
    try {
      const s = await api.updateSettings({ client_restriction: value });
      setValue((s.client_restriction as Restriction) || 'off');
      toast('已保存，立即生效', 'success');
    } catch (e) {
      toast((e as Error).message || '保存失败');
    }
    setSaving(false);
  }

  // thinking 整流是布尔开关，点击即时保存。
  async function toggleThinkingRepair() {
    const next = !thinkingRepair;
    setThinkingRepair(next);
    try {
      await api.updateSettings({ thinking_repair: next ? 'on' : 'off' });
      toast('已保存，立即生效', 'success');
    } catch (e) {
      setThinkingRepair(!next);
      toast((e as Error).message || '保存失败');
    }
  }

  return (
    <BlurFade>
      <div className="space-y-6">
        <div>
          <h2 className="text-lg font-semibold text-neutral-900">全局设置</h2>
          <p className="mt-1 text-sm text-neutral-500">这些设置立即生效、持久化保存，无需重启。</p>
        </div>

        <div className="space-y-4 rounded-2xl border border-neutral-200 bg-white p-6 shadow-sm">
          <div>
            <h3 className="text-base font-medium text-neutral-900">限制客户端访问</h3>
            <p className="mt-1 text-sm text-neutral-500">控制只允许哪类客户端调用网关。</p>
          </div>

          <div className="grid gap-3">
            {OPTIONS.map((opt) => {
              const active = value === opt.value;
              return (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => setValue(opt.value)}
                  className={cn(
                    'rounded-xl border p-4 text-left transition-all',
                    active ? 'border-indigo-400 bg-indigo-50/60 ring-1 ring-indigo-300/40' : 'border-neutral-200 bg-neutral-50 hover:border-indigo-300',
                  )}
                >
                  <div className="flex items-center gap-2">
                    <span className={cn('flex h-4 w-4 items-center justify-center rounded-full border-2', active ? 'border-indigo-500' : 'border-neutral-300')}>
                      {active && <span className="h-2 w-2 rounded-full bg-indigo-500" />}
                    </span>
                    <span className="text-sm font-medium text-neutral-900">{opt.title}</span>
                    {opt.value === 'strict' && <span className="rounded border border-emerald-200 bg-emerald-50 px-1.5 py-0.5 text-[10px] text-emerald-600">推荐</span>}
                  </div>
                  <p className="ml-6 mt-1.5 text-xs text-neutral-500">{opt.desc}</p>
                </button>
              );
            })}
          </div>

          <div className="flex items-center justify-between pt-2">
            <p className="text-xs text-neutral-400">⚠️ UA/header 可被伪造，这不是安全边界；真正的访问控制靠令牌。</p>
            <Button onClick={save} disabled={saving || loading}>{saving ? '保存中…' : '保存'}</Button>
          </div>
        </div>

        <div className="space-y-4 rounded-2xl border border-neutral-200 bg-white p-6 shadow-sm">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h3 className="text-base font-medium text-neutral-900">thinking 块 400 自动整流</h3>
              <p className="mt-1 text-sm text-neutral-500">
                开启后，上游因 thinking 块签名/结构非法返回 400（如 Invalid signature in thinking block）时，自动过滤/降级 thinking（必要时连同工具）块后同账号重发，避免请求直接失败。仅在出错时触发，正常请求不受影响。
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={thinkingRepair}
              disabled={loading}
              onClick={toggleThinkingRepair}
              className={cn(
                'relative h-6 w-11 shrink-0 rounded-full transition-colors',
                thinkingRepair ? 'bg-indigo-500' : 'bg-neutral-300',
              )}
            >
              <span className={cn('absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all', thinkingRepair ? 'left-[22px]' : 'left-0.5')} />
            </button>
          </div>
        </div>

        <div className="space-y-4 rounded-2xl border border-neutral-200 bg-white p-6 shadow-sm">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h3 className="text-base font-medium text-neutral-900">新号升温（按账号年龄限并发会话）</h3>
              <p className="mt-1 text-sm text-neutral-500">
                新号信任期内并发会话过高会被秒封（实测）。开启后,程序按账号年龄(自加入起的小时数)自动收紧"最大并发会话数",熬过新号期再放开。最终生效值 = min(账号自身 max_sessions, 当前年龄档位上限)。
              </p>
            </div>
            <button
              type="button" role="switch" aria-checked={warmupEnabled} disabled={loading || savingWarmup}
              onClick={() => { const n = !warmupEnabled; setWarmupEnabled(n); saveWarmup(n, warmupTiers); }}
              className={cn('relative h-6 w-11 shrink-0 rounded-full transition-colors', warmupEnabled ? 'bg-indigo-500' : 'bg-neutral-300')}
            >
              <span className={cn('absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all', warmupEnabled ? 'left-[22px]' : 'left-0.5')} />
            </button>
          </div>

          <div className={cn('space-y-2', !warmupEnabled && 'opacity-50 pointer-events-none')}>
            <div className="grid grid-cols-[1fr_1fr_auto] gap-2 text-xs font-medium text-neutral-500">
              <span>账号年龄 &lt; （小时）</span>
              <span>最大并发会话</span>
              <span />
            </div>
            {warmupTiers.map((t, i) => (
              <div key={i} className="grid grid-cols-[1fr_1fr_auto] items-center gap-2">
                <input type="number" min={0} step={0.5} value={t.h}
                  onChange={(e) => setWarmupTiers((p) => p.map((x, j) => j === i ? { ...x, h: Number(e.target.value) } : x))}
                  className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm" />
                <input type="number" min={0} value={t.max}
                  onChange={(e) => setWarmupTiers((p) => p.map((x, j) => j === i ? { ...x, max: Number(e.target.value) } : x))}
                  className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm" />
                <button type="button" onClick={() => setWarmupTiers((p) => p.filter((_, j) => j !== i))}
                  className="rounded-lg px-2 py-1 text-sm text-red-500 hover:bg-red-50">删除</button>
              </div>
            ))}
            <div className="flex items-center justify-between pt-1">
              <button type="button" onClick={() => setWarmupTiers((p) => [...p, { h: 48, max: 8 }])}
                className="rounded-lg border border-dashed border-neutral-300 px-3 py-1.5 text-sm text-neutral-600 hover:border-indigo-300">+ 加一档</button>
              <Button onClick={() => saveWarmup(warmupEnabled, warmupTiers)} disabled={savingWarmup || loading}>{savingWarmup ? '保存中…' : '保存升温表'}</Button>
            </div>
            <p className="text-[11px] text-neutral-400">
              示例(默认,取自存活号 #9/#17 曲线):0-2h≤2 · 2-12h≤3 · 12-24h≤5 · 超过最后一档(24h)→ 放开不限。每档"上限"填 0 = 该档不限。
            </p>
          </div>
        </div>
      </div>
    </BlurFade>
  );
}
