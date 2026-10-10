import type { Api } from './api';
import type { GitFile, GitState } from './model';

type DemoGit = Pick<Api, 'gitStatus' | 'gitInit' | 'gitDiff' | 'gitCommit' | 'gitPull' | 'gitPush'>;

const HEAD = 'info:\n  name: Détail d\'une transaction\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: "{{baseUrl}}/transactions/{{txId}}"\n  headers:\n    - name: Accept\n      value: application/json\n';
const WORK = 'info:\n  name: Détail d\'une transaction\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: "{{baseUrl}}/transactions/:id"\n  headers:\n    - name: Accept\n      value: application/json\n    - name: X-Canal\n      value: "{{canal}}"\n';
const NEW = 'info:\n  name: Rembourser\n  type: http\n  seq: 7\n\nhttp:\n  method: POST\n  url: "{{baseUrl}}/remboursements"\n';

/** Un dépôt simulé pour le mode démo : deux fichiers changés, valider les vide, pousser remet le compteur à zéro. */
export function createDemoGit(): DemoGit {
  let files: GitFile[] = [
    { path: 'transactions/detail.yml', state: 'M', staged: false, untracked: false, from: null },
    { path: 'transactions/rembourser.yml', state: 'A', staged: false, untracked: true, from: null },
  ];
  let ahead = 1;
  const state = (): GitState => ({ repo: true, branch: 'main', upstream: 'origin/main', ahead, behind: 0, unborn: false, files });
  return {
    gitStatus: async () => state(),
    gitInit: async () => state(),
    gitDiff: async (_root, path) => ({
      path,
      head: path.endsWith('rembourser.yml') ? null : HEAD,
      work: path.endsWith('rembourser.yml') ? NEW : WORK,
      unreadable: null,
    }),
    gitCommit: async (_root, _message, _paths, push) => {
      files = [];
      ahead = push ? 0 : ahead + 1;
      return { id: 'a1b2c3d', pushed: push, pushError: null };
    },
    gitPull: async () => 'Already up to date.',
    gitPush: async () => {
      ahead = 0;
      return '';
    },
  };
}
