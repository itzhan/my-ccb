import { Navigate, Route, Routes } from 'react-router-dom';
import { Loader2 } from 'lucide-react';
import { useAuth } from './auth';
import { Layout } from './components/Layout';
import Login from './pages/Login';
import Accounts from './pages/Accounts';
import Placeholder from './pages/Placeholder';

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
        <Route path="tokens" element={<Placeholder title="令牌管理" />} />
        <Route path="usage" element={<Placeholder title="调用记录" />} />
        <Route path="settings" element={<Placeholder title="设置" />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
