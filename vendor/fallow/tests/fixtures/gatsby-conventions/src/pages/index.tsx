export async function getServerData() {
  return { props: {} };
}

export function Head() {
  return null;
}

export const query = "query { site { siteMetadata { title } } }";
export const config = { defer: true };
export const unusedPageHelper = () => null;

export default function IndexPage() {
  return null;
}
