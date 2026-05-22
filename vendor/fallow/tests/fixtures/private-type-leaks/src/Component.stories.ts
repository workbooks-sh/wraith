// Storybook story file using the canonical pattern: a private `Story` type
// alias used by every exported story. Without the .stories.* skip this would
// generate one private-type-leak per export.

type StoryObj<_T> = { args: Record<string, unknown> };
type Story = StoryObj<{ label: string }>;

export const Default: Story = { args: { label: "Hello" } };

export const LongLabel: Story = { args: { label: "Long label example" } };
