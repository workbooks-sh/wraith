import * as fs from "node:fs";
import * as path from "node:path";
import Mocha from "mocha";

const collectTestFiles = (dir: string): string[] => {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  return entries.flatMap((entry) => {
    const resolved = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return collectTestFiles(resolved);
    }
    return entry.name.endsWith(".test.js") ? [resolved] : [];
  });
};

export async function run(): Promise<void> {
  const mocha = new Mocha({
    ui: "bdd",
    color: true,
    timeout: 20_000,
  });

  const testsRoot = __dirname;
  for (const file of collectTestFiles(testsRoot)) {
    if (file !== __filename) {
      mocha.addFile(file);
    }
  }

  await new Promise<void>((resolve, reject) => {
    mocha.run((failures) => {
      if (failures > 0) {
        reject(new Error(`${failures} test(s) failed.`));
        return;
      }
      resolve();
    });
  });
}
