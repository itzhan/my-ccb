import type { Account } from '@/api';

/** 账号是否正在被限流 */
export function isRateLimited(a: Account): boolean {
  return !!(a.rate_limit_reset_at && new Date(a.rate_limit_reset_at) > new Date());
}

/** 状态徽章样式(暗色)+ 文案 */
export function statusStyle(a: Account): { className: string; label: string; dot: string } {
  if (a.status === 'active' && isRateLimited(a)) {
    return { className: 'border-amber-500/30 bg-amber-500/10 text-amber-300', label: '限流中', dot: 'bg-amber-400' };
  }
  if (a.status === 'active') {
    return { className: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300', label: '活跃', dot: 'bg-emerald-400' };
  }
  if (a.status === 'error') {
    return { className: 'border-red-500/30 bg-red-500/10 text-red-300', label: '异常', dot: 'bg-red-400' };
  }
  return { className: 'border-zinc-600/40 bg-zinc-500/10 text-zinc-400', label: '停用', dot: 'bg-zinc-500' };
}

/** 用量进度条颜色 */
export function usageBarColor(pct: number): string {
  if (pct >= 80) return 'bg-red-500';
  if (pct >= 50) return 'bg-amber-500';
  return 'bg-emerald-500';
}

/** 剩余时间格式化 */
export function formatTimeLeft(resetsAt: string): string {
  const diff = new Date(resetsAt).getTime() - Date.now();
  if (diff <= 0) return '已重置';
  const days = Math.floor(diff / 86400000);
  const hours = Math.floor((diff % 86400000) / 3600000);
  const minutes = Math.floor((diff % 3600000) / 60000);
  if (days > 0) return `${days}d${hours}h${minutes}m`;
  if (hours > 0) return `${hours}h${minutes}m`;
  return `${minutes}m`;
}

/** OAuth 过期时间(毫秒时间戳)格式化 */
export function formatExpiresAt(expiresAt?: number | null): string {
  if (!expiresAt) return '未提供';
  return new Date(expiresAt).toLocaleString('zh-CN');
}

/** 活着优先排序:active&未限流(0) > active&限流(1) > error(2) > disabled/其它(3)。
 *  稳定排序,保持后端原顺序(后端已按状态分级+优先级)。 */
export function sortAccounts(list: Account[]): Account[] {
  const rank = (a: Account): number => {
    if (a.status === 'active') return isRateLimited(a) ? 1 : 0;
    if (a.status === 'error') return 2;
    return 3;
  };
  return [...list].sort((x, y) => rank(x) - rank(y));
}

export function formatNum(n: number): string {
  return n.toLocaleString();
}
