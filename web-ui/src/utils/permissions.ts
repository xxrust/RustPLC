export type UserRole = 'operator' | 'engineer' | 'auditor' | 'admin';

export const canEditTopology = (role: UserRole): boolean =>
  ['engineer', 'admin'].includes(role);

export const canInjectFaults = (role: UserRole): boolean =>
  ['engineer', 'admin'].includes(role);

export const canApproveTopology = (role: UserRole): boolean =>
  ['auditor', 'admin'].includes(role);

export const canManageUsers = (role: UserRole): boolean =>
  role === 'admin';

export const hasRole = (userRole: UserRole, requiredRole: UserRole): boolean => {
  const hierarchy = ['operator', 'engineer', 'auditor', 'admin'];
  return hierarchy.indexOf(userRole) >= hierarchy.indexOf(requiredRole);
};
