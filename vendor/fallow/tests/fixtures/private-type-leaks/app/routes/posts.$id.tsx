// Remix v2 route. Local LoaderArgs type shared by `loader` and the default
// route component export.
type LoaderArgs = {
  params: { id: string };
};

export async function loader(args: LoaderArgs): Promise<{ id: string }> {
  return { id: args.params.id };
}

export default function PostRoute(args: LoaderArgs): string {
  return args.params.id;
}
