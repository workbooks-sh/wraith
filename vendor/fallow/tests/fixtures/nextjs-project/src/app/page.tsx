export const revalidate = 60;
export const dynamic = 'force-static';
export async function generateMetadata() {
  return { title: 'Home' };
}
export const viewport = { themeColor: '#ffffff' };

export default function Page() {
  return <div>Hello</div>;
}
