import { describe, expect, it, vi } from 'vitest';
import { fetchUser } from './services/api';

vi.mock('./services/api');

describe('api', () => {
  it('uses the auto mock', () => {
    expect(fetchUser()).toBe('mock-user');
  });
});
