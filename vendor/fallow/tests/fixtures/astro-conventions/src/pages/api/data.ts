export function GET() {
  return new Response("ok");
}

export function POST() {
  return new Response("ok");
}

export const prerender = true;
export const unusedEndpointHelper = () => "dead";
