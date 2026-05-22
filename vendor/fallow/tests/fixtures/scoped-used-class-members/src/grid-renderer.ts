import { ICellRendererAngularComp } from './contracts';

export class PriceCellRenderer implements ICellRendererAngularComp {
  agInit(): void {}

  refresh(): boolean {
    return true;
  }

  unusedHelper(): boolean {
    return false;
  }
}
