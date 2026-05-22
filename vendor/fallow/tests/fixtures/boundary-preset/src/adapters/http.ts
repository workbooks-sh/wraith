// Allowed: adapters -> ports
import { UserService } from '../ports/user-service';

// Violation: adapters -> domain (not allowed, must go through ports)
import { User } from '../domain/user';

export const handler = () => UserService.get() + User.name;
