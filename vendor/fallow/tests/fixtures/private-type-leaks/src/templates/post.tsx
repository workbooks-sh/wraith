// Gatsby template. Local PageProps shared by `Head` and the default page
// component export.
type PageProps = {
  data: { title: string };
};

export function Head(props: PageProps): string {
  return props.data.title;
}

export default function PostTemplate(props: PageProps): string {
  return props.data.title;
}
