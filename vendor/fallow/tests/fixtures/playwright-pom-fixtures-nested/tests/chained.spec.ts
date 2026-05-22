import { test } from '../playwright/fixtures';

test('chained access reaches admin page', async ({ pages }) => {
  await pages.adminPage.assertGreeting();
});
