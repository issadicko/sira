import type { EnvVar } from './model';

/** Noms de variable acceptés pour une variable que l'on écrit : ceux de Bruno. */
const VARIABLE_NAME = /^[\w.-]+$/;

export function blankVar(name = ''): EnvVar {
  return { name, value: '', secret: false, enabled: true, description: null, dataType: null };
}

/**
 * Ce qui empêche d'enregistrer chaque ligne, `null` quand elle convient : un nom, des caractères permis (seulement pour un nom nouveau, un fichier
 * écrit à la main garde les siens), pas deux fois le même nom.
 */
export function varProblems(vars: EnvVar[], known: string[]): (string | null)[] {
  const before = new Set(known);
  const seen = new Set<string>();
  return vars.map((v) => {
    const name = v.name;
    if (!name.trim()) return 'Donne un nom à la variable.';
    if (seen.has(name)) return 'Cette variable est déjà définie plus haut.';
    seen.add(name);
    if (!before.has(name) && !VARIABLE_NAME.test(name)) return 'Lettres, chiffres, « _ », « - » et « . » seulement.';
    return null;
  });
}
