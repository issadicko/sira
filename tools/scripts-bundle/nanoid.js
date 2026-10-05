import { nanoid } from 'nanoid';

globalThis.__lib = { nanoid: () => nanoid() };
