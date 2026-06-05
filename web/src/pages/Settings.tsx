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
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const s = await api.getSettings();
      setValue((s.client_restriction as Restriction) || 'off');
    } catch { /* ignore */ }
    setLoading(false);
  }, []);

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
      </div>
    </BlurFade>
  );
}
