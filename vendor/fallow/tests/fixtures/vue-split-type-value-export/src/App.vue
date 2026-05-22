<script setup lang="ts">
import { Status } from './status';

const current = 'open' as Status;

function isStatus(value: string): value is Status {
  return Object.values(Status).includes(value as Status);
}

console.log(isStatus(current));
</script>

<template>
  <div>{{ current }}</div>
</template>
