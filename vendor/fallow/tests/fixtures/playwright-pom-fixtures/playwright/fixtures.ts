import { test as base } from '@playwright/test';
import { AdminPage } from '../src/pages/admin-page';
import { UserPage } from '../src/pages/user-page';

type MyFixtures = {
  adminPage: AdminPage;
  userPage: UserPage;
};

export { expect } from '@playwright/test';

export const test = base.extend<MyFixtures>({
  adminPage: async ({}, use) => {
    await use(new AdminPage());
  },
  userPage: async ({}, use) => {
    await use(new UserPage());
  },
});
