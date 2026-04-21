import React from 'react';
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { ConfigProvider, theme } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import enUS from 'antd/locale/en_US';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import IDDELayout from './layouts/IDDELayout';
import LoginPage from './pages/LoginPage';
import ProtectedRoute from './components/ProtectedRoute';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, staleTime: 30_000 },
  },
});

const App: React.FC = () => {
  const { i18n } = useTranslation();
  const antdLocale = i18n.language === 'en' ? enUS : zhCN;

  return (
    <QueryClientProvider client={queryClient}>
      <ConfigProvider locale={antdLocale} theme={{ algorithm: theme.darkAlgorithm }}>
        <BrowserRouter>
          <Routes>
            <Route path="/login" element={<LoginPage />} />
            <Route
              path="/*"
              element={
                <ProtectedRoute>
                  <IDDELayout />
                </ProtectedRoute>
              }
            />
          </Routes>
        </BrowserRouter>
      </ConfigProvider>
    </QueryClientProvider>
  );
};

export default App;
