export const shortMethod = (m: string) => ({ DELETE: 'DEL', OPTIONS: 'OPT' })[m.toUpperCase()] ?? m.toUpperCase();
export const methodClass = (m: string) => `m m-${m.toLowerCase()}`;
export const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
export const badgeOf = (requestType: string, method: string) => (requestType === 'graphql' ? 'GQL' : requestType === 'websocket' ? 'WS' : requestType === 'grpc' ? 'gRPC' : method);
