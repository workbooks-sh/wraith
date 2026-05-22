export default function handler() {
  return new Response("hello");
}

export const config = { method: "GET" };
export const unusedFunctionHelper = () => null;
