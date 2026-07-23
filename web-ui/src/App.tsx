import React, { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { ConfigProvider, theme } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import enUS from 'antd/locale/en_US';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import IDDELayout from './layouts/IDDELayout';
import LoginPage from './pages/LoginPage';
import ProtectedRoute from './components/ProtectedRoute';
import { authApi } from './services/api';
import { useAppStore } from './stores/appStore';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, staleTime: 30_000 },
  },
});

const App: React.FC = () => {
  const { i18n } = useTranslation();
  const antdLocale = i18n.language === 'en' ? enUS : zhCN;
  const setCurrentUser = useAppStore((state) => state.setCurrentUser);
  const [authReady, setAuthReady] = useState(
    () => !localStorage.getItem('auth_token')
  );

  useEffect(() => {
    const token = localStorage.getItem('auth_token');
    if (!token) return;
    void authApi
      .getCurrentUser()
      .then((response) => setCurrentUser(response.data))
      .catch(() => {
        localStorage.removeItem('auth_token');
        setCurrentUser(null);
      })
      .finally(() => setAuthReady(true));
  }, [setCurrentUser]);

  return (
    <QueryClientProvider client={queryClient}>
      <ConfigProvider locale={antdLocale} theme={{ algorithm: theme.darkAlgorithm }}>
        <BrowserRouter>
          {!authReady ? (
            <div className="app-auth-loading" role="status">Restoring authenticated workspace...</div>
          ) : <Routes>
            <Route path="/login" element={<LoginPage />} />
            <Route
              path="/*"
              element={
                <ProtectedRoute>
                  <IDDELayout />
                </ProtectedRoute>
              }
            />
          </Routes>}
        </BrowserRouter>
      </ConfigProvider>
    </QueryClientProvider>
  );
};

export default App;
