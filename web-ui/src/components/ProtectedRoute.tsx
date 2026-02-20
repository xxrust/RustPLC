import React from 'react';
import { Navigate } from 'react-router-dom';
import { useAppStore } from '../stores/appStore';
import { hasRole, type UserRole } from '../utils/permissions';

interface ProtectedRouteProps {
  children: React.ReactNode;
  requiredRole?: UserRole;
}

const ProtectedRoute: React.FC<ProtectedRouteProps> = ({ children, requiredRole }) => {
  const { currentUser } = useAppStore();

  if (!currentUser) {
    return <Navigate to="/login" replace />;
  }

  if (requiredRole && !hasRole(currentUser.role, requiredRole)) {
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
            textAlign: 'center',
          }}
        >
          <div
            style={{
              width: 64,
              height: 64,
              margin: '0 auto 16px',
              background: '#f5222d22',
              border: '2px solid #f5222d',
              borderRadius: '50%',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              fontSize: 32,
            }}
          >
            🚫
          </div>
          <h2 style={{ margin: '0 0 8px 0', fontSize: 18, color: '#e0e0e0' }}>
            Access Denied
          </h2>
          <p style={{ margin: 0, fontSize: 13, color: '#a0a0a0', lineHeight: 1.6 }}>
            You do not have permission to access this page.
            <br />
            Required role: <strong>{requiredRole}</strong>
            <br />
            Your role: <strong>{currentUser.role}</strong>
          </p>
          <button
            onClick={() => window.history.back()}
            style={{
              marginTop: 24,
              padding: '8px 16px',
              background: '#3a3a3a',
              border: 'none',
              borderRadius: 4,
              color: '#e0e0e0',
              fontSize: 12,
              cursor: 'pointer',
            }}
          >
            Go Back
          </button>
        </div>
      </div>
    );
  }

  return <>{children}</>;
};

export default ProtectedRoute;
