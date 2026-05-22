<script setup lang="ts">
import { formatGreeting } from '@shared/utils';

const message = formatGreeting('Nuxt srcDir');
</script>

<template>
  <div>{{ message }}</div>
</template>
