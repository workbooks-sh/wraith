// Negative: NOT a clone — authentication service, structurally different from anything else
import { createHash, randomBytes } from 'crypto';

interface User {
  id: string;
  email: string;
  passwordHash: string;
  salt: string;
  roles: string[];
  lastLogin: number | null;
  failedAttempts: number;
  lockedUntil: number | null;
}

interface AuthToken {
  token: string;
  userId: string;
  expiresAt: number;
  refreshToken: string;
}

interface AuthConfig {
  maxFailedAttempts: number;
  lockoutDuration: number;
  tokenExpiry: number;
  refreshExpiry: number;
  saltRounds: number;
}

export class AuthService {
  private users = new Map<string, User>();
  private tokens = new Map<string, AuthToken>();
  private config: AuthConfig;

  constructor(config?: Partial<AuthConfig>) {
    this.config = {
      maxFailedAttempts: 5,
      lockoutDuration: 15 * 60 * 1000,
      tokenExpiry: 60 * 60 * 1000,
      refreshExpiry: 7 * 24 * 60 * 60 * 1000,
      saltRounds: 10,
      ...config,
    };
  }

  async register(email: string, password: string): Promise<User> {
    if (this.findUserByEmail(email)) {
      throw new Error('Email already registered');
    }

    const salt = randomBytes(16).toString('hex');
    const passwordHash = this.hashPassword(password, salt);

    const user: User = {
      id: randomBytes(16).toString('hex'),
      email,
      passwordHash,
      salt,
      roles: ['user'],
      lastLogin: null,
      failedAttempts: 0,
      lockedUntil: null,
    };

    this.users.set(user.id, user);
    return user;
  }

  async login(email: string, password: string): Promise<AuthToken> {
    const user = this.findUserByEmail(email);
    if (!user) {
      throw new Error('Invalid credentials');
    }

    if (user.lockedUntil && Date.now() < user.lockedUntil) {
      throw new Error('Account locked');
    }

    const hash = this.hashPassword(password, user.salt);
    if (hash !== user.passwordHash) {
      user.failedAttempts++;
      if (user.failedAttempts >= this.config.maxFailedAttempts) {
        user.lockedUntil = Date.now() + this.config.lockoutDuration;
      }
      throw new Error('Invalid credentials');
    }

    user.failedAttempts = 0;
    user.lockedUntil = null;
    user.lastLogin = Date.now();

    const token = this.generateToken(user.id);
    this.tokens.set(token.token, token);
    return token;
  }

  verifyToken(tokenString: string): User | null {
    const token = this.tokens.get(tokenString);
    if (!token || Date.now() > token.expiresAt) {
      if (token) this.tokens.delete(tokenString);
      return null;
    }
    return this.users.get(token.userId) ?? null;
  }

  async refreshToken(refreshTokenString: string): Promise<AuthToken | null> {
    for (const [key, token] of this.tokens) {
      if (token.refreshToken === refreshTokenString) {
        if (Date.now() > token.expiresAt + this.config.refreshExpiry) {
          this.tokens.delete(key);
          return null;
        }
        this.tokens.delete(key);
        const newToken = this.generateToken(token.userId);
        this.tokens.set(newToken.token, newToken);
        return newToken;
      }
    }
    return null;
  }

  logout(tokenString: string): void {
    this.tokens.delete(tokenString);
  }

  private findUserByEmail(email: string): User | undefined {
    for (const user of this.users.values()) {
      if (user.email === email) return user;
    }
    return undefined;
  }

  private hashPassword(password: string, salt: string): string {
    return createHash('sha256').update(password + salt).digest('hex');
  }

  private generateToken(userId: string): AuthToken {
    return {
      token: randomBytes(32).toString('hex'),
      userId,
      expiresAt: Date.now() + this.config.tokenExpiry,
      refreshToken: randomBytes(32).toString('hex'),
    };
  }
}
