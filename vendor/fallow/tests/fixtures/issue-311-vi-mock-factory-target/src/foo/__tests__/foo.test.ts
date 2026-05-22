import { vi } from 'vitest';

vi.mock('../../bar/foo', () => ({
  useRegenerateSlotTextMutation: () => ({}),
}));
