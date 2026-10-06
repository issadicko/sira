import assert from 'node:assert/strict';
import { test } from 'node:test';

import { AUTH_TYPES, OAUTH_FLOWS, addOAuthParam, authLabel, defaultAuth, defaultOAuth2, describeToken, formatSpan, oauthFields, patchOAuthParam, removeOAuthParam, withAuthField } from './auth.ts';

test('ef_aut_01 chaque type proposé a une auth neuve du bon type', () => {
  for (const { type } of AUTH_TYPES) assert.equal(defaultAuth(type).type, type);
});

test('ef_aut_01 Digest demande un identifiant et un mot de passe, AWS ses six champs', () => {
  assert.deepEqual(defaultAuth('digest'), { type: 'digest', username: '', password: '' });
  assert.deepEqual(defaultAuth('awsv4'), { type: 'awsv4', accessKeyId: '', secretAccessKey: '', sessionToken: '', service: '', region: '', profileName: '' });
});

test('ef_aut_01 modifier un champ rend une nouvelle auth sans toucher l\'ancienne', () => {
  const before = defaultAuth('awsv4');
  const after = withAuthField(before, 'region', 'eu-west-1');
  assert.equal((after as { region: string }).region, 'eu-west-1');
  assert.equal((before as { region: string }).region, '');
});

test('ef_aut_01 un champ que le type n\'a pas est ignoré', () => {
  const none = defaultAuth('none');
  assert.equal(withAuthField(none, 'token', 'x'), none);
});

test('ef_aut_01 le libellé vient du type, ou du nom d\'un type non édité', () => {
  assert.equal(authLabel(defaultAuth('digest')), 'Digest');
  assert.equal(authLabel({ type: 'other', label: 'ntlm' }), 'ntlm');
});

test('ef_aut_02 OAuth 2 propose quatre flux et une auth neuve complète du bon type', () => {
  assert.equal(OAUTH_FLOWS.length, 4);
  const auth = defaultAuth('oauth2');
  if (auth.type !== 'oauth2') throw new Error('type attendu : oauth2');
  assert.deepEqual(
    { flow: auth.flow, id: auth.tokenId, prefix: auth.tokenPrefix, fetch: auth.autoFetchToken, params: auth.parameters },
    { flow: 'client_credentials', id: 'credentials', prefix: 'Bearer', fetch: true, params: [] },
  );
});

test('ef_aut_02 chaque flux ne montre que les champs qu\'il utilise', () => {
  const cc = oauthFields('client_credentials');
  assert.deepEqual([cc.authorizationUrl, cc.callbackUrl, cc.owner, cc.pkce, cc.accessTokenUrl, cc.clientSecret], [false, false, false, false, true, true]);
  const pw = oauthFields('resource_owner_password_credentials');
  assert.deepEqual([pw.owner, pw.authorizationUrl], [true, false]);
  const code = oauthFields('authorization_code');
  assert.deepEqual([code.authorizationUrl, code.callbackUrl, code.pkce, code.state, code.accessTokenUrl], [true, true, true, true, true]);
  const implicit = oauthFields('implicit');
  assert.deepEqual([implicit.accessTokenUrl, implicit.clientSecret, implicit.pkce, implicit.authorizationUrl], [false, false, false, true]);
});

test('ef_aut_02 les paramètres additionnels s\'ajoutent, se modifient et se retirent sans toucher l\'original', () => {
  const base = defaultOAuth2();
  const added = addOAuthParam(base, 'refresh');
  assert.deepEqual(added.parameters, [{ stage: 'refresh', name: '', value: '', placement: 'body' }]);
  const patched = patchOAuthParam(added, 0, { name: 'audience', placement: 'header' });
  assert.deepEqual([patched.parameters[0].name, patched.parameters[0].placement, patched.parameters[0].stage], ['audience', 'header', 'refresh']);
  assert.equal(added.parameters[0].name, '');
  assert.equal(removeOAuthParam(patched, 0).parameters.length, 0);
  assert.equal(base.parameters.length, 0);
});

test('ef_aut_02 les booléens du flux se modifient comme les textes', () => {
  const auth = withAuthField(defaultOAuth2(), 'pkce', true);
  assert.equal((auth as { pkce: boolean }).pkce, true);
});

test('ef_aut_02 la durée restante est arrondie vers le bas dans l\'unité qui convient', () => {
  assert.deepEqual([45_000, 59_999, 60_000, 12 * 60_000, 3 * 3_600_000 + 20 * 60_000, 3 * 3_600_000, 50 * 3_600_000].map(formatSpan), ['45 s', '59 s', '1 min', '12 min', '3 h 20', '3 h', '2 j']);
  assert.equal(formatSpan(-5), '0 s');
});

test('ef_aut_02 l\'éditeur dit où en est le jeton sans jamais en montrer la valeur', () => {
  const now = 1_000_000;
  const info = { id: 'credentials', tokenType: 'Bearer', scope: 'read', expiresAt: now + 90_000, expired: false, hasRefreshToken: true };
  assert.equal(describeToken(null, now).tone, 'none');
  assert.deepEqual(describeToken(info, now), { tone: 'ok', text: 'Jeton valide, expire dans 1 min · read' });
  assert.equal(describeToken({ ...info, expiresAt: null, scope: null }, now).text, 'Jeton valide, sans durée annoncée par le serveur');
  assert.deepEqual(describeToken({ ...info, expired: true }, now), { tone: 'expired', text: 'Jeton expiré, un jeton de rafraîchissement est gardé.' });
  assert.equal(describeToken({ ...info, expiresAt: now - 1, hasRefreshToken: false }, now).text, 'Jeton expiré.', "l'heure compte même si l'état date de plus tôt");
});
