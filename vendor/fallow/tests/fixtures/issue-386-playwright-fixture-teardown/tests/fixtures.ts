import { test as base } from "@playwright/test";
import { ServiceFactory } from "../src/service-factory";

export const test = base.extend({
  app: async ({}, use) => {
    const factory = new ServiceFactory();

    await use({});

    await factory.processEventsService.queryEventsForProcessId();
    Object.keys(factory.auditService);
  },
});
