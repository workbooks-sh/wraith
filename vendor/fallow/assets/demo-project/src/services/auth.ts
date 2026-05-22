import { getCurrentUser } from './user';

export const authenticate = (token: string) => {
  const user = getCurrentUser(token);
  return { authenticated: !!user, user };
};
