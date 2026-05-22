export async function loader() {
  return null;
}

export async function clientLoader() {
  return null;
}

export async function clientAction() {
  return null;
}

export function HydrateFallback() {
  return null;
}

export function ErrorBoundary() {
  return null;
}

export function shouldRevalidate() {
  return true;
}

export function Layout({ children }: { children: unknown }) {
  return children;
}

export const handle = { scope: "root" };
export const unusedRootHelper = () => null;

export default function Root() {
  return null;
}
