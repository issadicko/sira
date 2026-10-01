import { ApplicationConfig, inject, provideAppInitializer, provideBrowserGlobalErrorListeners, provideZonelessChangeDetection } from '@angular/core';

import { COMMANDS, Commands } from './core/commands';
import { SettingsStore } from './core/settings-store';
import { Workspace } from './core/store';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZonelessChangeDetection(),
    provideAppInitializer(() => void inject(SettingsStore)),
    {
      provide: COMMANDS,
      useFactory: () => {
        const ws = inject(Workspace);
        return new Commands((error) => ws.notify(String(error), true));
      },
    },
  ],
};
