import { ApplicationConfig, inject, provideBrowserGlobalErrorListeners, provideZonelessChangeDetection } from '@angular/core';

import { COMMANDS, Commands } from './core/commands';
import { Workspace } from './core/store';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZonelessChangeDetection(),
    {
      provide: COMMANDS,
      useFactory: () => {
        const ws = inject(Workspace);
        return new Commands((error) => ws.notify(String(error), true));
      },
    },
  ],
};
