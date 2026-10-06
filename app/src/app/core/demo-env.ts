import type { Api } from './api';
import { DemoCollection, collectionAt } from './demo-tree';
import { sameVars } from './disk-sync';
import { EnvVar } from './model';
import { envFileName, envNameProblem } from './tree-ops';

type DemoEnvironments = Pick<
  Api,
  'readEnvironment' | 'saveEnvironment' | 'createEnvironment' | 'renameEnvironment' | 'cloneEnvironment' | 'deleteEnvironment' | 'setDefaultEnvironment' | 'secretNames'
>;

const byName = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase());

/** Le « trousseau » de la démo : les valeurs des secrets, en mémoire, que l'interface ne relit jamais. */
export const demoKeychain = new Map<string, string>();
export const secretSlot = (root: string, env: string, name: string) => `${root}|${env}|${name}`;

/** Un secret n'a jamais de valeur dans le fichier. */
const stored = (vars: EnvVar[]): EnvVar[] => structuredClone(vars).map((v) => (v.secret ? { ...v, value: null } : v));

/** Environnements du mode démo : appliqués à la collection gardée en mémoire, avec les règles de nommage du moteur. */
export function createDemoEnvironments(collections: Map<string, DemoCollection>): DemoEnvironments {
  const at = (root: string) => collectionAt(collections, root);

  const known = (c: DemoCollection, name: string): EnvVar[] => {
    const vars = c.environments[name];
    if (!vars) throw `environments/${name}.yml : introuvable`;
    return vars;
  };

  const add = (c: DemoCollection, name: string, vars: EnvVar[]) => {
    c.environments[name] = vars;
    c.info.environments = [...c.info.environments.filter((e) => e !== name), name].sort(byName);
    return name;
  };

  const wanted = (raw: string, c: DemoCollection, except?: string) => {
    const problem = envNameProblem(raw);
    if (problem) throw problem;
    return envFileName(raw, c.info.environments, except);
  };

  return {
    readEnvironment: async (root, name) => structuredClone(known(at(root), name)),

    saveEnvironment: async (root, name, vars, create) => {
      const c = at(root);
      const next = stored(vars);
      const current = c.environments[name];
      if (!current && !create) throw `environments/${name}.yml : introuvable`;
      let kept = false;
      for (const v of vars) {
        if (!v.secret || v.value == null) continue;
        if (v.value === '') demoKeychain.delete(secretSlot(root, name, v.name));
        else demoKeychain.set(secretSlot(root, name, v.name), v.value);
        kept = true;
      }
      for (const old of current?.filter((v) => v.secret) ?? []) {
        if (!vars.some((v) => v.secret && v.name === old.name)) kept = demoKeychain.delete(secretSlot(root, name, old.name)) || kept;
      }
      if (current && sameVars(current, next)) return kept;
      if (current) c.environments[name] = next;
      else add(c, name, next);
      return true;
    },

    createEnvironment: async (root, name) => {
      const c = at(root);
      return add(c, wanted(name, c), []);
    },

    renameEnvironment: async (root, from, name) => {
      const c = at(root);
      const vars = known(c, from);
      const next = wanted(name, c, from);
      if (next === from) return from;
      delete c.environments[from];
      c.info.environments = c.info.environments.filter((e) => e !== from);
      for (const v of vars.filter((v) => v.secret)) {
        const value = demoKeychain.get(secretSlot(root, from, v.name));
        if (value !== undefined) demoKeychain.set(secretSlot(root, next, v.name), value);
        demoKeychain.delete(secretSlot(root, from, v.name));
      }
      add(c, next, vars);
      if (c.info.defaultEnvironment === from) c.info.defaultEnvironment = next;
      return next;
    },

    cloneEnvironment: async (root, from, name) => {
      const c = at(root);
      const copy = add(c, wanted(name, c), structuredClone(known(c, from)));
      for (const v of known(c, from).filter((v) => v.secret)) {
        const value = demoKeychain.get(secretSlot(root, from, v.name));
        if (value !== undefined) demoKeychain.set(secretSlot(root, copy, v.name), value);
      }
      return copy;
    },

    deleteEnvironment: async (root, name) => {
      const c = at(root);
      for (const v of known(c, name).filter((v) => v.secret)) demoKeychain.delete(secretSlot(root, name, v.name));
      delete c.environments[name];
      c.info.environments = c.info.environments.filter((e) => e !== name);
      if (c.info.defaultEnvironment === name) c.info.defaultEnvironment = null;
    },

    setDefaultEnvironment: async (root, name) => {
      at(root).info.defaultEnvironment = name;
    },

    secretNames: async (root, env) =>
      known(at(root), env)
        .filter((v) => v.secret && demoKeychain.has(secretSlot(root, env, v.name)))
        .map((v) => v.name),
  };
}
