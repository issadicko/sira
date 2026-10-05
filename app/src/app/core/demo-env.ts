import type { Api } from './api';
import { DemoCollection, collectionAt } from './demo-tree';
import { sameVars } from './disk-sync';
import { EnvVar } from './model';
import { envFileName, envNameProblem } from './tree-ops';

type DemoEnvironments = Pick<
  Api,
  'readEnvironment' | 'saveEnvironment' | 'createEnvironment' | 'renameEnvironment' | 'cloneEnvironment' | 'deleteEnvironment' | 'setDefaultEnvironment'
>;

const byName = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase());

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
      if (current && sameVars(current, next)) return false;
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
      add(c, next, vars);
      if (c.info.defaultEnvironment === from) c.info.defaultEnvironment = next;
      return next;
    },

    cloneEnvironment: async (root, from, name) => {
      const c = at(root);
      return add(c, wanted(name, c), structuredClone(known(c, from)));
    },

    deleteEnvironment: async (root, name) => {
      const c = at(root);
      known(c, name);
      delete c.environments[name];
      c.info.environments = c.info.environments.filter((e) => e !== name);
      if (c.info.defaultEnvironment === name) c.info.defaultEnvironment = null;
    },

    setDefaultEnvironment: async (root, name) => {
      at(root).info.defaultEnvironment = name;
    },
  };
}
