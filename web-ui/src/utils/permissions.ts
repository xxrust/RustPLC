import type { UserRole } from '../types';

export type { UserRole };

export const canEditTopology = (role: UserRole): boolean =>
  ['engineer', 'admin'].includes(role);

export const canInjectFaults = (role: UserRole): boolean =>
  ['engineer', 'admin'].includes(role);

export const canApproveTopology = (role: UserRole): boolean =>
  ['auditor', 'admin'].includes(role);

export const canManageUsers = (role: UserRole): boolean =>
  role === 'admin';

export const hasRole = (userRole: UserRole, requiredRole: UserRole): boolean => {
  if (userRole === 'admin' || userRole === requiredRole) return true;
  const hierarchy: UserRole[] = ['operator', 'engineer', 'auditor'];
  return hierarchy.includes(userRole)
    && hierarchy.includes(requiredRole)
    && hierarchy.indexOf(userRole) >= hierarchy.indexOf(requiredRole);
};
