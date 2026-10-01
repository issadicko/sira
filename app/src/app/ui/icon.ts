import { ChangeDetectionStrategy, Component, input } from '@angular/core';

@Component({
  selector: 'app-ic',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  template: `<svg class="ic" [attr.width]="size()" [attr.height]="size()" aria-hidden="true"><use [attr.href]="'#i-' + name()" /></svg>`,
})
export class Icon {
  readonly name = input.required<string>();
  readonly size = input(16);
}
