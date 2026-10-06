import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { reconciled, sameVars, varsKey } from './disk-sync';
import { blankVar, varProblems } from './env-ops';
import { EnvVar } from './model';
import { Workspace } from './store';
import { cloneName, envFileName, envNameProblem } from './tree-ops';

/** Saisie d'un nom d'environnement : un nouveau, ou celui d'un renommage ou d'une copie. */
export type EnvNaming = { mode: 'create' } | { mode: 'rename'; env: string } | { mode: 'clone'; env: string };

/**
 * Édition de l'environnement actif : un brouillon de ses variables, comparé à ce que Rust a lu du fichier (`base`), créé, renommé, dupliqué et
 * supprimé. Le brouillon n'agit sur aucune requête tant qu'il n'est pas enregistré : l'envoi et les variables lisent le disque.
 */
@Injectable({ providedIn: 'root' })
export class EnvStore {
  private readonly ws = inject(Workspace);

  /** Environnement dont le brouillon est ouvert ; suit l'environnement actif. */
  readonly owner = signal<string | null>(null);
  readonly draft = signal<EnvVar[]>([]);
  private readonly base = signal<EnvVar[]>([]);
  private root: string | null = null;
  /** Le fichier a changé sur le disque alors que le brouillon a des modifications : l'enregistrer les écraserait. */
  readonly stale = signal(false);
  readonly saving = signal(false);
  /** Secrets dont le trousseau garde une valeur (leurs noms seulement) et pourquoi il ne répond pas, le cas échéant. */
  readonly stored = signal<string[]>([]);
  readonly keychainError = signal<string | null>(null);
  readonly naming = signal<EnvNaming | null>(null);
  readonly namingBusy = signal(false);
  readonly namingError = signal<string | null>(null);
  /** Menu ouvert en `x`, `y` sur l'environnement `env`. */
  readonly menu = signal<{ x: number; y: number; env: string } | null>(null);

  readonly dirty = computed(() => !!this.owner() && !sameVars(this.draft(), this.base()));
  /** Le fichier n'existe plus : le brouillon est gardé, l'enregistrer recrée le fichier. */
  readonly missing = computed(() => {
    const c = this.ws.collection();
    const owner = this.owner();
    return !!c && !!owner && !c.environments.includes(owner);
  });
  readonly problems = computed(() =>
    varProblems(
      this.draft(),
      this.base().map((v) => v.name),
    ),
  );
  readonly valid = computed(() => this.problems().every((problem) => !problem));

  constructor() {
    effect(() => {
      const disk = this.ws.envVars();
      untracked(() => this.reconcile(this.ws.env(), disk));
    });
    effect(() => {
      const dirty = this.dirty();
      untracked(() => this.ws.envDirty.set(dirty));
    });
  }

  private reset(env: string | null, vars: EnvVar[]) {
    this.owner.set(env);
    this.base.set(vars);
    this.draft.set(structuredClone(vars));
    this.stale.set(false);
    void this.loadStored();
  }

  /** Relit quels secrets ont une valeur dans le trousseau ; sans trousseau, aucun, avec la raison. */
  async loadStored() {
    const root = this.ws.collection()?.root;
    const env = this.owner();
    if (!root || !env) {
      this.stored.set([]);
      this.keychainError.set(null);
      return;
    }
    try {
      const names = await api.secretNames(root, env);
      if (this.owner() !== env || this.ws.collection()?.root !== root) return;
      this.stored.set(names);
      this.keychainError.set(null);
    } catch (e) {
      if (this.owner() !== env) return;
      this.stored.set([]);
      this.keychainError.set(String(e));
    }
  }

  /** Accorde le brouillon à ce que le disque contient : adopté s'il n'y a rien à perdre, périmé sinon. */
  private reconcile(env: string | null, disk: EnvVar[]) {
    const root = this.ws.collection()?.root ?? null;
    if (!env || this.owner() !== env || this.root !== root) {
      this.root = root;
      return this.reset(env, disk);
    }
    const next = reconciled({ base: this.base(), draft: this.draft(), stale: this.stale() }, disk);
    this.base.set(next.base);
    this.draft.set(next.draft);
    this.stale.set(next.stale);
  }

  edit(vars: EnvVar[]) {
    this.draft.set(vars);
  }

  add(name: string): number {
    this.draft.update((vars) => [...vars, blankVar(name)]);
    return this.draft().length - 1;
  }

  /** Remplace le brouillon par le fichier du disque, après confirmation s'il y a des modifications à perdre. */
  async reloadFromDisk() {
    const c = this.ws.collection();
    const env = this.owner();
    if (!c || !env) return;
    const lost = `Tes modifications de « ${env} » seront remplacées par le fichier tel qu'il est sur le disque.`;
    if (this.dirty() && !(await this.ws.confirmDiscard(lost, 'Recharger depuis le disque', 'Recharger depuis le disque'))) return;
    try {
      const vars = await api.readEnvironment(c.root, env);
      this.reset(env, vars);
    } catch (e) {
      this.ws.notify(String(e), true);
    }
  }

  /** Abandonne les modifications du brouillon. */
  async discard() {
    if (this.stale() || this.missing()) return this.reloadFromDisk();
    if (!this.dirty()) return;
    const lost = `Tes modifications de « ${this.owner()} » seront perdues.`;
    if (await this.ws.confirmDiscard(lost, 'Abandonner les modifications', 'Abandonner les modifications')) this.reset(this.owner(), this.base());
  }

  /** Le fichier tel qu'il est sur le disque ; `null` avec la raison à l'utilisateur quand il ne se relit pas (introuvable, YAML invalide) : on n'enregistre pas à l'aveugle. */
  private async readDisk(root: string, env: string): Promise<EnvVar[] | null> {
    try {
      return await api.readEnvironment(root, env);
    } catch (e) {
      this.ws.notify(`Impossible de relire ${env}.yml avant d'enregistrer : ${e}`, true);
      return null;
    }
  }

  async save() {
    const c = this.ws.collection();
    const env = this.owner();
    if (!c || !env || this.saving()) return;
    if (!this.dirty()) {
      this.ws.notify(`Rien à enregistrer dans ${env}.yml`);
      return;
    }
    if (!this.valid()) {
      this.ws.notify('Corrige les variables signalées avant d’enregistrer.', true);
      return;
    }
    const recreated = this.missing();
    if (!recreated) {
      const seen = await this.readDisk(c.root, env);
      if (!seen) return;
      const outside = this.stale() || (!sameVars(seen, this.base()) && !sameVars(seen, this.draft()));
      const overwrite = `« ${env} » a changé sur le disque depuis que tu l'as ouvert. L'enregistrer remplace ces changements par ton brouillon.`;
      if (outside && !(await this.ws.confirmDiscard(overwrite, 'Écraser le fichier', 'Fichier modifié sur le disque'))) return;
      if (outside) {
        const again = await this.readDisk(c.root, env);
        if (!again) return;
        if (!sameVars(again, seen)) {
          this.ws.notify(`« ${env} » a encore changé pendant la confirmation : vérifie le fichier puis réessaie.`, true);
          return;
        }
      }
    }
    this.saving.set(true);
    try {
      await this.ws.writing(async () => {
        const vars = structuredClone(this.draft());
        await api.saveEnvironment(c.root, env, vars, recreated);
        const written = await api.readEnvironment(c.root, env).catch(() => vars);
        if (this.ws.collection()?.root !== c.root || this.owner() !== env) return;
        const unchanged = varsKey(this.draft()) === varsKey(vars);
        this.base.set(written);
        if (unchanged) this.draft.set(structuredClone(written));
        this.stale.set(false);
        if (recreated) await this.ws.reload();
        await this.ws.loadEnv();
        await this.loadStored();
      });
      this.ws.notify(recreated ? `${env}.yml recréé sur le disque` : `Enregistré dans ${env}.yml`);
    } catch (e) {
      this.ws.notify(`Échec de l'enregistrement : ${e}`, true);
    } finally {
      this.saving.set(false);
    }
  }

  openMenu(x: number, y: number, env: string) {
    this.menu.set({ x, y, env });
  }

  closeMenu() {
    this.menu.set(null);
  }

  beginNaming(naming: EnvNaming) {
    if (this.ws.busy()) return;
    this.namingError.set(null);
    this.naming.set(naming);
    this.ws.dialog.set('env');
  }

  /** Nom proposé à l'ouverture de la boîte de saisie. */
  suggestion(naming: EnvNaming): string {
    return naming.mode === 'create' ? '' : naming.mode === 'clone' ? cloneName(naming.env) : naming.env;
  }

  /** Nom que prendrait le fichier pour `raw` ; diffère de `raw` quand il est pris ou contient des caractères interdits. */
  resulting(raw: string): string {
    const naming = this.naming();
    const except = naming?.mode === 'rename' ? naming.env : undefined;
    return envFileName(raw, this.ws.collection()?.environments ?? [], except);
  }

  problem(raw: string): string | null {
    return envNameProblem(raw);
  }

  cancelNaming() {
    if (this.namingBusy()) return;
    this.naming.set(null);
    this.ws.dialog.set(null);
  }

  /** Après une erreur de renommage : le fichier a-t-il quand même changé de nom (clé `name` ou défaut non mis à jour) ? Renvoie son nouveau nom, sinon `null`. */
  private async renamedDespite(root: string, before: string[], from: string): Promise<string | null> {
    await this.ws.reload();
    const after = this.ws.collection()?.environments ?? [];
    const added = after.filter((env) => !before.includes(env));
    return this.ws.collection()?.root === root && !after.includes(from) && added.length === 1 ? added[0] : null;
  }

  async confirmNaming(raw: string) {
    const c = this.ws.collection();
    const naming = this.naming();
    if (!c || !naming || this.namingBusy()) return;
    const problem = envNameProblem(raw);
    if (problem) {
      this.namingError.set(problem);
      return;
    }
    this.namingBusy.set(true);
    this.namingError.set(null);
    try {
      const name = raw.trim();
      let warning: string | null = null;
      const result = await this.ws.writing(async () => {
        const created =
          naming.mode === 'create'
            ? await api.createEnvironment(c.root, name)
            : naming.mode === 'clone'
              ? await api.cloneEnvironment(c.root, naming.env, name)
              : await api.renameEnvironment(c.root, naming.env, name).catch(async (e) => {
                  const done = await this.renamedDespite(c.root, c.environments, naming.env);
                  if (!done) throw e;
                  warning = String(e);
                  return done;
                });
        await this.ws.reload();
        if (this.ws.collection()?.root !== c.root) return created;
        if (naming.mode === 'rename' && this.ws.env() === naming.env) {
          this.owner.set(created);
          this.ws.env.set(created);
          await this.ws.loadEnv();
        }
        return created;
      });
      this.naming.set(null);
      this.ws.dialog.set(null);
      if (naming.mode !== 'rename') await this.ws.setEnv(result);
      if (warning) {
        this.ws.notify(warning, true);
      } else {
        this.ws.notify(
          naming.mode === 'create' ? `Environnement « ${result} » créé` : naming.mode === 'clone' ? `« ${result} » créé, copie de « ${naming.env} »` : `Renommé en « ${result} »`,
        );
      }
    } catch (e) {
      this.namingError.set(String(e));
      await this.ws.reload();
    } finally {
      this.namingBusy.set(false);
    }
  }

  /** Envoie l'environnement à la corbeille du système, après confirmation. */
  async remove(env: string) {
    const c = this.ws.collection();
    if (!c || this.ws.busy()) return;
    const unsaved = this.owner() === env && this.dirty() ? ' Tes modifications non enregistrées seront perdues.' : '';
    const trash = this.ws.demo
      ? "Mode démo : il disparaît de la collection en mémoire, rien n'est envoyé à la corbeille de ton disque."
      : 'Tu pourras la récupérer de là.';
    const message = `« ${env} » va dans la corbeille du système. ${trash}${unsaved}`;
    if (!(await this.ws.confirmDiscard(message, 'Mettre à la corbeille', "Supprimer l'environnement"))) return;
    try {
      await this.ws.writing(async () => {
        await api.deleteEnvironment(c.root, env);
        await this.ws.reload();
        const fresh = this.ws.collection();
        if (!fresh || fresh.root !== c.root || this.ws.env() !== env) return;
        this.reset(null, []);
        await this.ws.useEnv(fresh.defaultEnvironment && fresh.environments.includes(fresh.defaultEnvironment) ? fresh.defaultEnvironment : (fresh.environments[0] ?? null));
      });
      this.ws.notify(`« ${env} » est dans la corbeille`);
    } catch (e) {
      this.ws.notify(`Échec de la suppression : ${e}`, true);
      await this.ws.reload();
    }
  }

  /** Choisit l'environnement que la collection ouvre par défaut ; `null` pour n'en choisir aucun. */
  async setDefault(env: string | null) {
    const c = this.ws.collection();
    if (!c || this.ws.busy()) return;
    try {
      await this.ws.writing(async () => {
        await api.setDefaultEnvironment(c.root, env);
        await this.ws.reload();
      });
      this.ws.notify(env ? `« ${env} » s'ouvrira par défaut` : 'Plus aucun environnement ouvert par défaut');
    } catch (e) {
      this.ws.notify(`Échec : ${e}`, true);
    }
  }
}
