import { defineCollection } from "astro:content";

export const collections = {
  blog: defineCollection({}),
};

export const unusedCollectionHelper = true;
