import { computed, inject, Injectable } from '@angular/core';
import { InnerService } from './inner.service';

@Injectable({ providedIn: 'root' })
export class OuterService {
  private readonly inner = inject(InnerService);

  readonly forwardedAaa = this.inner.aaa;
  readonly doubledBbb = computed(() => this.inner.bbb * 2);
}
