export interface ICellRendererAngularComp {
  agInit(): void;
  refresh(): boolean;
}

export interface Authorizable {
  authorize(): boolean;
}

export abstract class BaseCommand {
  abstract execute(): Promise<void>;
}
