import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { Choice, SyncOperation } from '../core/model';
import { dirname } from '../core/paths';
import { Workspace } from '../core/store';
import { choiceLabel, isArbitrated, opState, pairingLabel, versionLabel, versionOf } from '../core/sync';
import { SyncStore } from '../core/sync-store';
import { Icon } from './icon';
import { methodClass, shortMethod } from './method';

/** Barre latérale de la synchro OpenAPI : source, compteurs, groupes d'opérations et rapprochements proposés. */
@Component({
  selector: 'app-sync-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    <div class="pane-head">
      <span class="pane-title">Synchro OpenAPI</span>
      @if (sync.connected() || sync.plan()) {
        <button class="icon-btn sm" (click)="sync.relaunch()" [disabled]="sync.comparing()" title="Relancer la comparaison" aria-label="Relancer la comparaison">
          @if (sync.comparing()) {
            <span class="spinner"></span>
          } @else {
            <app-ic name="sync" [size]="15" />
          }
        </button>
      }
    </div>
    @if (sync.plan(); as plan) {
      <div class="sync-src">
        <span class="mono" [title]="plan.source"><app-ic name="file" [size]="13" /><span class="clip-text">{{ plan.source }}</span></span>
        <span class="muted">{{ versionLabel(plan) }}</span>
      </div>
      @if (ws.demo) {
        <div class="sync-note note"><app-ic name="alert" [size]="14" /><span>Mode démo : plan fictif sur la collection de démo, rien n'est écrit.</span></div>
      }
      @if (!plan.hasBase) {
        <div class="sync-note note"><app-ic name="alert" [size]="14" /><span>Première synchro : sans base, toute différence est un conflit à arbitrer.</span></div>
      }
      @if (sync.groups(); as g) {
        <div class="sync-counts">
          <span><i class="dot" [class.warn]="sync.conflictsLeft()" [class.good]="!sync.conflictsLeft()"></i><b>{{ sync.conflictsLeft() }}</b> {{ sync.conflictsLeft() > 1 ? 'conflits' : 'conflit' }}</span>
          <span><b>{{ g.auto.length }}</b> {{ g.auto.length > 1 ? 'fusions auto' : 'fusion auto' }}</span>
          <span><b>{{ g.created.length }}</b> {{ g.created.length > 1 ? 'nouvelles' : 'nouvelle' }}</span>
          <span><b>{{ g.removed.length }}</b> {{ g.removed.length > 1 ? 'dépréciées' : 'dépréciée' }}</span>
        </div>
        <div class="sb-scroll" style="padding-top: 0">
          @if (g.conflicts.length) {
            <div class="group-head">À arbitrer<span class="n">{{ g.conflicts.length }}</span></div>
            @for (c of g.conflicts; track c.changeId) {
              <button class="op-row" [class.is-active]="sync.selection()?.changeId === c.changeId" (click)="sync.select({ key: c.key, changeId: c.changeId })">
                <span [class]="methodClass(c.op.method)">{{ shortMethod(c.op.method) }}</span>
                <span class="op-path" [title]="c.op.path">{{ c.op.path }}</span>
                @if (arbitrated(c.changeId)) {
                  <span class="op-state good" [attr.aria-label]="'arbitré'"><app-ic name="check" [size]="12" /></span>
                } @else {
                  <span class="op-state warn">à arbitrer</span>
                }
                <span class="op-field">{{ c.change.label }}@if (choiceOf(c.changeId); as choice) { · {{ label(choice) }} }</span>
              </button>
            }
          }
          @if (g.auto.length) {
            <div class="group-head" [class.gap]="g.conflicts.length">Fusion automatique<span class="n">{{ g.auto.length }}</span></div>
            @for (o of g.auto; track o.key) {
              <button class="op-row" [class.is-active]="sync.selection()?.key === o.key" (click)="sync.select({ key: o.key, changeId: null })">
                <span [class]="methodClass(o.method)">{{ shortMethod(o.method) }}</span>
                <span class="op-path" [title]="o.path">{{ o.path }}</span>
                <span class="op-state" [class.good]="state(o).tone === 'good'">{{ state(o).text }}</span>
              </button>
            }
          }
          @if (g.created.length) {
            <div class="group-head gap">Nouvelles<span class="n">{{ g.created.length }}</span></div>
            @for (o of g.created; track o.key) {
              <label class="op-row">
                <span [class]="methodClass(o.method)">{{ shortMethod(o.method) }}</span>
                <span class="op-path" [title]="o.path">{{ o.path }}</span>
                <input type="checkbox" class="cb" [checked]="!skipped(o.key)" (change)="sync.toggleSkip(o.key)" [attr.aria-label]="'Créer ' + o.method + ' ' + o.path" />
                <span class="op-field">{{ skipped(o.key) ? 'ignorée : ne sera pas créée' : target(o) }}</span>
              </label>
            }
          }
          @if (g.removed.length) {
            <div class="group-head gap">Dépréciées<span class="n">{{ g.removed.length }}</span></div>
            @for (o of g.removed; track o.key) {
              <div class="op-row is-deprecated" title="Retirée de la spec">
                <span [class]="methodClass(o.method)">{{ shortMethod(o.method) }}</span>
                <span class="op-path" [title]="o.path">{{ o.path }}</span>
                <span class="op-state">{{ state(o).text }}</span>
              </div>
            }
          }
          @if (g.missing.length) {
            <div class="group-head gap">Manquantes<span class="n">{{ g.missing.length }}</span></div>
            @for (o of g.missing; track o.key) {
              <div class="op-row">
                <span [class]="methodClass(o.method)">{{ shortMethod(o.method) }}</span>
                <span class="op-path" [title]="o.path">{{ o.path }}</span>
                <span class="op-state">{{ state(o).text }}</span>
                <span class="op-field mono clip-text" [title]="o.file ?? ''">{{ o.file }}</span>
                <span class="op-actions seg" role="group" [attr.aria-label]="'Que faire de ' + o.method + ' ' + o.path">
                  <button [attr.aria-pressed]="recreated(o.key)" (click)="sync.setRecreate(o.key, true)">Recréer</button>
                  <button [attr.aria-pressed]="!recreated(o.key)" (click)="sync.setRecreate(o.key, false)">Oublier</button>
                </span>
              </div>
            }
          }
          @if (plan.suggestions.length) {
            <div class="group-head gap">Rapprochements proposés<span class="n">{{ plan.suggestions.length }}</span></div>
            @for (s of plan.suggestions; track s.removed + s.added) {
              <div class="pair">
                <div class="pair-keys mono">
                  <span class="clip-text" [title]="s.removed">{{ pairingLabel(plan, s.removed) }}</span>
                  <app-ic name="arrow-right" [size]="12" />
                  <span class="clip-text" [title]="s.added">{{ pairingLabel(plan, s.added) }}</span>
                </div>
                <div class="pair-reason">{{ s.reason }}</div>
                <button class="btn" [disabled]="sync.comparing()" (click)="sync.pair(s.removed, s.added)">Rapprocher</button>
              </div>
            }
          }
          @if (!g.conflicts.length && !g.auto.length && !g.created.length && !g.removed.length && !g.missing.length) {
            <p class="pal-empty">Aucune différence avec la spec.</p>
          }
        </div>
      }
    } @else if (sync.synced(); as done) {
      <div class="sync-src">
        <span class="mono" [title]="done.source"><app-ic name="file" [size]="13" /><span class="clip-text">{{ done.source }}</span></span>
        <span class="muted">{{ versionOf(done.to.version) }} · base à jour</span>
      </div>
    } @else if (sync.comparing()) {
      <p class="pal-empty"><span class="spinner"></span> Comparaison avec la spec…</p>
    } @else if (sync.showConnect() || (sync.status() && !sync.connected())) {
      <p class="pal-empty">Cette collection n'est pas connectée à une spec OpenAPI.</p>
      @if (!sync.connecting()) {
        <div class="sync-actions"><button class="btn" (click)="sync.beginConnect()"><app-ic name="import" [size]="14" />Connecter une spec OpenAPI…</button></div>
      }
    } @else if (sync.error(); as e) {
      <div class="sync-actions"><div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div></div>
    }
  `,
})
export class SyncSidebar {
  protected readonly sync = inject(SyncStore);
  protected readonly ws = inject(Workspace);
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  protected readonly versionLabel = versionLabel;
  protected readonly pairingLabel = pairingLabel;
  protected readonly versionOf = versionOf;

  protected label(choice: Choice) {
    return choiceLabel(choice).toLowerCase();
  }

  protected state(op: SyncOperation) {
    return opState(op.status);
  }

  protected arbitrated(changeId: string) {
    return isArbitrated(this.sync.decisions(), changeId);
  }

  protected choiceOf(changeId: string) {
    const choice = this.sync.decisions().choices[changeId]?.choice;
    return choice ?? null;
  }

  protected skipped(key: string) {
    return this.sync.decisions().skip.includes(key);
  }

  protected recreated(key: string) {
    return this.sync.decisions().recreate.includes(key);
  }

  protected target(op: SyncOperation) {
    const folder = op.file ? dirname(op.file) : '';
    return folder ? `→ ${folder}` : '→ racine';
  }
}
