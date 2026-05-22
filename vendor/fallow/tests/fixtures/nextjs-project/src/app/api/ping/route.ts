export const runtime = 'edge';
export const preferredRegion = 'auto';

export async function GET() {
  return Response.json({ ok: true });
}
