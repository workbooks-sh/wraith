export default function IndexScreen() {
    return null;
}

export function ErrorBoundary() {
    return null;
}

export async function loader() {
    return { greeting: "hello" };
}

export function generateStaticParams() {
    return [{ slug: "hello-world" }];
}

export const unusedIndexHelper = "unused";
