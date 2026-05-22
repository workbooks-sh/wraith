<script setup lang="ts">
import { formatGreeting } from '@shared/utils';

const message = formatGreeting('Nuxt');
</script>

<template>
  <div>{{ message }}</div>
</template>
