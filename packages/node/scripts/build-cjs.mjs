import { mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const inputDir = join(packageDir, ".cjs-build");
const outputDir = join(packageDir, "dist");

async function emitCommonJsFiles(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const inputPath = join(directory, entry.name);
    if (entry.isDirectory()) {
      await emitCommonJsFiles(inputPath);
      continue;
    }

    const extension = extname(entry.name);
    if (extension !== ".js" && extension !== ".map") continue;

    const relativePath = relative(inputDir, inputPath);
    const outputPath = join(
      outputDir,
      relativePath.replace(/\.js(\.map)?$/, ".cjs$1"),
    );
    let contents = await readFile(inputPath, "utf8");
    contents = contents.replace(
      /require\((['"])(\.\.?\/[^'"]+)\.js\1\)/g,
      "require($1$2.cjs$1)",
    );
    contents = contents.replace(/\.js\.map/g, ".cjs.map");

    await mkdir(dirname(outputPath), { recursive: true });
    await writeFile(outputPath, contents);
  }
}

try {
  await emitCommonJsFiles(inputDir);
} finally {
  await rm(inputDir, { recursive: true, force: true });
}
