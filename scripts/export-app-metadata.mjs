#!/usr/bin/env node

import { appendFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const packageMetadata = JSON.parse(
  readFileSync(join(root, "package.json"), "utf8"),
);

if (!packageMetadata.productName || !packageMetadata.version || !packageMetadata.author) {
  throw new Error("package.json must define productName, version, and author");
}

if (process.env.GITHUB_ENV) {
  appendFileSync(
    process.env.GITHUB_ENV,
    `APP_NAME=${packageMetadata.productName}\nAPP_VERSION=${packageMetadata.version}\nAPP_AUTHOR=${packageMetadata.author}\n`,
    "utf8",
  );
}

console.log(`${packageMetadata.productName} ${packageMetadata.version}`);
