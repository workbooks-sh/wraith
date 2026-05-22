import { Injectable } from '@angular/core';

@Injectable({ providedIn: 'root' })
export class InnerService {
  readonly aaa = 1;
  readonly bbb = 2;
  readonly ccc = 3;
}
