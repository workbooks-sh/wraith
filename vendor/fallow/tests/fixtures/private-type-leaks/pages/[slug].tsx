// Next.js Pages Router page. Local PageProps shared by `getStaticProps` and
// the default Component export.
type PageProps = {
  slug: string;
};

export function getStaticProps(): { props: PageProps } {
  return { props: { slug: "hello" } };
}

export default function PostPage(props: PageProps): string {
  return props.slug;
}
