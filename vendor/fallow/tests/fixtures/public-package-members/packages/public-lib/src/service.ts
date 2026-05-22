export class WorkspaceService {
  usedMethod() {
    return 'used internally';
  }

  externalApiMethod() {
    return 'available to consumers';
  }
}

export enum PublicStatus {
  Active = 'active',
  External = 'external',
}
