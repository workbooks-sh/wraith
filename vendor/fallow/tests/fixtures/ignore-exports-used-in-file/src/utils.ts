export function publicApi(): number {
  return usedOnlyHere() + 1;
}

export function usedOnlyHere(): number {
  return 1;
}

export function completelyUnused(): number {
  return 2;
}

export type LocallyUsedType = {
  value: string;
};

type LocalConsumer = LocallyUsedType;

export const localConsumer: LocalConsumer = {
  value: 'local',
};

export type DeadType = {
  value: number;
};

// Regression: `function name() {}; export { name };` declares `name` and
// references it only inside the export specifier. The export specifier
// identifier must NOT count as a same-file use (it IS the export site, not
// a consumer of it), so this export must still be reported when
// ignoreExportsUsedInFile is enabled.
function specifierOnlyExport(): number {
  return 1;
}
export { specifierOnlyExport };

function aliasedSpecifierOnlyExport(): number {
  return 2;
}
export { aliasedSpecifierOnlyExport as aliasedSpecifierExportAlias };

// Regression: `export default name;` references `defaultViaIdentifier` only
// at the default-export site. Same rule.
function defaultViaIdentifier(): number {
  return 3;
}
export default defaultViaIdentifier;
