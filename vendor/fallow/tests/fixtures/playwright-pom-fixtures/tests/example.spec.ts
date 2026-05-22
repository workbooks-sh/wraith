import { test } from '../playwright/fixtures';

test('admin and user', async ({ adminPage, userPage }) => {
  await adminPage.assertGreeting();
  await userPage.assertGreeting();
});
