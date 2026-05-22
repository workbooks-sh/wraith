<script setup lang="ts">
import FancyCard from './FancyCard.vue';
import { vFocusTrap } from './directives';
import { formatDate } from './utils';
import { handlers } from './utils';
import { Counter } from './utils';
import { unusedImported } from './utils';
const date = new Date();
const counter = new Counter();
</script>

<template>
    <FancyCard />
    <input v-focus-trap />
    <button v-on="handlers">Add</button>
    <div>{{ formatDate(date) }}</div>
    <button @click="counter.bump()">{{ counter.value }}</button>
</template>
