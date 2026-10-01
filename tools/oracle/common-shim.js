import { format, applyEdits } from 'jsonc-parser';
import fastJsonFormat from 'fast-json-format';
import { patternHasher } from '@usebruno/common/utils';
import { hasLongLine } from './bruno/packages/bruno-app/src/utils/common/long-lines.js';
export const prettifyJsonString = (jsonDataString) => {
  if (typeof jsonDataString !== 'string') return jsonDataString;
  try {
    const { hashed, restore } = patternHasher(jsonDataString);
    if (hasLongLine(jsonDataString)) return restore(fastJsonFormat(hashed));
    const edits = format(hashed, undefined, { tabSize: 2, insertSpaces: true });
    return restore(applyEdits(hashed, edits));
  } catch (error) {}
  return jsonDataString;
};
