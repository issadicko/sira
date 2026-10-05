import type { EnvVar } from './model';

/** Noms de variable acceptés pour une variable que l'on écrit : ceux de Bruno. */
const VARIABLE_NAME = /^[\w.-]+$/;

export function blankVar(name = ''): EnvVar {
  return { name, value: '', secret: false, enabled: true, description: null, dataType: null };
}

/**
 * Ce qui empêche d'enregistrer chaque ligne, `null` quand elle convient : un nom, des caractères permis (seulement pour un nom nouveau, un fichier
 * écrit à la main garde ses noms), pas deux fois le même nom (sauf les doublons que le fichier contient déjà : les corriger détruirait une donnée).
 */
export function varProblems(vars: EnvVar[], known: string[]): (string | null)[] {
  const before = new Set(known);
  const allowed = new Map<string, number>();
  for (const name of known) allowed.set(name, (allowed.get(name) ?? 0) + 1);
  const seen = new Map<string, number>();
  return vars.map((v) => {
    const name = v.name;
    if (!name.trim()) return 'Donne un nom à la variable.';
    const count = (seen.get(name) ?? 0) + 1;
    seen.set(name, count);
    if (count > Math.max(1, allowed.get(name) ?? 0)) return 'Cette variable est déjà définie plus haut.';
    if (!before.has(name) && !VARIABLE_NAME.test(name)) return 'Lettres, chiffres, « _ », « - » et « . » seulement.';
    return null;
  });
}
