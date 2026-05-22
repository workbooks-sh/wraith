import { authenticate } from './auth';

export const getCurrentUser = (token: string) => {
  if (!token) {
    authenticate('guest');
    return null;
  }
  return { id: 1, name: 'User' };
};
