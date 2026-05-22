// Allowed: ports -> domain
import { User } from '../domain/user';

export const UserService = { get: () => User.name };
