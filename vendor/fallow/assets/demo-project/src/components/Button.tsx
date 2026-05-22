export const Button = ({ label }: { label: string }) => label;

export const ButtonVariant = {
  primary: "primary",
  secondary: "secondary",
} as const;

const deprecated = () => null;

const check = false;
export const deprecatedRender = (checked: boolean) => null;

const checked = true;
