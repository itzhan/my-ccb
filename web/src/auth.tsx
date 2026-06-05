import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { api, setAuth } from './api';

const STORAGE_KEY = 'claude-code-gateway_auth';

interface AuthCtx {
  authed: boolean;
  ready: boolean;
  login: (password: string) => Promise<void>;
  logout: () => void;
}

const Ctx = createContext<AuthCtx>(null!);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [authed, setAuthed] = useState(false);
  const [ready, setReady] = useState(false);

  // 启动时尝试用已保存的密码恢复会话(等价于 Vue router.ts 的 tryRestoreAuth)
  useEffect(() => {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (!saved) { setReady(true); return; }
    setAuth(saved);
    api.getDashboard()
      .then(() => setAuthed(true))
      .catch(() => { localStorage.removeItem(STORAGE_KEY); setAuth(''); })
      .finally(() => setReady(true));
  }, []);

  async function login(password: string) {
    setAuth(password);
    await api.getDashboard(); // 校验密码
    localStorage.setItem(STORAGE_KEY, password);
    setAuthed(true);
  }

  function logout() {
    localStorage.removeItem(STORAGE_KEY);
    setAuth('');
    setAuthed(false);
  }

  return <Ctx.Provider value={{ authed, ready, login, logout }}>{children}</Ctx.Provider>;
}

export function useAuth() {
  return useContext(Ctx);
}
