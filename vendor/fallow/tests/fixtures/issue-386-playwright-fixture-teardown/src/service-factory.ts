import { AuditService, ProcessEventsService } from "./process-events-service";

export class ServiceFactory {
  private _service: ProcessEventsService | undefined;
  private _auditService: AuditService | undefined;

  get processEventsService(): ProcessEventsService {
    return (this._service ??= new ProcessEventsService());
  }

  get auditService(): AuditService {
    return (this._auditService ??= new AuditService());
  }
}
