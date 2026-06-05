import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '@/auth';
import { Input } from '@/components/ui/input';
import { DotPattern } from '@/components/magic/dot-pattern';
import { BorderBeam } from '@/components/magic/border-beam';
import { ShimmerButton } from '@/components/magic/shimmer-button';
import { AuroraText } from '@/components/magic/aurora-text';

export default function Login() {
  const { login } = useAuth();
  const navigate = useNavigate();
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!password.trim()) {
      setError('请输入密码');
      return;
    }
    setError('');
    setLoading(true);
    try {
      await login(password.trim());
      navigate('/', { replace: true });
    } catch {
      setError('密码错误');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="relative flex min-h-screen items-center justify-center overflow-hidden px-4">
      <DotPattern className="[mask-image:radial-gradient(45%_45%_at_50%_45%,#000,transparent)]" />
      <div className="pointer-events-none absolute left-1/2 top-1/3 h-96 w-96 -translate-x-1/2 rounded-full bg-primary/20 blur-[120px]" />

      <div className="relative z-10 w-full max-w-sm">
        <div className="relative overflow-hidden rounded-3xl border border-border bg-card/70 p-8 shadow-2xl backdrop-blur-xl">
          <BorderBeam size={140} duration={10} />
          <div className="mb-7 flex flex-col items-center">
            <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-indigo-500 to-fuchsia-500 shadow-lg shadow-primary/30 ring-1 ring-white/10">
              <img src="/favicon.svg" alt="Logo" className="h-9 w-9" />
            </div>
            <h1 className="text-2xl font-semibold tracking-tight">
              <AuroraText>Claude Code Gateway</AuroraText>
            </h1>
            <p className="mt-1.5 text-sm text-muted-foreground">管理控制台</p>
          </div>

          <form onSubmit={submit} className="space-y-4">
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="管理员密码"
              className="h-11 rounded-xl"
              autoFocus
            />
            {error && <p className="text-center text-sm text-red-400">{error}</p>}
            <ShimmerButton type="submit" disabled={loading} className="h-11 w-full font-medium">
              {loading ? '登录中…' : '登录'}
            </ShimmerButton>
          </form>

          <p className="mt-6 text-center text-xs text-muted-foreground">多账号 · 负载均衡 · 指纹对齐</p>
        </div>
      </div>
    </div>
  );
}
