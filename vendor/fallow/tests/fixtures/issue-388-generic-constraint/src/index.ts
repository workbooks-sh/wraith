import { ConcreteClient, ConcreteService } from "./concrete";

(async () => {
  const svc = new ConcreteService(new ConcreteClient());
  await svc.getLatest("123");
})();
