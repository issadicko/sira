/** Rend le focus à `previous`, ou à la barre d'URL puis au centre de commande s'il a quitté le DOM. */
export function restoreFocus(previous: HTMLElement | null) {
  const target = previous?.isConnected ? previous : document.querySelector<HTMLElement>('.url-input') ?? document.querySelector<HTMLElement>('.command-center');
  target?.focus();
}
