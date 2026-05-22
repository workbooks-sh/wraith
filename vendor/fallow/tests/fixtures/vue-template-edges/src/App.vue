<script setup lang="ts">
import { vTooltip } from './directives';
import {
  activeAttribute,
  attributeSources,
  fallbackItem,
  message,
  placement,
  unusedImported,
} from './utils';
</script>

<template>
  <List v-slot="{ item = fallbackItem }">{{ item }}</List>
  <div v-tooltip:[placement]="message" />
  <button v-bind:[attributeSources[activeAttribute]]="message">Apply</button>
  <span>{{ unusedImported }}</span>
</template>
