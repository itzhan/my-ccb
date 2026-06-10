import { createContext, useCallback, useContext, useState, type ComponentType } from 'react';
import { NavLink, Outlet, useLocation } from 'react-router-dom';
import { LogOut, Users, CircleCheck, TriangleAlert, CirclePause, KeyRound } from 'lucide-react';
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
  { to: '/warmup', label: '养号', end: false },
  { to: '/settings', label: '设置', end: false },
];

type Stat = { label: string; value: number; valueClass: string; icon: ComponentType<{ className?: string }>; iconClass: string };
const STATS = (d: Dashboard): Stat[] => [
  { label: '总账号', value: d.accounts.total, valueClass: 'text-neutral-900', icon: Users, iconClass: 'text-neutral-400' },
  { label: '活跃', value: d.accounts.active, valueClass: 'text-emerald-600', icon: CircleCheck, iconClass: 'text-emerald-500' },
  { label: '异常', value: d.accounts.error, valueClass: 'text-red-500', icon: TriangleAlert, iconClass: 'text-red-400' },
  { label: '停用', value: d.accounts.disabled, valueClass: 'text-neutral-400', icon: CirclePause, iconClass: 'text-neutral-300' },
  { label: '令牌', value: d.tokens, valueClass: 'text-indigo-600', icon: KeyRound, iconClass: 'text-indigo-400' },
];

export function Layout() {
  const { logout } = useAuth();
  const [dash, setDash] = useState<Dashboard | null>(null);
  const showOverview = useLocation().pathname === '/'; // 概览只在账号页显示

  const load = useCallback(() => {
    api.getDashboard().then(setDash).catch(() => {});
  }, []);
  usePolling(load, 10000);

  return (
    <div className="relative min-h-screen">
      {/* 极淡点阵 + 顶部一抹彩色光晕 */}
      <DotPattern className="fill-neutral-300/45 [mask-image:radial-gradient(70%_42%_at_50%_0%,#000_8%,transparent_72%)]" />
      <div className="pointer-events-none absolute inset-x-0 top-0 h-56 bg-[radial-gradient(50%_110px_at_50%_0,rgb(129_140_248/0.12),transparent)]" />

      <div className="relative">
        {/* 顶栏 */}
        <header className="sticky top-0 z-40 border-b border-border bg-white/70 backdrop-blur-xl">
          <div className="mx-auto flex max-w-7xl items-center justify-between px-6 py-3.5">
            <div className="flex items-center gap-8">
              <div className="flex items-center gap-2">
                <img src="/favicon.svg" alt="" className="h-6 w-6" />
                <span className="text-[15px] font-semibold tracking-tight text-neutral-900">
                  Claude Code <AuroraText colors={['#4f46e5', '#7c3aed', '#db2777', '#4f46e5']}>Gateway</AuroraText>
                </span>
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
                          ? 'bg-neutral-100 font-medium text-neutral-900'
                          : 'text-neutral-500 hover:bg-neutral-50 hover:text-neutral-900',
                      )
                    }
                  >
                    {n.label}
                  </NavLink>
                ))}
              </nav>
            </div>
            <Button variant="ghost" size="sm" onClick={logout} className="text-neutral-500 hover:text-neutral-900">
              <LogOut className="h-4 w-4" /> 退出
            </Button>
          </div>
        </header>

        <main className="mx-auto max-w-7xl space-y-8 px-6 py-8">
          {/* 概览统计(仅账号页) */}
          {showOverview && dash && (
            <section className="space-y-3">
              <h2 className="text-sm font-medium text-neutral-500">概览</h2>
              <div className="grid grid-cols-2 gap-4 md:grid-cols-5">
                {STATS(dash).map((s) => {
                  const Icon = s.icon;
                  return (
                    <MagicCard
                      key={s.label}
                      gradientColor="hsl(252 90% 60% / 0.06)"
                      className="relative border-neutral-200 p-5 shadow-sm transition-shadow duration-200 hover:shadow-md"
                    >
                      <div className="mb-3 flex items-center justify-between">
                        <span className="text-xs font-medium text-neutral-500">{s.label}</span>
                        <Icon className={cn('h-4 w-4', s.iconClass)} />
                      </div>
                      <p className={cn('text-[2rem] font-semibold leading-none tracking-tight', s.valueClass)}>
                        <NumberTicker value={s.value} />
                      </p>
                    </MagicCard>
                  );
                })}
              </div>
            </section>
          )}

          <RefreshCtx.Provider value={load}>
            <Outlet />
          </RefreshCtx.Provider>
        </main>
      </div>
    </div>
  );
}
