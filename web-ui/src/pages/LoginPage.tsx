import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { authApi } from '../services/api';
import { useAppStore } from '../stores/appStore';

const LoginPage: React.FC = () => {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const navigate = useNavigate();
  const { setCurrentUser } = useAppStore();

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');

    if (!username || !password) {
      setError('Username and password are required');
      return;
    }

    try {
      setLoading(true);
      const response = await authApi.login(username, password);
      localStorage.setItem('auth_token', response.data.token);
      setCurrentUser(response.data.user);
      navigate('/');
    } catch (err: any) {
      console.error('Login failed:', err);
      setError(err.response?.data?.message || 'Login failed: Invalid credentials');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        minHeight: '100vh',
        background: '#1a1a1a',
      }}
    >
      <div
        style={{
          width: 400,
          background: '#2d2d2d',
          border: '1px solid #3a3a3a',
          borderRadius: 8,
          padding: 32,
          boxShadow: '0 8px 24px rgba(0,0,0,0.5)',
        }}
      >
        <div style={{ textAlign: 'center', marginBottom: 32 }}>
          <h1
            style={{
              margin: 0,
              fontSize: 24,
              fontWeight: 700,
              color: '#00bcd4',
              letterSpacing: '-0.02em',
            }}
          >
            RustPLC Web UI
          </h1>
          <p style={{ margin: '8px 0 0 0', fontSize: 13, color: '#a0a0a0' }}>
            Industrial Control System IDDE
          </p>
        </div>

        <form onSubmit={handleLogin}>
          <div style={{ marginBottom: 16 }}>
            <label
              style={{
                display: 'block',
                fontSize: 11,
                color: '#a0a0a0',
                marginBottom: 6,
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
              }}
            >
              Username
            </label>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="Enter username"
              disabled={loading}
              style={{
                width: '100%',
                padding: '10px 12px',
                background: '#1e1e1e',
                border: '1px solid #3a3a3a',
                borderRadius: 4,
                color: '#e0e0e0',
                fontSize: 13,
                outline: 'none',
              }}
              onFocus={(e) => (e.target.style.borderColor = '#00bcd4')}
              onBlur={(e) => (e.target.style.borderColor = '#3a3a3a')}
            />
          </div>

          <div style={{ marginBottom: 24 }}>
            <label
              style={{
                display: 'block',
                fontSize: 11,
                color: '#a0a0a0',
                marginBottom: 6,
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
              }}
            >
              Password
            </label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter password"
              disabled={loading}
              style={{
                width: '100%',
                padding: '10px 12px',
                background: '#1e1e1e',
                border: '1px solid #3a3a3a',
                borderRadius: 4,
                color: '#e0e0e0',
                fontSize: 13,
                outline: 'none',
              }}
              onFocus={(e) => (e.target.style.borderColor = '#00bcd4')}
              onBlur={(e) => (e.target.style.borderColor = '#3a3a3a')}
            />
          </div>

          {error && (
            <div
              style={{
                marginBottom: 16,
                padding: '10px 12px',
                background: '#f5222d22',
                border: '1px solid #f5222d',
                borderRadius: 4,
                color: '#f5222d',
                fontSize: 12,
              }}
            >
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={loading}
            style={{
              width: '100%',
              padding: '12px',
              background: loading ? '#3a3a3a' : '#00bcd4',
              border: 'none',
              borderRadius: 4,
              color: loading ? '#5a5a5a' : '#1e1e1e',
              fontSize: 14,
              fontWeight: 600,
              cursor: loading ? 'not-allowed' : 'pointer',
              transition: 'all 0.2s',
            }}
          >
            {loading ? 'Logging in...' : 'Login'}
          </button>
        </form>

        <div
          style={{
            marginTop: 24,
            paddingTop: 24,
            borderTop: '1px solid #3a3a3a',
            fontSize: 11,
            color: '#5a5a5a',
            textAlign: 'center',
          }}
        >
          <p style={{ margin: 0 }}>Demo Credentials:</p>
          <p style={{ margin: '4px 0 0 0', fontFamily: 'JetBrains Mono, monospace' }}>
            engineer / password
          </p>
        </div>
      </div>
    </div>
  );
};

export default LoginPage;
