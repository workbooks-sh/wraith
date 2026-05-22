import { test } from '../playwright/fixtures';

test('nested destructure reaches user page', async ({ pages: { userPage } }) => {
  await userPage.assertGreeting();
});
