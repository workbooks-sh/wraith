import { BaseClient } from "./base-client";
import { BaseService } from "./base-service";

export class ConcreteClient extends BaseClient {}

export class ConcreteService extends BaseService<ConcreteClient> {}
