// Next.js App Router page. Local Props type used by both `Page` and
// `generateMetadata`. Without the route-convention skip this would generate
// 2 leaks per page file.
type Props = {
  params: Promise<{ slug: string }>;
};

export async function generateMetadata(props: Props): Promise<{ title: string }> {
  const { slug } = await props.params;
  return { title: slug };
}

export default async function Page(props: Props): Promise<string> {
  const { slug } = await props.params;
  return slug;
}
