type Props = {
  label: string;
};

interface Options {
  enabled: boolean;
}

type InternalState = {
  value: string;
};

export type PublicBacking = {
  value: string;
};

export function Component(props: Props): PublicBacking {
  return { value: props.label };
}

export class Service {
  #state: InternalState = { value: "ready" };

  configure(options: Options): void {
    void this.#state;
    void options;
  }
}

export function UsesExportedType(backing: PublicBacking): PublicBacking {
  return backing;
}
