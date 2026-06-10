import { Navigate, Route, Routes } from 'react-router-dom';
import { Loader2 } from 'lucide-react';
import { useAuth } from './auth';
import { Layout } from './components/Layout';
import Login from './pages/Login';
import Accounts from './pages/Accounts';
import Tokens from './pages/Tokens';
import Usage from './pages/Usage';
import Warmup from './pages/Warmup';
import Settings from './pages/Settings';

function Splash() {
  return (
    <div className="flex min-h-screen items-center justify-center">
      <Loader2 className="h-6 w-6 animate-spin text-primary" />
    </div>
  );
}

export default function App() {
  const { authed, ready } = useAuth();
  if (!ready) return <Splash />;

  return (
    <Routes>
      <Route path="/login" element={authed ? <Navigate to="/" replace /> : <Login />} />
      <Route path="/" element={authed ? <Layout /> : <Navigate to="/login" replace />}>
        <Route index element={<Accounts />} />
        <Route path="tokens" element={<Tokens />} />
        <Route path="usage" element={<Usage />} />
        <Route path="warmup" element={<Warmup />} />
        <Route path="settings" element={<Settings />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
