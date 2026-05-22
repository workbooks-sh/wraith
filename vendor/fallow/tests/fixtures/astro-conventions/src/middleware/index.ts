export function onRequest(_context: unknown, next: () => unknown) {
  return next();
}

export const unusedMiddlewareHelper = () => "dead";
