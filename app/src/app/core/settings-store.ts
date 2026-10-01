import { Injectable, signal } from '@angular/core';

import { FontGroup, FontSetting, SETTINGS_KEY, Settings, cssVariables, parseSettings, resetFont, serializeSettings, withFont } from './settings';

function saved(): string | null {
  try {
    return localStorage.getItem(SETTINGS_KEY);
  } catch {
    return null;
  }
}

/** Réglages de l'application : appliqués à `:root` en variables CSS dès leur lecture, enregistrés dans `localStorage` à chaque changement. */
@Injectable({ providedIn: 'root' })
export class SettingsStore {
  readonly settings = signal<Settings>(parseSettings(saved()));

  constructor() {
    this.apply();
  }

  change(group: FontGroup, change: Partial<FontSetting>) {
    this.commit(withFont(this.settings(), group, change));
  }

  reset(group: FontGroup) {
    this.commit(resetFont(this.settings(), group));
  }

  private commit(settings: Settings) {
    this.settings.set(settings);
    this.apply();
    try {
      localStorage.setItem(SETTINGS_KEY, serializeSettings(settings));
    } catch {
      /* stockage indisponible */
    }
  }

  private apply() {
    const style = document.documentElement.style;
    for (const [name, value] of Object.entries(cssVariables(this.settings()))) style.setProperty(name, value);
  }
}
