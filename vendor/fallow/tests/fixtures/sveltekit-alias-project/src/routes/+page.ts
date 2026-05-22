import { loadGreeting } from '$utils/greeting';

export const load = async () => {
  return { greeting: loadGreeting('SvelteKit') };
};
