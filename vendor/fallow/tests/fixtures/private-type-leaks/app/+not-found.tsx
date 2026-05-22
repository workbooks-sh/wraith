// Expo Router special file. Exercises the `app/**/+*.{ts,tsx,js,jsx}` skip
// pattern. Local NotFoundProps shared by the default screen and a metadata
// helper export.
type NotFoundProps = {
  title: string;
};

export function metadata(props: NotFoundProps): { title: string } {
  return { title: props.title };
}

export default function NotFoundScreen(props: NotFoundProps): string {
  return props.title;
}
