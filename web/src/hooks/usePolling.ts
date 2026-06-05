import { useEffect, useRef } from 'react';

/**
 * 立即执行一次 callback,并每 intervalMs 轮询执行(组件卸载时清理)。
 * callback 用 useCallback 包裹以保持稳定引用。
 */
export function usePolling(callback: () => void | Promise<void>, intervalMs: number) {
  const cbRef = useRef(callback);
  cbRef.current = callback;

  useEffect(() => {
    let alive = true;
    const tick = () => { if (alive) cbRef.current(); };
    tick();
    const id = setInterval(tick, intervalMs);
    return () => { alive = false; clearInterval(id); };
  }, [intervalMs]);
}
