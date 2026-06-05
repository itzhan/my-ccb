import { createContext, useCallback, useContext, useState } from 'react';
import { NavLink, Outlet } from 'react-router-dom';
import { LogOut } from 'lucide-react';
import { api, type Dashboard } from '@/api';
import { useAuth } from '@/auth';
import { usePolling } from '@/hooks/usePolling';
import { cn } from '@/lib/utils';
import { NumberTicker } from '@/components/magic/number-ticker';
import { DotPattern } from '@/components/magic/dot-pattern';
import { AuroraText } from '@/components/magic/aurora-text';
import { MagicCard } from '@/components/magic/magic-card';
import { Button } from '@/components/ui/button';

// 让子页面在增删改后能立刻刷新顶部统计
const RefreshCtx = createContext<() => void>(() => {});
export function useDashboardRefresh() {
  return useContext(RefreshCtx);
}

const NAV = [
  { to: '/', label: '账号', end: true },
  { to: '/tokens', label: '令牌', end: false },
  { to: '/usage', label: '调用记录', end: false },
  { to: '/settings', label: '设置', end: false },
];

const STATS = (d: Dashboard) => [
  { label: '总账号', value: d.accounts.total, color: 'text-foreground' },
  { label: '活跃', value: d.accounts.active, color: 'text-emerald-400' },
  { label: '异常', value: d.accounts.error, color: 'text-red-400' },
  { label: '停用', value: d.accounts.disabled, color: 'text-zinc-400' },
  { label: '令牌', value: d.tokens, color: 'text-primary' },
];

export function Layout() {
  const { logout } = useAuth();
  const [dash, setDash] = useState<Dashboard | null>(null);

  const load = useCallback(() => {
    api.getDashboard().then(setDash).catch(() => {});
  }, []);
  usePolling(load, 10000);

  return (
    <div className="relative min-h-screen">
      {/* 背景点阵 + 顶部光晕 */}
      <DotPattern className="[mask-image:radial-gradient(60%_50%_at_50%_0%,#000_30%,transparent_100%)]" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-72 bg-[radial-gradient(60%_120px_at_50%_0,hsl(252_90%_67%/0.18),transparent)]" />

      <div className="relative">
        {/* 顶栏 */}
        <header className="sticky top-0 z-40 border-b border-border/60 bg-background/70 backdrop-blur-xl">
          <div className="mx-auto flex max-w-7xl items-center justify-between px-6 py-3">
            <div className="flex items-center gap-7">
              <div className="flex items-center gap-2">
                <img src="/favicon.svg" alt="" className="h-6 w-6" />
                <h1 className="text-base font-semibold tracking-tight">
                  <AuroraText>Claude Code Gateway</AuroraText>
                </h1>
              </div>
              <nav className="flex items-center gap-1">
                {NAV.map((n) => (
                  <NavLink
                    key={n.to}
                    to={n.to}
                    end={n.end}
                    className={({ isActive }) =>
                      cn(
                        'rounded-lg px-3 py-1.5 text-sm transition-colors',
                        isActive
                          ? 'bg-primary/15 text-primary font-medium'
                          : 'text-muted-foreground hover:bg-accent hover:text-foreground',
                      )
                    }
                  >
                    {n.label}
                  </NavLink>
                ))}
              </nav>
            </div>
            <Button variant="ghost" size="sm" onClick={logout} className="text-muted-foreground">
              <LogOut className="h-4 w-4" /> 退出
            </Button>
          </div>
        </header>

        <main className="mx-auto max-w-7xl space-y-6 px-6 py-6">
          {/* 统计卡 */}
          {dash && (
            <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
              {STATS(dash).map((s) => (
                <MagicCard key={s.label} className="px-4 py-3">
                  <p className="mb-1 text-xs text-muted-foreground">{s.label}</p>
                  <p className={cn('text-2xl font-bold', s.color)}>
                    <NumberTicker value={s.value} />
                  </p>
                </MagicCard>
              ))}
            </div>
          )}

          <RefreshCtx.Provider value={load}>
            <Outlet />
          </RefreshCtx.Provider>
        </main>
      </div>
    </div>
  );
}
