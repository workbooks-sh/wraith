<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { env } from '$env/static/public';

  export let data;

  function navigate() {
    goto('/about');
  }
</script>

<h1>Welcome to {env.PUBLIC_SITE_NAME}</h1>
<p>Current path: {page.url.pathname}</p>
<button on:click={navigate}>Go to About</button>
